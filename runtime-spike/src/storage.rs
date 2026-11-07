//! Initial local storage; promotion and governance are not implemented.
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct Storage {
    conn: Connection,
}

/// Caller-asserted synthetic text; no model execution or provenance attestation is implied.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticTraceInput {
    pub schema: String,
    pub actor: String,
    pub source: String,
    pub prompt: String,
    pub response: String,
    pub preference: Option<SyntheticPreference>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticPreference {
    pub preferred: String,
    pub rejected: String,
    pub source: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Storage {
    #[cfg(unix)]
    fn ensure_observation_in_current_tenure(
        conn: &Connection,
        domain: &str,
        observation: i64,
    ) -> Result<(), String> {
        let (observed,transition):(Option<i64>,Option<i64>)=conn.query_row("SELECT (SELECT e.seq FROM ledger_events e WHERE e.kind='event_observation' AND e.reference=?1), (SELECT MAX(e.seq) FROM event_champion_transitions t JOIN ledger_events e ON e.kind='event_champion_transition' AND e.reference=t.id WHERE t.domain=?2)",params![observation,domain],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        if !matches!((observed,transition),(Some(o),Some(t)) if o>t) {
            return Err("source observation predates current champion transition".into());
        }
        Ok(())
    }

    #[cfg(unix)]
    fn validate_container_observation_history_conn(
        conn: &Connection,
        seq: i64,
        value: &serde_json::Value,
    ) -> Result<(), String> {
        let domain = value["domain"].as_str().ok_or("missing domain")?;
        let observation = value["observation_id"]
            .as_i64()
            .ok_or("missing observation")?;
        let observed: i64 = conn
            .query_row(
                "SELECT seq FROM ledger_events WHERE kind='event_observation' AND reference=?1",
                [observation],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let (transition,champion):(i64,String)=conn.query_row("SELECT e.seq,t.next_id FROM event_champion_transitions t JOIN ledger_events e ON e.kind='event_champion_transition' AND e.reference=t.id WHERE t.domain=?1 AND e.seq<?2 ORDER BY e.seq DESC LIMIT 1",params![domain,seq],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        let count:i64=conn.query_row("SELECT COUNT(*) FROM event_champion_transitions t JOIN ledger_events e ON e.kind='event_champion_transition' AND e.reference=t.id WHERE t.domain=?1 AND e.seq<?2",params![domain,seq],|r|r.get(0)).map_err(|e|e.to_string())?;
        if observed <= transition
            || observed >= seq
            || count <= 0
            || value["champion_id"].as_str() != Some(champion.as_str())
            || value["champion_generation"].as_i64() != Some(count - 1)
        {
            return Err("container observation champion history mismatch".into());
        }
        Ok(())
    }
    #[cfg(unix)]
    fn container_observation_source_conn(
        conn: &Connection,
        domain: &str,
        observation: i64,
    ) -> Result<serde_json::Value, String> {
        if domain.trim().is_empty() || domain.len() > 256 || observation <= 0 {
            return Err("invalid observation scope".into());
        }
        if !crate::ledger::event_observation_binding_matches(conn, observation)? {
            return Err("observation evidence binding failure".into());
        }
        let (champion,shadow,prior,event,champion_result,champion_error,shadow_result,shadow_error):(String,String,String,String,Option<String>,Option<String>,Option<String>,Option<String>) = conn.query_row(
            "SELECT champion_id,shadow_id,prior_json,event_json,champion_json,champion_error,shadow_json,shadow_error FROM event_observations WHERE id=?1 AND domain=?2",params![observation,domain],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).map_err(|e|e.to_string())?;
        if champion_result.is_none()
            || shadow_result.is_none()
            || champion_error.is_some()
            || shadow_error.is_some()
        {
            return Err("source observation incomplete or failed".into());
        }
        let prior: serde_json::Value = serde_json::from_str(&prior).map_err(|e| e.to_string())?;
        let event: serde_json::Value = serde_json::from_str(&event).map_err(|e| e.to_string())?;
        let source = serde_json::json!({"domain":domain,"observation_id":observation,"champion_id":champion,"shadow_id":shadow,"prior":prior,"event":event});
        crate::container_observation::decode_source(&source)?;
        Ok(source)
    }

    #[cfg(unix)]
    pub(crate) fn load_container_observation_source(
        &mut self,
        domain: &str,
        observation: i64,
    ) -> Result<(serde_json::Value, (i64, String), i64), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let source = Self::container_observation_source_conn(&tx, domain, observation)?;
        let (champion,generation,semantics):(String,i64,String)=tx.query_row("SELECT c.champion_id,c.generation,r.state_semantics FROM event_champions c JOIN event_fixture_pins p ON p.domain=c.domain JOIN event_replay_runs r ON r.id=p.replay_id WHERE c.domain=?1",[domain],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        if source["champion_id"].as_str() != Some(champion.as_str()) || semantics != "measured-v2" {
            return Err("source observation is not from the current measured champion".into());
        }
        Self::ensure_observation_in_current_tenure(&tx, domain, observation)?;
        let checkpoint = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((source, checkpoint, generation))
    }

    #[cfg(unix)]
    pub(crate) fn save_container_observation_experiment(
        &mut self,
        value: &serde_json::Value,
        checkpoint: (i64, &str),
        time: &str,
    ) -> Result<i64, String> {
        let payload = value.to_string();
        if payload.len() > 4_194_304 {
            return Err("container observation evidence too large".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let head: (i64, String) = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if (head.0, head.1.as_str()) != checkpoint {
            return Err("ledger changed during container observation".into());
        }
        let domain = value["domain"].as_str().ok_or("missing domain")?;
        let observation = value["observation_id"]
            .as_i64()
            .ok_or("missing observation")?;
        let source = Self::container_observation_source_conn(&tx, domain, observation)?;
        let current: (String, i64) = tx
            .query_row(
                "SELECT champion_id,generation FROM event_champions WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if value["champion_id"].as_str() != Some(current.0.as_str())
            || value["champion_generation"].as_i64() != Some(current.1)
        {
            return Err("champion changed during container observation".into());
        }
        crate::container_observation::validate(value, &source)?;
        let seq = crate::ledger::append_container_observation_experiment(&tx, &payload, time)?;
        Self::validate_container_observation_history_conn(&tx, seq, value)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(seq)
    }

    #[cfg(unix)]
    fn inspect_container_observation_experiment_conn(
        conn: &Connection,
        seq: i64,
    ) -> Result<serde_json::Value, String> {
        let payload:String=conn.query_row("SELECT payload FROM ledger_events WHERE seq=?1 AND kind='container_observation_experiment'",[seq],|r|r.get(0)).map_err(|e|e.to_string())?;
        let value: serde_json::Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        let source = Self::container_observation_source_conn(
            conn,
            value["domain"].as_str().ok_or("missing domain")?,
            value["observation_id"]
                .as_i64()
                .ok_or("missing observation")?,
        )?;
        crate::container_observation::validate(&value, &source)?;
        Self::validate_container_observation_history_conn(conn, seq, &value)?;
        Ok(value)
    }

    /// Inspect a historical-input experiment; its sequence is not an ordinary observation ID.
    #[cfg(unix)]
    pub fn inspect_container_observation_experiment(
        &mut self,
        seq: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let value = Self::inspect_container_observation_experiment_conn(&tx, seq)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    /// Stable snapshot for an experimental container stream. Its events never enter review tables.
    #[cfg(unix)]
    pub(crate) fn load_container_shadow_stream(
        &mut self,
        domain: &str,
        stream: &str,
        capacity: usize,
    ) -> Result<(crate::event_state::EventState, (i64, String), String, i64), String> {
        if domain.trim().is_empty()
            || domain.len() > 256
            || stream.trim().is_empty()
            || stream.len() > 256
        {
            return Err("invalid container shadow stream".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (champion, generation, semantics): (String, i64, String) = tx.query_row("SELECT c.champion_id,c.generation,r.state_semantics FROM event_champions c JOIN event_fixture_pins p ON p.domain=c.domain JOIN event_replay_runs r ON r.id=p.replay_id WHERE c.domain=?1", [domain], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|e| e.to_string())?;
        if semantics != "measured-v2" {
            return Err("container shadow requires measured-v2 domain".into());
        }
        let checkpoint = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let state = Self::container_shadow_state(&tx, domain, stream, capacity)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((state, checkpoint, champion, generation))
    }

    #[cfg(unix)]
    fn container_shadow_state(
        conn: &Connection,
        domain: &str,
        stream: &str,
        capacity: usize,
    ) -> Result<crate::event_state::EventState, String> {
        let mut state = crate::event_state::EventState::new(capacity)?;
        let mut stmt = conn.prepare("SELECT payload FROM ledger_events WHERE kind='container_shadow_experiment' ORDER BY seq").map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let payload: String = row.get(0).map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_str(&payload).map_err(|e| e.to_string())?;
            if value["domain"].as_str() == Some(domain) && value["stream"].as_str() == Some(stream)
            {
                state = crate::container_shadow::validate(&value, &state)?;
            }
        }
        Ok(state)
    }

    #[cfg(unix)]
    pub(crate) fn save_container_shadow_experiment(
        &mut self,
        value: &serde_json::Value,
        prior: &crate::event_state::EventState,
        checkpoint: (i64, &str),
        champion: &str,
        generation: i64,
        time: &str,
    ) -> Result<i64, String> {
        let payload = value.to_string();
        if payload.len() > 4_194_304 {
            return Err("container shadow evidence too large".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let head: (i64, String) = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if (head.0, head.1.as_str()) != checkpoint {
            return Err("ledger changed during container shadow".into());
        }
        let domain = value["domain"].as_str().ok_or("missing domain")?;
        let stream = value["stream"].as_str().ok_or("missing stream")?;
        let current: (String, i64) = tx
            .query_row(
                "SELECT champion_id,generation FROM event_champions WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if current != (champion.to_owned(), generation)
            || value["champion_id"].as_str() != Some(champion)
            || value["champion_generation"].as_i64() != Some(generation)
        {
            return Err("champion changed during container shadow".into());
        }
        let stored = Self::container_shadow_state(&tx, domain, stream, prior.capacity())?;
        if stored.encode() != prior.encode() {
            return Err("container shadow stream changed".into());
        }
        crate::container_shadow::validate(value, prior)?;
        let seq = crate::ledger::append_container_shadow_experiment(&tx, &payload, time)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(seq)
    }

    /// Inspect a container experiment by its ledger sequence, not an observation ID.
    #[cfg(unix)]
    pub fn inspect_container_shadow_experiment(
        &mut self,
        seq: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let payload: String = tx.query_row("SELECT payload FROM ledger_events WHERE seq=?1 AND kind='container_shadow_experiment'", [seq], |r|r.get(0)).map_err(|e|e.to_string())?;
        let value: serde_json::Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        let domain = value["domain"].as_str().ok_or("missing domain")?;
        let stream = value["stream"].as_str().ok_or("missing stream")?;
        let capacity = usize::try_from(value["capacity"].as_u64().ok_or("missing capacity")?)
            .map_err(|e| e.to_string())?;
        Self::container_shadow_state(&tx, domain, stream, capacity)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::from_connection(Connection::open(path).map_err(|e| e.to_string())?)
    }

    /// Open a current-schema database without running migrations or allowing writes.
    pub fn open_read_only(path: &Path) -> Result<Self, String> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version != 37 {
            return Err("read-only export requires schema version 37; migrate separately".into());
        }
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    pub fn from_connection(mut conn: Connection) -> Result<Self, String> {
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|e| e.to_string())?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| e.to_string())?;
        match version {
            0 => {
                let count: i64 = tx
                    .query_row(
                        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                if count != 0 {
                    return Err("unversioned nonempty database".into());
                }
                tx.execute_batch("CREATE TABLE candidates (id TEXT PRIMARY KEY, artifact BLOB NOT NULL, runtime TEXT NOT NULL, abi_version INTEGER NOT NULL); PRAGMA user_version=1;").map_err(|e| e.to_string())?;
            }
            1..=37 => (),
            _ => return Err(format!("unsupported schema version {version}")),
        }
        if version <= 1 {
            tx.execute_batch("CREATE TABLE replay_runs (id INTEGER PRIMARY KEY, candidate_id TEXT NOT NULL REFERENCES candidates(id), pack_version TEXT NOT NULL, pack_digest TEXT NOT NULL, passed INTEGER NOT NULL, started_at_unix_ns TEXT NOT NULL, environment TEXT NOT NULL); CREATE TABLE replay_cases (run_id INTEGER NOT NULL REFERENCES replay_runs(id), case_id TEXT NOT NULL, input INTEGER NOT NULL, expected INTEGER NOT NULL, actual INTEGER, error TEXT, passed INTEGER NOT NULL, PRIMARY KEY(run_id, case_id)); PRAGMA user_version=2;").map_err(|e| e.to_string())?;
        }
        if version <= 2 {
            tx.execute_batch("CREATE TABLE shadow_observations (id INTEGER PRIMARY KEY, champion_id TEXT NOT NULL REFERENCES candidates(id), shadow_id TEXT NOT NULL REFERENCES candidates(id), input INTEGER NOT NULL, champion_result INTEGER, champion_error TEXT, shadow_result INTEGER, shadow_error TEXT, champion_us INTEGER NOT NULL, shadow_us INTEGER NOT NULL, observed_unix_ns TEXT NOT NULL); PRAGMA user_version=3;").map_err(|e| e.to_string())?;
        }
        if version <= 3 {
            tx.execute_batch("CREATE TABLE review_records (id INTEGER PRIMARY KEY, candidate_id TEXT NOT NULL REFERENCES candidates(id), replay_id INTEGER NOT NULL REFERENCES replay_runs(id), shadow_id INTEGER NOT NULL REFERENCES shadow_observations(id), actor TEXT NOT NULL, action TEXT NOT NULL CHECK(action IN ('approve','reject')), created_unix_ns TEXT NOT NULL); PRAGMA user_version=4;").map_err(|e| e.to_string())?;
        }
        if version <= 4 {
            tx.execute_batch("CREATE TABLE IF NOT EXISTS ledger_events (seq INTEGER PRIMARY KEY, kind TEXT NOT NULL, reference INTEGER NOT NULL, payload TEXT NOT NULL, created_ns TEXT NOT NULL, prev TEXT NOT NULL, hash TEXT NOT NULL); PRAGMA user_version=5;").map_err(|e| e.to_string())?;
        }
        if version <= 5 {
            tx.execute_batch("CREATE TABLE review_outcomes (review_id INTEGER PRIMARY KEY REFERENCES review_records(id), status TEXT NOT NULL CHECK(status IN ('validated','falsified')), actor TEXT NOT NULL, source TEXT NOT NULL, created_unix_ns TEXT NOT NULL); PRAGMA user_version=6;").map_err(|e| e.to_string())?;
        }
        if version <= 6 {
            tx.execute_batch("CREATE TABLE window_observations (id INTEGER PRIMARY KEY, champion_id TEXT NOT NULL REFERENCES candidates(id), shadow_id TEXT NOT NULL REFERENCES candidates(id), prior_window BLOB NOT NULL, input INTEGER NOT NULL, champion_result INTEGER, champion_error TEXT, shadow_result INTEGER, shadow_error TEXT, champion_us INTEGER NOT NULL, shadow_us INTEGER NOT NULL, observed_unix_ns TEXT NOT NULL); PRAGMA user_version=7;").map_err(|e| e.to_string())?;
        }
        if version <= 7 {
            tx.execute_batch("CREATE TABLE window_replay_runs (id INTEGER PRIMARY KEY, candidate_id TEXT NOT NULL REFERENCES candidates(id), pack_version TEXT NOT NULL, pack_digest TEXT NOT NULL, passed INTEGER NOT NULL, started_unix_ns TEXT NOT NULL, environment TEXT NOT NULL); CREATE TABLE window_replay_cases (run_id INTEGER NOT NULL REFERENCES window_replay_runs(id), case_id TEXT NOT NULL, prior_window BLOB NOT NULL, input INTEGER NOT NULL, expected INTEGER NOT NULL, actual INTEGER, error TEXT, passed INTEGER NOT NULL, PRIMARY KEY(run_id,case_id)); PRAGMA user_version=8;").map_err(|e|e.to_string())?;
        }
        if version <= 8 {
            tx.execute_batch("CREATE TABLE window_review_records (id INTEGER PRIMARY KEY, candidate_id TEXT NOT NULL REFERENCES candidates(id), replay_id INTEGER NOT NULL REFERENCES window_replay_runs(id), shadow_id INTEGER NOT NULL REFERENCES window_observations(id), actor TEXT NOT NULL, action TEXT NOT NULL CHECK(action IN ('approve','reject')), created_unix_ns TEXT NOT NULL); PRAGMA user_version=9;").map_err(|e|e.to_string())?;
        }
        if version <= 9 {
            tx.execute_batch("CREATE TABLE window_review_outcomes (review_id INTEGER PRIMARY KEY REFERENCES window_review_records(id), status TEXT NOT NULL CHECK(status IN ('validated','falsified')), actor TEXT NOT NULL, source TEXT NOT NULL, created_unix_ns TEXT NOT NULL); PRAGMA user_version=10;").map_err(|e|e.to_string())?;
        }
        if version <= 10 {
            tx.execute_batch("CREATE TABLE window_streams (name TEXT PRIMARY KEY, capacity INTEGER NOT NULL, generation INTEGER NOT NULL, window BLOB NOT NULL); PRAGMA user_version=11;").map_err(|e|e.to_string())?;
        }
        if version <= 11 {
            tx.execute_batch("CREATE TABLE window_review_cards (review_id INTEGER PRIMARY KEY REFERENCES window_review_records(id), human TEXT NOT NULL, bridge TEXT NOT NULL, proposal TEXT NOT NULL, created_unix_ns TEXT NOT NULL); PRAGMA user_version=12;").map_err(|e|e.to_string())?;
        }
        if version <= 12 {
            tx.execute_batch("CREATE TABLE window_fixture_pins (domain TEXT PRIMARY KEY, pack_version TEXT NOT NULL, pack_digest TEXT NOT NULL, actor TEXT NOT NULL, created_unix_ns TEXT NOT NULL); PRAGMA user_version=13;").map_err(|e|e.to_string())?;
        }
        if version <= 13 {
            tx.execute_batch("ALTER TABLE window_replay_cases ADD COLUMN ordinal INTEGER; UPDATE window_replay_cases SET ordinal=(SELECT COUNT(*)-1 FROM window_replay_cases AS earlier WHERE earlier.run_id=window_replay_cases.run_id AND earlier.rowid<=window_replay_cases.rowid); CREATE UNIQUE INDEX window_replay_ordinal ON window_replay_cases(run_id,ordinal); PRAGMA user_version=14;").map_err(|e|e.to_string())?;
        }
        if version <= 14 {
            tx.execute_batch("CREATE TABLE window_fixture_pins_v15 (id INTEGER PRIMARY KEY, domain TEXT NOT NULL UNIQUE, pack_version TEXT NOT NULL, pack_digest TEXT NOT NULL, actor TEXT NOT NULL, created_unix_ns TEXT NOT NULL); INSERT INTO window_fixture_pins_v15(domain,pack_version,pack_digest,actor,created_unix_ns) SELECT domain,pack_version,pack_digest,actor,created_unix_ns FROM window_fixture_pins ORDER BY domain; DROP TABLE window_fixture_pins; ALTER TABLE window_fixture_pins_v15 RENAME TO window_fixture_pins; PRAGMA user_version=15;").map_err(|e|e.to_string())?;
        }
        if version <= 15 {
            tx.execute_batch("CREATE TABLE champion_domains(domain TEXT PRIMARY KEY, champion_id TEXT NOT NULL REFERENCES candidates(id), generation INTEGER NOT NULL); CREATE TABLE champion_transitions(id INTEGER PRIMARY KEY, domain TEXT NOT NULL, previous_id TEXT NOT NULL, next_id TEXT NOT NULL REFERENCES candidates(id), review_id INTEGER NOT NULL, actor TEXT NOT NULL, created_unix_ns TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('bootstrap','promote'))); PRAGMA user_version=16;").map_err(|e|e.to_string())?;
        }
        if version <= 16 {
            tx.execute_batch("CREATE TABLE champion_transitions_v17(id INTEGER PRIMARY KEY, domain TEXT NOT NULL, previous_id TEXT NOT NULL, next_id TEXT NOT NULL REFERENCES candidates(id), review_id INTEGER NOT NULL, actor TEXT NOT NULL, created_unix_ns TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('bootstrap','promote','rollback'))); INSERT INTO champion_transitions_v17 SELECT * FROM champion_transitions; DROP TABLE champion_transitions; ALTER TABLE champion_transitions_v17 RENAME TO champion_transitions; PRAGMA user_version=17;").map_err(|e|e.to_string())?;
        }
        if version <= 17 {
            tx.execute_batch("CREATE TABLE event_observations (id INTEGER PRIMARY KEY, champion_id TEXT NOT NULL REFERENCES candidates(id), shadow_id TEXT NOT NULL REFERENCES candidates(id), prior_json TEXT NOT NULL, event_json TEXT NOT NULL, champion_json TEXT, champion_error TEXT, shadow_json TEXT, shadow_error TEXT, champion_us INTEGER NOT NULL, shadow_us INTEGER NOT NULL, observed_unix_ns TEXT NOT NULL); PRAGMA user_version=18;").map_err(|e|e.to_string())?;
        }
        if version <= 18 {
            tx.execute_batch("CREATE TABLE event_replay_runs (id INTEGER PRIMARY KEY, candidate_id TEXT NOT NULL REFERENCES candidates(id), pack_version TEXT NOT NULL, pack_digest TEXT NOT NULL, passed INTEGER NOT NULL, started_unix_ns TEXT NOT NULL, environment TEXT NOT NULL); CREATE TABLE event_replay_cases (run_id INTEGER NOT NULL REFERENCES event_replay_runs(id), ordinal INTEGER NOT NULL, case_id TEXT NOT NULL, window_json TEXT NOT NULL, event_json TEXT NOT NULL, expected_json TEXT NOT NULL, actual_json TEXT, error TEXT, passed INTEGER NOT NULL, PRIMARY KEY(run_id,ordinal), UNIQUE(run_id,case_id)); PRAGMA user_version=19;").map_err(|e| e.to_string())?;
        }
        if version <= 19 {
            tx.execute_batch("CREATE TABLE event_streams(name TEXT PRIMARY KEY, capacity INTEGER NOT NULL, generation INTEGER NOT NULL, latencies BLOB NOT NULL); PRAGMA user_version=20;").map_err(|e|e.to_string())?;
        }
        if version <= 20 {
            tx.execute_batch("CREATE TABLE event_reviews(id INTEGER PRIMARY KEY,candidate_id TEXT NOT NULL REFERENCES candidates(id),replay_id INTEGER NOT NULL REFERENCES event_replay_runs(id),observation_id INTEGER NOT NULL REFERENCES event_observations(id),actor TEXT NOT NULL,action TEXT NOT NULL CHECK(action IN ('approve','reject')),human TEXT NOT NULL,bridge TEXT NOT NULL,proposal TEXT NOT NULL,created_unix_ns TEXT NOT NULL); PRAGMA user_version=21;").map_err(|e|e.to_string())?;
        }
        if version <= 21 {
            tx.execute_batch("CREATE TABLE event_review_outcomes(review_id INTEGER PRIMARY KEY REFERENCES event_reviews(id),status TEXT NOT NULL CHECK(status IN ('validated','falsified')),actor TEXT NOT NULL,source TEXT NOT NULL,created_unix_ns TEXT NOT NULL); PRAGMA user_version=22;").map_err(|e|e.to_string())?;
        }
        if version <= 22 {
            tx.execute_batch("CREATE TABLE event_fixture_pins(id INTEGER PRIMARY KEY,domain TEXT NOT NULL UNIQUE,replay_id INTEGER NOT NULL REFERENCES event_replay_runs(id),pack_version TEXT NOT NULL,pack_digest TEXT NOT NULL,actor TEXT NOT NULL,created_unix_ns TEXT NOT NULL); PRAGMA user_version=23;").map_err(|e|e.to_string())?;
        }
        if version <= 23 {
            tx.execute_batch("CREATE TABLE event_champions(domain TEXT PRIMARY KEY,champion_id TEXT NOT NULL REFERENCES candidates(id),generation INTEGER NOT NULL); CREATE TABLE event_champion_transitions(id INTEGER PRIMARY KEY,domain TEXT NOT NULL,previous_id TEXT NOT NULL,next_id TEXT NOT NULL REFERENCES candidates(id),reference INTEGER NOT NULL,actor TEXT NOT NULL,created_unix_ns TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('bootstrap','promote','rollback'))); PRAGMA user_version=24;").map_err(|e|e.to_string())?;
        }
        if version <= 24 {
            tx.execute_batch("CREATE TABLE event_stream_receipts(id INTEGER PRIMARY KEY,name TEXT NOT NULL REFERENCES event_streams(name),capacity INTEGER NOT NULL,generation INTEGER NOT NULL,prior BLOB NOT NULL,next BLOB NOT NULL,observation_id INTEGER REFERENCES event_observations(id),created_unix_ns TEXT NOT NULL,UNIQUE(name,generation)); PRAGMA user_version=25;").map_err(|e|e.to_string())?;
        }
        if version <= 25 {
            tx.execute_batch("CREATE TABLE measured_streams(name TEXT PRIMARY KEY,capacity INTEGER NOT NULL,generation INTEGER NOT NULL,state BLOB NOT NULL); CREATE TABLE measured_stream_receipts(id INTEGER PRIMARY KEY,name TEXT NOT NULL REFERENCES measured_streams(name),capacity INTEGER NOT NULL,generation INTEGER NOT NULL,prior BLOB NOT NULL,next BLOB NOT NULL,observation_id INTEGER REFERENCES event_observations(id),created_unix_ns TEXT NOT NULL,UNIQUE(name,generation)); PRAGMA user_version=26;").map_err(|e|e.to_string())?;
        }
        if version <= 26 {
            tx.execute_batch("ALTER TABLE event_replay_runs ADD COLUMN state_semantics TEXT NOT NULL DEFAULT 'legacy-v1'; PRAGMA user_version=27;").map_err(|e|e.to_string())?;
        }
        if version <= 27 {
            tx.execute_batch(
                "ALTER TABLE event_observations ADD COLUMN domain TEXT; PRAGMA user_version=28;",
            )
            .map_err(|e| e.to_string())?;
        }
        if version <= 28 {
            tx.execute_batch("CREATE TABLE synthetic_event_candidates(id INTEGER PRIMARY KEY,candidate_id TEXT NOT NULL UNIQUE REFERENCES candidates(id),created_unix_ns TEXT NOT NULL); PRAGMA user_version=29;").map_err(|e|e.to_string())?;
        }
        if version <= 29 {
            tx.execute_batch("CREATE TABLE synthetic_window_candidates(id INTEGER PRIMARY KEY,candidate_id TEXT NOT NULL UNIQUE REFERENCES candidates(id),created_unix_ns TEXT NOT NULL); PRAGMA user_version=30;").map_err(|e|e.to_string())?;
        }
        if version <= 30 {
            tx.execute_batch("CREATE TABLE synthetic_scalar_candidates(id INTEGER PRIMARY KEY,candidate_id TEXT NOT NULL UNIQUE REFERENCES candidates(id),created_unix_ns TEXT NOT NULL); PRAGMA user_version=31;").map_err(|e|e.to_string())?;
        }
        if version <= 31 {
            tx.execute_batch("ALTER TABLE event_replay_runs ADD COLUMN worker_path TEXT; ALTER TABLE event_replay_runs ADD COLUMN worker_sha256 TEXT; ALTER TABLE event_observations ADD COLUMN worker_path TEXT; ALTER TABLE event_observations ADD COLUMN worker_sha256 TEXT; PRAGMA user_version=32;").map_err(|e|e.to_string())?;
        }
        if version <= 32 {
            tx.execute_batch("CREATE TABLE event_training_traces(id INTEGER PRIMARY KEY,review_id INTEGER NOT NULL REFERENCES event_reviews(id),actor TEXT NOT NULL,source TEXT NOT NULL,prompt TEXT NOT NULL,response TEXT NOT NULL,preferred TEXT,rejected TEXT,pair_source TEXT,created_unix_ns TEXT NOT NULL,CHECK((preferred IS NULL AND rejected IS NULL AND pair_source IS NULL) OR (preferred IS NOT NULL AND rejected IS NOT NULL AND pair_source IS NOT NULL))); PRAGMA user_version=33;").map_err(|e|e.to_string())?;
        }
        if version <= 33 {
            tx.execute_batch("CREATE TABLE event_followup_pins(id INTEGER PRIMARY KEY,domain TEXT NOT NULL,candidate_id TEXT NOT NULL REFERENCES candidates(id),replay_id INTEGER NOT NULL REFERENCES event_replay_runs(id),case_id TEXT NOT NULL,actor TEXT NOT NULL,created_unix_ns TEXT NOT NULL,UNIQUE(domain,candidate_id)); PRAGMA user_version=34;").map_err(|e|e.to_string())?;
        }
        if version <= 34 {
            tx.execute_batch("CREATE TABLE event_followup_receipts(id INTEGER PRIMARY KEY,review_id INTEGER NOT NULL REFERENCES event_reviews(id),observation_id INTEGER NOT NULL REFERENCES event_observations(id),pin_id INTEGER NOT NULL REFERENCES event_followup_pins(id),status TEXT NOT NULL CHECK(status IN ('match','verdict_mismatch','input_mismatch','worker_error')),created_unix_ns TEXT NOT NULL,UNIQUE(review_id,observation_id,pin_id)); PRAGMA user_version=35;").map_err(|e|e.to_string())?;
        }
        if version <= 35 {
            tx.execute_batch("CREATE TABLE event_attestation_receipts(id INTEGER PRIMARY KEY,trace_id INTEGER NOT NULL REFERENCES event_training_traces(id),trace_authority TEXT NOT NULL,outcome_authority TEXT NOT NULL,trace_payload BLOB NOT NULL,outcome_payload BLOB NOT NULL,trace_signature BLOB NOT NULL,outcome_signature BLOB NOT NULL,authority_digest TEXT NOT NULL,checkpoint_seq INTEGER NOT NULL,checkpoint_hash TEXT NOT NULL,created_unix_ns TEXT NOT NULL,UNIQUE(trace_id,trace_signature,outcome_signature,authority_digest,checkpoint_seq,checkpoint_hash)); PRAGMA user_version=36;").map_err(|e|e.to_string())?;
        }
        if version <= 36 {
            tx.execute_batch("CREATE TABLE event_preference_receipts(id INTEGER PRIMARY KEY,trace_id INTEGER NOT NULL REFERENCES event_training_traces(id),authority TEXT NOT NULL,payload BLOB NOT NULL,signature BLOB NOT NULL,authority_digest TEXT NOT NULL,checkpoint_seq INTEGER NOT NULL,checkpoint_hash TEXT NOT NULL,created_unix_ns TEXT NOT NULL,UNIQUE(trace_id,signature,authority_digest,checkpoint_seq,checkpoint_hash)); PRAGMA user_version=37;").map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let file: String = conn
            .query_row("PRAGMA database_list", [], |r| r.get(2))
            .map_err(|e| e.to_string())?;
        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if !file.is_empty() && !mode.eq_ignore_ascii_case("wal") {
            return Err("WAL mode unavailable".into());
        }
        Ok(Self { conn })
    }

    /// Non-executing scalar artifact intake.
    pub fn register(&self, artifact: &[u8]) -> Result<String, String> {
        if artifact.is_empty() || artifact.len() > 65_536 {
            return Err("invalid artifact size".into());
        }
        let id = digest(artifact);
        self.conn.execute("INSERT OR IGNORE INTO candidates(id,artifact,runtime,abi_version) VALUES (?1,?2,'wasmi-experimental',1)",params![id,artifact]).map_err(|e|e.to_string())?;
        let (bytes, runtime, abi): (Vec<u8>, String, i64) = self
            .conn
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if bytes != artifact || runtime != "wasmi-experimental" || abi != 1 {
            return Err("candidate identity or ABI mismatch".into());
        }
        Ok(id)
    }
    /// Caller asserts these are fixed synthetic test bytes; this is not attestation or a sandbox.
    pub fn register_synthetic_scalar(&self, artifact: &[u8]) -> Result<String, String> {
        if artifact.is_empty() || artifact.len() > 65_536 {
            return Err("invalid artifact size".into());
        }
        let id = digest(artifact);
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let inserted=tx.execute("INSERT OR IGNORE INTO candidates(id,artifact,runtime,abi_version) VALUES (?1,?2,'wasmi-experimental',1)",params![id,artifact]).map_err(|e|e.to_string())?;
        if inserted == 0 {
            let synthetic:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM synthetic_scalar_candidates WHERE candidate_id=?1)",[&id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !synthetic {
                return Err("existing candidate is not classified synthetic".into());
            }
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
                .to_string();
            tx.execute("INSERT INTO synthetic_scalar_candidates(candidate_id,created_unix_ns) VALUES (?1,?2)",params![id,now]).map_err(|e|e.to_string())?;
            crate::ledger::append_synthetic_scalar_candidate(&tx, tx.last_insert_rowid())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.artifact_for_abi(&id, 1)?;
        Ok(id)
    }
    /// Non-executing window artifact intake.
    pub fn register_window(&self, artifact: &[u8]) -> Result<String, String> {
        if artifact.is_empty() || artifact.len() > 65_536 {
            return Err("invalid artifact size".into());
        }
        let id = digest(artifact);
        self.conn.execute("INSERT OR IGNORE INTO candidates(id,artifact,runtime,abi_version) VALUES (?1,?2,'wasmi-experimental',2)",params![id,artifact]).map_err(|e|e.to_string())?;
        let (bytes, runtime, abi): (Vec<u8>, String, i64) = self
            .conn
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if bytes != artifact || runtime != "wasmi-experimental" || abi != 2 {
            return Err("candidate identity or ABI mismatch".into());
        }
        Ok(id)
    }
    /// Caller asserts these are fixed synthetic test bytes; this is not attestation or a sandbox.
    pub fn register_synthetic_window(&self, artifact: &[u8]) -> Result<String, String> {
        if artifact.is_empty() || artifact.len() > 65_536 {
            return Err("invalid artifact size".into());
        }
        let id = digest(artifact);
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let inserted=tx.execute("INSERT OR IGNORE INTO candidates(id,artifact,runtime,abi_version) VALUES (?1,?2,'wasmi-experimental',2)",params![id,artifact]).map_err(|e|e.to_string())?;
        if inserted == 0 {
            let synthetic:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM synthetic_window_candidates WHERE candidate_id=?1)",[&id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !synthetic {
                return Err("existing candidate is not classified synthetic".into());
            }
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
                .to_string();
            tx.execute("INSERT INTO synthetic_window_candidates(candidate_id,created_unix_ns) VALUES (?1,?2)",params![id,now]).map_err(|e|e.to_string())?;
            crate::ledger::append_synthetic_window_candidate(&tx, tx.last_insert_rowid())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.artifact_for_abi(&id, 2)?;
        Ok(id)
    }
    /// Non-executing intake. Existing records are never silently relabeled synthetic.
    pub fn register_event(&self, artifact: &[u8]) -> Result<String, String> {
        if artifact.is_empty() || artifact.len() > 65_536 {
            return Err("invalid artifact size".into());
        }
        let id = digest(artifact);
        self.conn.execute("INSERT OR IGNORE INTO candidates(id,artifact,runtime,abi_version) VALUES (?1,?2,'wasmi-experimental',3)",params![id,artifact]).map_err(|e|e.to_string())?;
        let (bytes, runtime, abi): (Vec<u8>, String, i64) = self
            .conn
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if bytes != artifact || runtime != "wasmi-experimental" || abi != 3 {
            return Err("candidate identity or ABI mismatch".into());
        }
        Ok(id)
    }
    /// Caller asserts these are fixed synthetic test bytes; this is not a sandbox or attestation.
    pub fn register_synthetic_event(&self, artifact: &[u8]) -> Result<String, String> {
        if artifact.is_empty() || artifact.len() > 65_536 {
            return Err("invalid artifact size".into());
        }
        let id = digest(artifact);
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let inserted=tx.execute("INSERT OR IGNORE INTO candidates(id,artifact,runtime,abi_version) VALUES (?1,?2,'wasmi-experimental',3)",params![id,artifact]).map_err(|e|e.to_string())?;
        if inserted == 0 {
            let synthetic: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM synthetic_event_candidates WHERE candidate_id=?1)",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !synthetic {
                return Err("existing candidate is not classified synthetic".into());
            }
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
                .to_string();
            tx.execute("INSERT INTO synthetic_event_candidates(candidate_id,created_unix_ns) VALUES (?1,?2)",params![id,now]).map_err(|e|e.to_string())?;
            crate::ledger::append_synthetic_event_candidate(&tx, tx.last_insert_rowid())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.artifact_for_abi(&id, 3)?;
        Ok(id)
    }
    pub(crate) fn save_event_replay(
        &mut self,
        candidate: &str,
        version: &str,
        digest: &str,
        started: &str,
        results: &[crate::event_replay::CaseResult],
        semantics: &str,
        worker: &crate::worker_identity::WorkerIdentity,
    ) -> Result<i64, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO event_replay_runs(candidate_id,pack_version,pack_digest,passed,started_unix_ns,environment,state_semantics) VALUES (?1,?2,?3,?4,?5,'synthetic',?6)", params![candidate,version,digest,results.iter().all(|c| c.passed),started,semantics]).map_err(|e| e.to_string())?;
        let id = tx.last_insert_rowid();
        for (ordinal, result) in results.iter().enumerate() {
            let w = result.case.window;
            let e = result.case.event;
            let window=serde_json::json!({"count":w.count,"mean":w.mean,"mean_bits":format!("{:016x}",w.mean.to_bits()),"p99_us":w.p99_us,"last_error":w.last_error}).to_string();
            let event=serde_json::json!({"timestamp_ns":e.timestamp_ns,"entity_id":e.entity_id,"metric":e.metric,"metric_bits":format!("{:016x}",e.metric.to_bits()),"flags":e.flags}).to_string();
            let expected = crate::event_replay::verdict_json(result.case.expected);
            let actual = result.actual.map(crate::event_replay::verdict_json);
            tx.execute("INSERT INTO event_replay_cases(run_id,ordinal,case_id,window_json,event_json,expected_json,actual_json,error,passed) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![id,ordinal as i64,result.case.id,window,event,expected,actual,result.error,result.passed]).map_err(|e|e.to_string())?;
        }
        crate::ledger::append_event_replay(&tx, id, started)?;
        tx.execute(
            "UPDATE event_replay_runs SET worker_path=?1,worker_sha256=?2 WHERE id=?3",
            params![worker.path, worker.sha256, id],
        )
        .map_err(|e| e.to_string())?;
        crate::ledger::append_event_worker_provenance(&tx, "event_replay_worker", id, started)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Checks one snapshot for internal consistency, not authenticity or review eligibility.
    pub fn verify_event_replay(
        &mut self,
        id: i64,
    ) -> Result<crate::event_replay::VerifiedReplay, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let result = Self::verify_event_replay_conn(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
    /// Record report digests as a caller assertion, never as promotion evidence.
    pub fn record_container_comparison_assertion(
        &mut self,
        replay: i64,
        observation: i64,
        actor: &str,
        replay_report_sha256: &str,
        shadow_report_sha256: &str,
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        self.record_container_comparison_inner(
            replay,
            observation,
            actor,
            replay_report_sha256,
            shadow_report_sha256,
            checkpoint,
            None,
        )
    }

    /// Retain caller-supplied report and receipt bytes; no execution is attested.
    pub fn record_container_comparison_capture(
        &mut self,
        replay: i64,
        observation: i64,
        actor: &str,
        bundle_bytes: &[u8],
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        #[derive(serde::Deserialize, serde::Serialize)]
        #[serde(deny_unknown_fields)]
        struct Bundle {
            schema: String,
            replay_report: String,
            shadow_report: String,
            receipts: Vec<String>,
        }
        if bundle_bytes.is_empty() || bundle_bytes.len() > 4_194_304 {
            return Err("invalid comparison bundle size".into());
        }
        let bundle: Bundle = serde_json::from_slice(bundle_bytes)
            .map_err(|e| format!("invalid comparison bundle: {e}"))?;
        if bundle.schema != "sese-synthetic-container-comparison-bundle-v1"
            || bundle.receipts.is_empty()
            || bundle.receipts.len() > 1002
            || bundle
                .receipts
                .iter()
                .any(|r| r.is_empty() || r.len() > 4096)
            || bundle.replay_report.is_empty()
            || bundle.shadow_report.is_empty()
        {
            return Err("invalid comparison bundle".into());
        }
        for value in std::iter::once(&bundle.replay_report)
            .chain(std::iter::once(&bundle.shadow_report))
            .chain(bundle.receipts.iter())
        {
            let parsed: serde_json::Value =
                serde_json::from_str(value).map_err(|_| "invalid captured JSON")?;
            if !parsed.is_object() {
                return Err("invalid captured JSON".into());
            }
        }
        let replay_hash = digest(bundle.replay_report.as_bytes());
        let shadow_hash = digest(bundle.shadow_report.as_bytes());
        let snapshot = serde_json::to_value(bundle).map_err(|e| e.to_string())?;
        self.record_container_comparison_inner(
            replay,
            observation,
            actor,
            &replay_hash,
            &shadow_hash,
            checkpoint,
            Some(snapshot),
        )
    }

    fn record_container_comparison_inner(
        &mut self,
        replay: i64,
        observation: i64,
        actor: &str,
        replay_report_sha256: &str,
        shadow_report_sha256: &str,
        checkpoint: (i64, &str),
        bundle: Option<serde_json::Value>,
    ) -> Result<i64, String> {
        let valid_hash = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if actor.trim().is_empty()
            || actor.len() > 256
            || !valid_hash(replay_report_sha256)
            || !valid_hash(shadow_report_sha256)
        {
            return Err("invalid container comparison assertion".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let head: (i64, String) = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if (head.0, head.1.as_str()) != checkpoint {
            return Err("ledger changed during comparison".into());
        }
        let verified = Self::verify_event_replay_in_verified_snapshot(&tx, replay)?;
        if !crate::ledger::event_observation_binding_matches(&tx, observation)? {
            return Err("invalid observation binding".into());
        }
        let shadow: String = tx
            .query_row(
                "SELECT shadow_id FROM event_observations WHERE id=?1",
                [observation],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if shadow != verified.candidate_id {
            return Err("comparison candidate mismatch".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let mut payload = serde_json::json!({"schema":"sese-container-comparison-assertion-v1", "replay_id":replay,
            "observation_id":observation, "candidate_id":verified.candidate_id, "actor":actor,
            "replay_report_sha256":replay_report_sha256, "shadow_report_sha256":shadow_report_sha256,
            "preview_checkpoint":{"seq":head.0,"hash":head.1}, "evidence_origin":"caller_asserted",
            "synthetic":true, "promotion_evidence":false});
        if let Some(bundle) = bundle {
            payload["bundle"] = bundle;
        }
        let seq = crate::ledger::append_container_comparison_assertion(
            &tx,
            replay,
            &payload.to_string(),
            &now,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(seq)
    }

    pub fn inspect_container_comparison_assertion(
        &mut self,
        seq: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let payload: String = tx.query_row("SELECT payload FROM ledger_events WHERE seq=?1 AND kind='container_comparison_assertion'", [seq], |r|r.get(0)).map_err(|e|e.to_string())?;
        let value =
            serde_json::from_str(&payload).map_err(|e| format!("invalid assertion: {e}"))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }

    #[cfg(unix)]
    pub(crate) fn save_container_replay_experiment(
        &mut self,
        payload: &serde_json::Value,
        candidate: &str,
        checkpoint: (i64, &str),
        time: &str,
    ) -> Result<i64, String> {
        let serialized = payload.to_string();
        if serialized.len() > 4_194_304 {
            return Err("container replay evidence too large".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let head: (i64, String) = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if (head.0, head.1.as_str()) != checkpoint {
            return Err("ledger changed during container replay".into());
        }
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM synthetic_event_candidates s JOIN candidates c ON c.id=s.candidate_id WHERE c.id=?1 AND c.abi_version=3)", [candidate], |r|r.get(0)).map_err(|e|e.to_string())?;
        if !valid {
            return Err("synthetic candidate unavailable".into());
        }
        if payload["candidate_id"].as_str() != Some(candidate) {
            return Err("container replay candidate mismatch".into());
        }
        Self::validate_container_replay_experiment(&tx, payload)?;
        let seq = crate::ledger::append_container_replay_experiment(&tx, &serialized, time)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(seq)
    }

    /// Inspect a separate synthetic container experiment, never promotion evidence.
    #[cfg(unix)]
    pub fn inspect_container_replay_experiment(
        &mut self,
        seq: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let payload: String = tx.query_row("SELECT payload FROM ledger_events WHERE seq=?1 AND kind='container_replay_experiment'", [seq], |r|r.get(0)).map_err(|e|e.to_string())?;
        let value =
            serde_json::from_str(&payload).map_err(|e| format!("invalid container replay: {e}"))?;
        Self::validate_container_replay_experiment(&tx, &value)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }

    #[cfg(unix)]
    fn validate_container_replay_experiment(
        conn: &Connection,
        value: &serde_json::Value,
    ) -> Result<(), String> {
        if value["schema"] != "sese-container-replay-experiment-v1"
            || value["synthetic"] != true
            || value["execution_authenticated"] != false
            || value["promotion_evidence"] != false
        {
            return Err("invalid container replay provenance".into());
        }
        let text = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("invalid container replay {key}"))
        };
        let candidate = text("candidate_id")?;
        let (artifact, runtime, abi): (Vec<u8>, String, i64) = conn
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [candidate],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if digest(&artifact) != candidate
            || runtime != "wasmi-experimental"
            || abi != 3
            || !crate::builtin_event::NAMES
                .iter()
                .copied()
                .any(|name| crate::builtin_event::artifact(name).is_ok_and(|v| v == artifact))
        {
            return Err("invalid container replay candidate".into());
        }
        let version = text("fixture_version")?;
        if version.trim().is_empty() || version.len() > 256 {
            return Err("invalid fixture version".into());
        }
        let expected_digest = text("fixture_digest")?;
        let semantics = text("state_semantics")?;
        let passed = value["passed"]
            .as_bool()
            .ok_or("invalid container replay result")?;
        let cases = value["cases"]
            .as_array()
            .ok_or("invalid container replay cases")?;
        let mut rows = Vec::with_capacity(cases.len());
        for (ordinal, case) in cases.iter().enumerate() {
            let id = case["id"]
                .as_str()
                .ok_or("invalid container replay case")?
                .to_owned();
            let actual = if case["actual"].is_null() {
                None
            } else {
                Some(case["actual"].to_string())
            };
            let error = if case["error"].is_null() {
                None
            } else {
                Some(
                    case["error"]
                        .as_str()
                        .ok_or("invalid container replay error")?
                        .to_owned(),
                )
            };
            let result = case["passed"]
                .as_bool()
                .ok_or("invalid container case result")?;
            if (actual.is_some()
                && !(case["receipt"].is_string() && case["receipt_path"].is_string()))
                || (actual.is_none()
                    && !(case["receipt"].is_null() && case["receipt_path"].is_null()))
            {
                return Err("invalid container receipt presence".into());
            }
            rows.push((
                ordinal as i64,
                id,
                case["window"].to_string(),
                case["event"].to_string(),
                case["expected"].to_string(),
                actual,
                error,
                i64::from(result),
            ));
        }
        crate::event_replay::verify_cases(rows, expected_digest, i64::from(passed), semantics)?;
        let image = text("image")?;
        let host = text("docker_host")?;
        let mut names = std::collections::HashSet::new();
        let mut containers = std::collections::HashSet::new();
        let mut daemon: Option<String> = None;
        for case in cases {
            if case["actual"].is_null() {
                continue;
            }
            let w = &case["window"];
            let e = &case["event"];
            let window = crate::event_abi::Window {
                count: crate::event_replay::u64_field(w, "count")?,
                mean: crate::event_replay::float_field(w, "mean", "mean_bits")?,
                p99_us: crate::event_replay::u64_field(w, "p99_us")?,
                last_error: crate::event_replay::u32_field(w, "last_error")?,
            };
            let event = crate::event_abi::Event {
                timestamp_ns: crate::event_replay::u64_field(e, "timestamp_ns")?,
                entity_id: crate::event_replay::u64_field(e, "entity_id")?,
                metric: crate::event_replay::float_field(e, "metric", "metric_bits")?,
                flags: crate::event_replay::u32_field(e, "flags")?,
            };
            let actual = crate::event_replay::verdict(&case["actual"].to_string())?;
            let (name, container, observed_daemon) =
                crate::event_executor::verify_captured_receipt(
                    case["receipt"].as_str().ok_or("missing captured receipt")?,
                    case["receipt_path"]
                        .as_str()
                        .ok_or("missing receipt path")?,
                    &artifact,
                    &window,
                    &event,
                    actual,
                    image,
                    host,
                )?;
            if !names.insert(name)
                || !containers.insert(container)
                || daemon.as_ref().is_some_and(|d| d != &observed_daemon)
            {
                return Err("reused container identity or changed daemon".into());
            }
            daemon = Some(observed_daemon);
        }
        Ok(())
    }

    /// Inspect a typed replay, including failed cases, from one verified snapshot.
    pub fn inspect_event_replay(&mut self, id: i64) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let value = Self::inspect_event_replay_conn(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    fn inspect_event_replay_conn(conn: &Connection, id: i64) -> Result<serde_json::Value, String> {
        let verified = Self::verify_event_replay_conn(conn, id)?;
        let (started, worker_path, worker_sha256): (String, Option<String>, Option<String>) = conn.query_row(
            "SELECT started_unix_ns,worker_path,worker_sha256 FROM event_replay_runs WHERE id=?1", [id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).map_err(|e|e.to_string())?;
        let cases = {
            let mut stmt = conn.prepare("SELECT case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=?1 ORDER BY ordinal").map_err(|e|e.to_string())?;
            let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
            let mut cases = Vec::new();
            let parse =
                |s: &str| serde_json::from_str::<serde_json::Value>(s).map_err(|e| e.to_string());
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let case_id: String = row.get(0).map_err(|e| e.to_string())?;
                let window: String = row.get(1).map_err(|e| e.to_string())?;
                let event: String = row.get(2).map_err(|e| e.to_string())?;
                let expected: String = row.get(3).map_err(|e| e.to_string())?;
                let actual: Option<String> = row.get(4).map_err(|e| e.to_string())?;
                let error: Option<String> = row.get(5).map_err(|e| e.to_string())?;
                let passed: bool = row.get(6).map_err(|e| e.to_string())?;
                cases.push(serde_json::json!({"id":case_id,"window":parse(&window)?,"event":parse(&event)?,"expected":parse(&expected)?,"actual":actual.as_deref().map(&parse).transpose()?,"error":error,"passed":passed}));
            }
            cases
        };
        let value = serde_json::json!({"schema":"sese-synthetic-event-replay-inspection-v1","id":id,"candidate_id":verified.candidate_id,"fixture_version":verified.pack_version,"fixture_digest":verified.pack_digest,"state_semantics":verified.state_semantics,"passed":verified.passed,"started_unix_ns":started,"worker":{"path":worker_path,"sha256":worker_sha256},"cases":cases,"synthetic":true});
        Ok(value)
    }
    fn verify_event_replay_conn(
        conn: &Connection,
        id: i64,
    ) -> Result<crate::event_replay::VerifiedReplay, String> {
        if !crate::ledger::verify_storage(conn)? {
            return Err("ledger integrity failure".into());
        }
        Self::verify_event_replay_in_verified_snapshot(conn, id)
    }
    // Call only after verifying this transaction's stable snapshot.
    fn verify_event_replay_in_verified_snapshot(
        conn: &Connection,
        id: i64,
    ) -> Result<crate::event_replay::VerifiedReplay, String> {
        let (candidate,version,digest,passed,environment,artifact,runtime,abi,semantics):(String,String,String,i64,String,Vec<u8>,String,i64,String)=conn.query_row("SELECT r.candidate_id,r.pack_version,r.pack_digest,r.passed,r.environment,c.artifact,c.runtime,c.abi_version,r.state_semantics FROM event_replay_runs r JOIN candidates c ON c.id=r.candidate_id WHERE r.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        if version.trim().is_empty()
            || environment != "synthetic"
            || runtime != "wasmi-experimental"
            || abi != 3
            || crate::storage::digest(&artifact) != candidate
        {
            return Err("invalid replay provenance".into());
        }
        let rows: Vec<crate::event_replay::StoredCase> = {
            let mut stmt=conn.prepare("SELECT ordinal,case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=?1 ORDER BY ordinal LIMIT 1001").map_err(|e|e.to_string())?;
            let rows = stmt
                .query_map([id], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
        };
        if !crate::ledger::event_replay_binding_matches(conn, id)? {
            return Err("event replay ledger binding failure".into());
        }
        let cases = crate::event_replay::verify_cases(rows, &digest, passed, &semantics)?;
        Ok(crate::event_replay::VerifiedReplay {
            candidate_id: candidate,
            pack_version: version,
            pack_digest: digest,
            state_semantics: semantics,
            passed: passed == 1,
            cases,
        })
    }
    /// Records a synthetic decision; it does not activate or promote a candidate.
    pub fn record_event_review(
        &mut self,
        candidate: &str,
        replay: i64,
        observation: i64,
        actor: &str,
        action: &str,
        human: &str,
        bridge: &str,
        proposal: &str,
    ) -> Result<i64, String> {
        self.record_event_review_inner(
            candidate,
            replay,
            observation,
            actor,
            action,
            human,
            bridge,
            proposal,
            None,
            None,
        )
    }
    /// Record only if the verified ledger still has the caller's exact review-time head.
    pub fn record_event_review_at_checkpoint(
        &mut self,
        candidate: &str,
        replay: i64,
        observation: i64,
        actor: &str,
        action: &str,
        human: &str,
        bridge: &str,
        proposal: &str,
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        self.record_event_review_inner(
            candidate,
            replay,
            observation,
            actor,
            action,
            human,
            bridge,
            proposal,
            Some(checkpoint),
            None,
        )
    }
    /// Record a decision against a domain observation included in the guarded preview.
    pub fn record_event_review_from_preview(
        &mut self,
        domain: &str,
        candidate: &str,
        replay: i64,
        observation: i64,
        actor: &str,
        action: &str,
        human: &str,
        bridge: &str,
        proposal: &str,
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        if domain.trim().is_empty() || domain.len() > 16_384 {
            return Err("invalid preview domain".into());
        }
        self.record_event_review_inner(
            candidate,
            replay,
            observation,
            actor,
            action,
            human,
            bridge,
            proposal,
            Some(checkpoint),
            Some(domain),
        )
    }
    fn record_event_review_inner(
        &mut self,
        candidate: &str,
        replay: i64,
        observation: i64,
        actor: &str,
        action: &str,
        human: &str,
        bridge: &str,
        proposal: &str,
        checkpoint: Option<(i64, &str)>,
        preview_domain: Option<&str>,
    ) -> Result<i64, String> {
        if [candidate, actor, human, bridge, proposal]
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 16_384)
            || !["approve", "reject"].contains(&action)
        {
            return Err("invalid event review".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if let Some((seq, hash)) = checkpoint {
            let head: Option<(i64, String)> = tx
                .query_row(
                    "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if head.as_ref().map(|(n, h)| (*n, h.as_str())) != Some((seq, hash)) {
                return Err("ledger changed during review; inspect evidence again".into());
            }
        }
        if let Some(domain) = preview_domain {
            let count: i64 = tx.query_row("SELECT count(*) FROM (SELECT id FROM event_observations WHERE domain=?1 LIMIT 101)", [domain], |r| r.get(0)).map_err(|e|e.to_string())?;
            if count > 100 {
                return Err("review preview exceeds 100 observations".into());
            }
            let included: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM (SELECT id FROM event_observations WHERE domain=?1 ORDER BY id LIMIT 100) WHERE id=?2)", params![domain, observation], |r| r.get(0)).map_err(|e|e.to_string())?;
            if !included {
                return Err("observation is not in the specified preview domain".into());
            }
        }
        let verified = Self::verify_event_replay_conn(&tx, replay)?;
        if verified.candidate_id != candidate
            || !crate::ledger::event_observation_binding_matches(&tx, observation)?
        {
            return Err("invalid event review evidence".into());
        }
        let (shadow,c,ce,s,se):(String,Option<String>,Option<String>,Option<String>,Option<String>)=tx.query_row("SELECT shadow_id,champion_json,champion_error,shadow_json,shadow_error FROM event_observations WHERE id=?1",[observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
        let prior: String = tx
            .query_row(
                "SELECT prior_json FROM event_observations WHERE id=?1",
                [observation],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let parsed: serde_json::Value = serde_json::from_str(&prior).map_err(|e| e.to_string())?;
        let observation_semantics = match parsed.get("state_semantics") {
            None => "legacy-v1",
            Some(v) => v.as_str().ok_or("invalid observation state semantics")?,
        };
        if observation_semantics != verified.state_semantics {
            return Err("replay and observation semantics differ".into());
        }
        if shadow != candidate {
            return Err("evidence does not match candidate".into());
        }
        if action == "approve"
            && (!verified.passed || c.is_none() || s.is_none() || ce.is_some() || se.is_some())
        {
            return Err("evidence is incomplete or failed".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_reviews(candidate_id,replay_id,observation_id,actor,action,human,bridge,proposal,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![candidate,replay,observation,actor,action,human,bridge,proposal,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_review(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Returns a review and its referenced synthetic evidence from one verified snapshot.
    pub fn inspect_event_review(&mut self, id: i64) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (candidate,replay,observation,actor,action,human,bridge,proposal,created):(String,i64,i64,String,String,String,String,String,String)=tx.query_row("SELECT candidate_id,replay_id,observation_id,actor,action,human,bridge,proposal,created_unix_ns FROM event_reviews WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        let verified = Self::verify_event_replay_conn(&tx, replay)?;
        let (prior,event,champion,champion_error,shadow,shadow_error,domain):(String,String,Option<String>,Option<String>,Option<String>,Option<String>,Option<String>)=tx.query_row("SELECT prior_json,event_json,champion_json,champion_error,shadow_json,shadow_error,domain FROM event_observations WHERE id=?1",[observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        let (replay_worker_path, replay_worker_hash): (Option<String>, Option<String>) = tx
            .query_row(
                "SELECT worker_path,worker_sha256 FROM event_replay_runs WHERE id=?1",
                [replay],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let (observation_worker_path, observation_worker_hash): (Option<String>, Option<String>) =
            tx.query_row(
                "SELECT worker_path,worker_sha256 FROM event_observations WHERE id=?1",
                [observation],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let parse =
            |s: &str| serde_json::from_str::<serde_json::Value>(s).map_err(|e| e.to_string());
        let cases = {
            let mut stmt=tx.prepare("SELECT case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=?1 ORDER BY ordinal").map_err(|e|e.to_string())?;
            let mut rows = stmt.query([replay]).map_err(|e| e.to_string())?;
            let mut cases = Vec::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: String = row.get(0).map_err(|e| e.to_string())?;
                let window: String = row.get(1).map_err(|e| e.to_string())?;
                let event: String = row.get(2).map_err(|e| e.to_string())?;
                let expected: String = row.get(3).map_err(|e| e.to_string())?;
                let actual: Option<String> = row.get(4).map_err(|e| e.to_string())?;
                let error: Option<String> = row.get(5).map_err(|e| e.to_string())?;
                let passed: bool = row.get(6).map_err(|e| e.to_string())?;
                cases.push(serde_json::json!({"id":id,"window":parse(&window)?,"event":parse(&event)?,"expected":parse(&expected)?,"actual":actual.as_deref().map(&parse).transpose()?,"error":error,"passed":passed}));
            }
            cases
        };
        let outcome:Option<(String,String,String,String)>=tx.query_row("SELECT status,actor,source,created_unix_ns FROM event_review_outcomes WHERE review_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e|e.to_string())?;
        let status = outcome.as_ref().map(|v| v.0.as_str()).unwrap_or("pending");
        let outcome_detail=outcome.as_ref().map(|(status,actor,source,time)|serde_json::json!({"status":status,"actor":actor,"source":source,"created_unix_ns":time}));
        let value = serde_json::json!({"id":id,"candidate_id":candidate,"action":action,"actor":actor,"created_unix_ns":created,"card":{"human":human,"bridge":bridge,"proposal":proposal},"replay":{"id":replay,"worker":{"path":replay_worker_path,"sha256":replay_worker_hash},"pack_version":verified.pack_version,"pack_digest":verified.pack_digest,"state_semantics":verified.state_semantics,"passed":verified.passed,"case_count":verified.cases,"cases":cases},"observation":{"id":observation,"worker":{"path":observation_worker_path,"sha256":observation_worker_hash},"domain":domain,"prior":parse(&prior)?,"event":parse(&event)?,"champion":champion.as_deref().map(&parse).transpose()?,"champion_error":champion_error,"shadow":shadow.as_deref().map(&parse).transpose()?,"shadow_error":shadow_error},"outcome":status,"outcome_detail":outcome_detail,"synthetic":true});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    /// Page through synthetic decisions, outcomes and champion transitions in ledger order.
    pub fn inspect_event_history(
        &mut self,
        domain: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<serde_json::Value, String> {
        if domain.trim().is_empty()
            || domain.len() > 16_384
            || after_seq < 0
            || !(1..=100).contains(&limit)
        {
            return Err("invalid history query".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (entries, next_after_seq) = {
            let mut stmt = tx.prepare("SELECT e.seq,e.kind,e.reference,e.created_ns FROM ledger_events e LEFT JOIN event_reviews r ON e.kind IN ('event_review','event_outcome') AND r.id=e.reference LEFT JOIN event_observations o ON o.id=r.observation_id LEFT JOIN event_champion_transitions t ON e.kind='event_champion_transition' AND t.id=e.reference WHERE e.seq>?1 AND ((e.kind IN ('event_review','event_outcome') AND o.domain=?2) OR (e.kind='event_champion_transition' AND t.domain=?2)) ORDER BY e.seq LIMIT ?3").map_err(|e|e.to_string())?;
            let mut rows = stmt
                .query(params![after_seq, domain, (limit + 1) as i64])
                .map_err(|e| e.to_string())?;
            let mut entries = Vec::new();
            let mut more = false;
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                if entries.len() == limit {
                    more = true;
                    break;
                }
                let seq: i64 = row.get(0).map_err(|e| e.to_string())?;
                let kind: String = row.get(1).map_err(|e| e.to_string())?;
                let reference: i64 = row.get(2).map_err(|e| e.to_string())?;
                let timestamp: String = row.get(3).map_err(|e| e.to_string())?;
                let detail = match kind.as_str() {
                    "event_review" => {
                        let (candidate,replay,observation,actor,action):(String,i64,i64,String,String)=tx.query_row("SELECT candidate_id,replay_id,observation_id,actor,action FROM event_reviews WHERE id=?1",[reference],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
                        serde_json::json!({"review_id":reference,"candidate_id":candidate,"replay_id":replay,"observation_id":observation,"actor":actor,"action":action})
                    }
                    "event_outcome" => {
                        let (status,actor,source):(String,String,String)=tx.query_row("SELECT status,actor,source FROM event_review_outcomes WHERE review_id=?1",[reference],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
                        serde_json::json!({"review_id":reference,"status":status,"actor":actor,"source":source,"evidence":"caller_asserted"})
                    }
                    "event_champion_transition" => {
                        let (action,previous,next,review,actor):(String,String,String,i64,String)=tx.query_row("SELECT kind,previous_id,next_id,reference,actor FROM event_champion_transitions WHERE id=?1",[reference],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
                        serde_json::json!({"transition_id":reference,"action":action,"previous_id":previous,"next_id":next,"reference":review,"actor":actor})
                    }
                    _ => return Err("unexpected history event".into()),
                };
                entries.push(serde_json::json!({"seq":seq,"kind":kind,"created_unix_ns":timestamp,"detail":detail}));
            }
            let next = if more {
                entries.last().and_then(|entry| entry["seq"].as_i64())
            } else {
                None
            };
            (entries, next)
        };
        let head: Option<(i64, String)> = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-event-history-v1","domain":domain,"after_seq":after_seq,"entries":entries,"next_after_seq":next_after_seq,"checkpoint":head.map(|(seq,hash)|serde_json::json!({"seq":seq,"hash":hash})),"synthetic":true}),
        )
    }

    /// Inspect one bounded page of domain-scoped synthetic observations in a verified snapshot.
    pub fn inspect_event_observations(
        &mut self,
        domain: &str,
        after_id: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let value = Self::inspect_event_observations_conn(&tx, domain, after_id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    fn inspect_event_observations_conn(
        conn: &Connection,
        domain: &str,
        after_id: i64,
    ) -> Result<serde_json::Value, String> {
        if domain.trim().is_empty() || domain.len() > 16_384 || after_id < 0 {
            return Err("invalid observation query".into());
        }
        if !crate::ledger::verify_storage(conn)? {
            return Err("ledger integrity failure".into());
        }
        let (observations, next_after_id) = {
            let mut stmt = conn.prepare("SELECT id,champion_id,shadow_id,prior_json,event_json,champion_json,champion_error,shadow_json,shadow_error,champion_us,shadow_us,observed_unix_ns FROM event_observations WHERE domain=?1 AND id>?2 ORDER BY id LIMIT 101").map_err(|e|e.to_string())?;
            let mut rows = stmt
                .query(params![domain, after_id])
                .map_err(|e| e.to_string())?;
            let mut values = Vec::new();
            let mut next = None;
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: i64 = row.get(0).map_err(|e| e.to_string())?;
                if values.len() == 100 {
                    next = values
                        .last()
                        .and_then(|v: &serde_json::Value| v["id"].as_i64());
                    break;
                }
                if !crate::ledger::event_observation_binding_matches(conn, id)? {
                    return Err("observation evidence binding failure".into());
                }
                let champion_id: String = row.get(1).map_err(|e| e.to_string())?;
                let shadow_id: String = row.get(2).map_err(|e| e.to_string())?;
                let prior: String = row.get(3).map_err(|e| e.to_string())?;
                let event: String = row.get(4).map_err(|e| e.to_string())?;
                let champion: Option<String> = row.get(5).map_err(|e| e.to_string())?;
                let champion_error: Option<String> = row.get(6).map_err(|e| e.to_string())?;
                let shadow: Option<String> = row.get(7).map_err(|e| e.to_string())?;
                let shadow_error: Option<String> = row.get(8).map_err(|e| e.to_string())?;
                let champion_us: i64 = row.get(9).map_err(|e| e.to_string())?;
                let shadow_us: i64 = row.get(10).map_err(|e| e.to_string())?;
                let time: String = row.get(11).map_err(|e| e.to_string())?;
                let parse = |s: &str| {
                    serde_json::from_str::<serde_json::Value>(s).map_err(|e| e.to_string())
                };
                values.push(serde_json::json!({"id":id,"champion_id":champion_id,"shadow_id":shadow_id,"prior":parse(&prior)?,"event":parse(&event)?,"champion":champion.as_deref().map(&parse).transpose()?,"champion_error":champion_error,"shadow":shadow.as_deref().map(&parse).transpose()?,"shadow_error":shadow_error,"champion_us":champion_us,"shadow_us":shadow_us,"observed_unix_ns":time}));
            }
            (values, next)
        };
        Ok(
            serde_json::json!({"schema":"sese-synthetic-observation-page-v1","domain":domain,"after_id":after_id,"observations":observations,"next_after_id":next_after_id,"synthetic":true}),
        )
    }
    /// Assemble one bounded review preview and its ledger head in a single verified snapshot.
    pub fn event_review_preview(
        &mut self,
        replay: i64,
        domain: &str,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let value = Self::event_review_preview_conn(&tx, replay, domain)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    fn event_review_preview_conn(
        conn: &Connection,
        replay: i64,
        domain: &str,
    ) -> Result<serde_json::Value, String> {
        let replay = Self::inspect_event_replay_conn(conn, replay)?;
        let page = Self::inspect_event_observations_conn(conn, domain, 0)?;
        if !page["next_after_id"].is_null() {
            return Err("review preview exceeds 100 observations".into());
        }
        let candidate = replay["candidate_id"]
            .as_str()
            .ok_or("invalid replay preview")?;
        let selected:Option<(i64,i64,String,String,String)>=conn.query_row("SELECT id,replay_id,case_id,actor,created_unix_ns FROM event_followup_pins WHERE domain=?1 AND candidate_id=?2",params![domain,candidate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(|e|e.to_string())?;
        let followup = if let Some((id, run, case_id, actor, time)) = selected {
            Some(
                serde_json::json!({"selection_id":id,"case_id":case_id,"actor":actor,"created_unix_ns":time,"replay":Self::inspect_event_replay_conn(conn,run)?}),
            )
        } else {
            None
        };
        let (seq, hash): (i64, String) = conn
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-review-preview-v1","replay":replay,"followup":followup,"observations":page["observations"],"checkpoint":{"seq":seq,"hash":hash},"synthetic":true}),
        )
    }

    /// Compare stored experimental container evidence with local review evidence in one read snapshot.
    #[cfg(unix)]
    pub fn event_container_review_preview(
        &mut self,
        domain: &str,
        replay: i64,
        observation: i64,
        container_replay_seq: i64,
        container_shadow_seq: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let preview = Self::event_review_preview_conn(&tx, replay, domain)?;
        Self::ensure_observation_in_current_tenure(&tx, domain, observation)?;
        let (pinned_version, pinned_digest, pinned_semantics): (String,String,String) = tx.query_row("SELECT p.pack_version,p.pack_digest,r.state_semantics FROM event_fixture_pins p JOIN event_replay_runs r ON r.id=p.replay_id WHERE p.domain=?1",[domain],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        if preview["replay"]["fixture_version"].as_str() != Some(pinned_version.as_str())
            || preview["replay"]["fixture_digest"].as_str() != Some(pinned_digest.as_str())
            || preview["replay"]["state_semantics"].as_str() != Some(pinned_semantics.as_str())
        {
            return Err("local replay does not match domain fixture pin".into());
        }
        let replay_payload: String = tx.query_row("SELECT payload FROM ledger_events WHERE seq=?1 AND kind='container_replay_experiment'",[container_replay_seq],|r|r.get(0)).map_err(|e|e.to_string())?;
        let container_replay: serde_json::Value =
            serde_json::from_str(&replay_payload).map_err(|e| e.to_string())?;
        Self::validate_container_replay_experiment(&tx, &container_replay)?;
        let (shadow_kind,shadow_payload): (String,String) = tx.query_row("SELECT kind,payload FROM ledger_events WHERE seq=?1 AND kind IN ('container_shadow_experiment','container_observation_experiment')",[container_shadow_seq],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        let shadow: serde_json::Value =
            serde_json::from_str(&shadow_payload).map_err(|e| e.to_string())?;
        if shadow_kind == "container_shadow_experiment" {
            let shadow_domain = shadow["domain"].as_str().ok_or("invalid shadow domain")?;
            let stream = shadow["stream"].as_str().ok_or("invalid shadow stream")?;
            let capacity = usize::try_from(
                shadow["capacity"]
                    .as_u64()
                    .ok_or("invalid shadow capacity")?,
            )
            .map_err(|e| e.to_string())?;
            Self::container_shadow_state(&tx, shadow_domain, stream, capacity)?;
        } else {
            Self::inspect_container_observation_experiment_conn(&tx, container_shadow_seq)?;
        }
        let current: (String, i64) = tx
            .query_row(
                "SELECT champion_id,generation FROM event_champions WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        crate::container_review::compare(
            &preview,
            &container_replay,
            &shadow,
            domain,
            observation,
            &current,
        )?;
        let result = serde_json::json!({"schema":"sese-container-review-preview-v1","matched":true,"domain":domain,"application_replay_id":replay,"application_observation_id":observation,"container_replay_seq":container_replay_seq,"container_shadow_seq":container_shadow_seq,"container_comparison_seq":container_shadow_seq,"container_comparison_kind":shadow_kind,"review":preview,"container_replay":container_replay,"container_shadow":shadow,"checkpoint":preview["checkpoint"],"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
    pub fn event_outcome_status(&self, review: i64) -> Result<String, String> {
        self.conn.query_row("SELECT COALESCE(o.status,'pending') FROM event_reviews r LEFT JOIN event_review_outcomes o ON o.review_id=r.id WHERE r.id=?1",[review],|r|r.get(0)).map_err(|e|e.to_string())
    }
    /// Check a signed synthetic preference against its recorded trace and a supplied checkpoint.
    pub fn check_synthetic_preference_link(
        &mut self,
        claim: crate::attestation::SignedClaim<'_, crate::attestation::SyntheticPreferenceEvidence>,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let result = Self::check_synthetic_preference_link_conn(&tx, claim, checkpoint)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    fn check_synthetic_preference_link_conn(
        tx: &Connection,
        claim: crate::attestation::SignedClaim<'_, crate::attestation::SyntheticPreferenceEvidence>,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        if checkpoint.0 <= 0
            || !crate::attestation::verify_synthetic_preference(
                claim.authority,
                claim.evidence,
                claim.signature,
            )
        {
            return Err("invalid synthetic preference attestation".into());
        }
        if !crate::ledger::verify_storage(tx)? {
            return Err("ledger integrity failure".into());
        }
        let anchor: Option<String> = tx
            .query_row(
                "SELECT hash FROM ledger_events WHERE seq=?1",
                [checkpoint.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if anchor.as_deref() != Some(checkpoint.1) {
            return Err("checkpoint mismatch".into());
        }
        let (review,replay,candidate,fixture,review_hash,trace_hash,trace_seq,prompt,preferred,rejected,source,trace_time):(i64,i64,String,String,String,String,i64,String,Option<String>,Option<String>,Option<String>,String)=tx.query_row("SELECT r.id,r.replay_id,r.candidate_id,p.pack_digest,er.hash,et.hash,et.seq,t.prompt,t.preferred,t.rejected,t.pair_source,t.created_unix_ns FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN ledger_events er ON er.kind='event_review' AND er.reference=r.id JOIN ledger_events et ON et.kind='event_training_trace' AND et.reference=t.id WHERE t.id=?1",[claim.evidence.trace_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?))).map_err(|e|e.to_string())?;
        let trace_time: u64 = trace_time.parse().map_err(|_| "invalid trace time")?;
        let evidence = claim.evidence;
        let reference = &evidence.reference;
        let replay_evidence = Self::verify_event_replay_in_verified_snapshot(tx, replay)?;
        if trace_seq > checkpoint.0
            || reference.review_id != review
            || reference.replay_id != replay
            || reference.candidate_sha256 != candidate
            || reference.fixture_sha256 != fixture
            || reference.review_ledger_hash != review_hash
            || replay_evidence.candidate_id != candidate
            || replay_evidence.pack_digest != fixture
            || evidence.trace_ledger_hash != trace_hash
            || evidence.prompt != prompt
            || preferred.as_deref() != Some(evidence.preferred.as_str())
            || rejected.as_deref() != Some(evidence.rejected.as_str())
            || source.as_deref() != Some(evidence.source.as_str())
            || evidence.reviewed_unix_ns < trace_time
        {
            return Err("preference does not match checkpointed trace".into());
        }
        Ok(
            serde_json::json!({"schema":"sese-synthetic-preference-link-check-v1","trace_id":evidence.trace_id,"review_id":review,"signature_and_record_links_match":true,"authority_config_verified":false,"comparability_verified":false,"reviewer_independence_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Atomically retain a checked synthetic preference and its ledger receipt.
    pub fn record_synthetic_preference_receipt(
        &mut self,
        authorities: &crate::attestation::AuthoritySet,
        claim: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticPreferenceEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        use crate::attestation::{AuthorityRole, SignedClaim};
        let authority = authorities
            .get(claim.authority_id, AuthorityRole::PreferenceReviewer)
            .ok_or("unknown or revoked preference authority")?;
        let payload = claim.evidence.canonical_bytes()?;
        let config = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        Self::check_synthetic_preference_link_conn(
            &tx,
            SignedClaim {
                evidence: claim.evidence,
                authority,
                signature: claim.signature,
            },
            checkpoint,
        )?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_preference_receipts(trace_id,authority,payload,signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![claim.evidence.trace_id,claim.authority_id,payload,claim.signature.as_slice(),config,checkpoint.0,checkpoint.1,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_preference_receipt(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Inspect ledger bindings without claiming to recheck a signature or comparability.
    pub fn inspect_synthetic_preference_receipt(
        &mut self,
        id: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (trace,authority,payload,config,seq,hash,time):(i64,String,Vec<u8>,String,i64,String,String)=tx.query_row("SELECT trace_id,authority,payload,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns FROM event_preference_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-preference-receipt-v1","id":id,"trace_id":trace,"authority":authority,"payload_sha256":digest(&payload),"authority_config_sha256":config,"checkpoint":{"seq":seq,"hash":hash},"created_unix_ns":time,"signature_rechecked":false,"authority_config_verified":false,"comparability_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Recheck two synthetic receipts together in one snapshot; not an eligibility decision.
    pub fn check_synthetic_evidence_package(
        &mut self,
        attestation_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let result = Self::check_synthetic_evidence_package_conn(
            &tx,
            attestation_id,
            preference_id,
            authorities,
            checkpoint,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    /// Check a trace/outcome receipt without requiring a preference receipt.
    /// Operator declarations and caller assertions are not proof of eligibility.
    pub fn check_synthetic_common_evidence_policy(
        &mut self,
        attestation_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{verify_attestation, AuthorityRole};
        if attestation_id <= 0
            || checkpoint.0 <= 0
            || policy.authority_digest() != authorities.digest()
        {
            return Err("invalid common evidence query or authority mismatch".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let anchor: Option<String> = tx
            .query_row(
                "SELECT hash FROM ledger_events WHERE seq=?1",
                [checkpoint.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if anchor.as_deref() != Some(checkpoint.1) {
            return Err("checkpoint mismatch".into());
        }
        let (trace, source, observer, tp, op, ts, os, config, seq): (i64, String, String, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, String, i64) = tx.query_row("SELECT a.trace_id,a.trace_authority,a.outcome_authority,a.trace_payload,a.outcome_payload,a.trace_signature,a.outcome_signature,a.authority_digest,e.seq FROM event_attestation_receipts a JOIN ledger_events e ON e.kind='event_attestation_receipt' AND e.reference=a.id WHERE a.id=?1", [attestation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e| e.to_string())?;
        let supplied: String = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if config != supplied || seq > checkpoint.0 || !policy.declares(&source, &observer, None) {
            return Err("common evidence not covered by checkpoint or policy".into());
        }
        let source = authorities
            .get(&source, AuthorityRole::TraceSource)
            .ok_or("unknown or revoked trace authority")?;
        let observer = authorities
            .get(&observer, AuthorityRole::OutcomeObserver)
            .ok_or("unknown or revoked outcome authority")?;
        let trace_sig: [u8; 64] = ts.try_into().map_err(|_| "invalid trace signature")?;
        let outcome_sig: [u8; 64] = os.try_into().map_err(|_| "invalid outcome signature")?;
        if !verify_attestation(source, AuthorityRole::TraceSource, &tp, &trace_sig)
            || !verify_attestation(observer, AuthorityRole::OutcomeObserver, &op, &outcome_sig)
        {
            return Err("common evidence signature mismatch".into());
        }
        let (review, status): (i64, String) = tx.query_row("SELECT t.review_id,o.status FROM event_training_traces t JOIN event_review_outcomes o ON o.review_id=t.review_id WHERE t.id=?1", [trace], |r| Ok((r.get(0)?,r.get(1)?))).map_err(|e| e.to_string())?;
        let result = serde_json::json!({"schema":"sese-synthetic-common-evidence-policy-check-v1","attestation_receipt_id":attestation_id,"trace_id":trace,"review_id":review,"outcome":status,"outcome_evidence":"caller_asserted","signatures_rechecked":true,"operator_declaration_matches":true,"authority_config_sha256":supplied,"trust_policy_sha256":policy.digest().iter().map(|b| format!("{b:02x}")).collect::<String>(),"authority_config_verified":false,"trust_policy_custody_verified":false,"checkpoint_custody_verified":false,"observer_independence_verified":false,"outcome_truth_verified":false,"synthetic":true,"training_ready":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    /// Apply a pinned rule to a checked caller-asserted outcome; not a training export.
    pub fn preview_synthetic_reward(
        &mut self,
        attestation_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        rule: &crate::reward_rule::SyntheticRewardRule,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let mut result = self.check_synthetic_common_evidence_policy(
            attestation_id,
            authorities,
            policy,
            checkpoint,
        )?;
        let status = result["outcome"].as_str().ok_or("missing outcome")?;
        let reward = rule.reward(status)?;
        result["schema"] = serde_json::json!("sese-synthetic-reward-preview-v1");
        result["reward"] = serde_json::json!(reward);
        result["reward_rule"] = serde_json::json!({"version":rule.version(),"sha256":rule.digest().iter().map(|b| format!("{b:02x}")).collect::<String>(),"actor":rule.actor(),"source":rule.source()});
        result["checkpoint"] = serde_json::json!({"seq":checkpoint.0,"hash":checkpoint.1});
        result["reward_rule_custody_verified"] = serde_json::json!(false);
        Ok(result)
    }

    /// Match signed synthetic evidence to an operator declaration in one snapshot.
    /// This does not establish independence, custody, outcome truth or eligibility.
    pub fn check_synthetic_evidence_policy(
        &mut self,
        attestation_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        if policy.authority_digest() != authorities.digest() {
            return Err("trust policy authority mismatch".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let mut result = Self::check_synthetic_evidence_package_conn(
            &tx,
            attestation_id,
            preference_id,
            authorities,
            checkpoint,
        )?;
        let (source, observer): (String, String) = tx.query_row("SELECT trace_authority,outcome_authority FROM event_attestation_receipts WHERE id=?1", [attestation_id], |r| Ok((r.get(0)?,r.get(1)?))).map_err(|e| e.to_string())?;
        let reviewer: String = tx
            .query_row(
                "SELECT authority FROM event_preference_receipts WHERE id=?1",
                [preference_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !policy.declares(&source, &observer, Some(&reviewer)) {
            return Err("evidence not covered by trust policy".into());
        }
        result["schema"] = serde_json::json!("sese-synthetic-evidence-policy-check-v1");
        result["operator_declaration_matches"] = serde_json::json!(true);
        result["trust_policy_sha256"] = serde_json::json!(policy
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>());
        result["trust_policy_custody_verified"] = serde_json::json!(false);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    fn check_synthetic_evidence_package_conn(
        tx: &Connection,
        attestation_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{verify_attestation, AuthorityRole};
        if attestation_id <= 0 || preference_id <= 0 || checkpoint.0 <= 0 {
            return Err("invalid evidence package query".into());
        }
        if !crate::ledger::verify_storage(tx)? {
            return Err("ledger integrity failure".into());
        }
        let anchor: Option<String> = tx
            .query_row(
                "SELECT hash FROM ledger_events WHERE seq=?1",
                [checkpoint.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if anchor.as_deref() != Some(checkpoint.1) {
            return Err("checkpoint mismatch".into());
        }
        let (trace,source,observer,tp,op,ts,os,config,aseq):(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,String,i64)=tx.query_row("SELECT a.trace_id,a.trace_authority,a.outcome_authority,a.trace_payload,a.outcome_payload,a.trace_signature,a.outcome_signature,a.authority_digest,e.seq FROM event_attestation_receipts a JOIN ledger_events e ON e.kind='event_attestation_receipt' AND e.reference=a.id WHERE a.id=?1",[attestation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        let (pref_trace,reviewer,pp,ps,pconfig,pseq):(i64,String,Vec<u8>,Vec<u8>,String,i64)=tx.query_row("SELECT p.trace_id,p.authority,p.payload,p.signature,p.authority_digest,e.seq FROM event_preference_receipts p JOIN ledger_events e ON e.kind='event_preference_receipt' AND e.reference=p.id WHERE p.id=?1",[preference_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(|e|e.to_string())?;
        let supplied = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if trace != pref_trace
            || config != supplied
            || pconfig != supplied
            || checkpoint.0 < aseq.max(pseq)
        {
            return Err("evidence package mismatch or stale checkpoint".into());
        }
        let source = authorities
            .get(&source, AuthorityRole::TraceSource)
            .ok_or("unknown or revoked trace authority")?;
        let observer = authorities
            .get(&observer, AuthorityRole::OutcomeObserver)
            .ok_or("unknown or revoked outcome authority")?;
        let reviewer = authorities
            .get(&reviewer, AuthorityRole::PreferenceReviewer)
            .ok_or("unknown or revoked preference authority")?;
        let trace_sig: [u8; 64] = ts.try_into().map_err(|_| "invalid trace signature")?;
        let outcome_sig: [u8; 64] = os.try_into().map_err(|_| "invalid outcome signature")?;
        let preference_sig: [u8; 64] = ps.try_into().map_err(|_| "invalid preference signature")?;
        if !verify_attestation(source, AuthorityRole::TraceSource, &tp, &trace_sig)
            || !verify_attestation(observer, AuthorityRole::OutcomeObserver, &op, &outcome_sig)
            || !verify_attestation(
                reviewer,
                AuthorityRole::PreferenceReviewer,
                &pp,
                &preference_sig,
            )
        {
            return Err("evidence package signature mismatch".into());
        }
        let (review,status):(i64,String)=tx.query_row("SELECT t.review_id,o.status FROM event_training_traces t JOIN event_review_outcomes o ON o.review_id=t.review_id WHERE t.id=?1",[trace],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-evidence-package-check-v1","attestation_receipt_id":attestation_id,"preference_receipt_id":preference_id,"trace_id":trace,"review_id":review,"outcome":status,"outcome_evidence":"caller_asserted","all_signatures_rechecked":true,"authority_config_sha256":supplied,"authority_config_verified":false,"checkpoint_custody_verified":false,"observer_independence_verified":false,"reviewer_independence_verified":false,"comparability_verified":false,"outcome_truth_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Export one checked synthetic evidence package; it is not a training export.
    pub fn export_synthetic_evidence_package(
        &mut self,
        attestation_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        checkpoint: (i64, &str),
    ) -> Result<String, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let check = Self::check_synthetic_evidence_package_conn(
            &tx,
            attestation_id,
            preference_id,
            authorities,
            checkpoint,
        )?;
        let trace_id = check["trace_id"].as_i64().ok_or("invalid checked trace")?;
        let (review,candidate,replay,action,review_actor,fixture_version,fixture_digest,trace_actor,trace_source,prompt,response,preferred,rejected,pair_source,status,outcome_actor,outcome_source):(i64,String,i64,String,String,String,String,String,String,String,String,String,String,String,String,String,String)=tx.query_row("SELECT r.id,r.candidate_id,r.replay_id,r.action,r.actor,p.pack_version,p.pack_digest,t.actor,t.source,t.prompt,t.response,t.preferred,t.rejected,t.pair_source,o.status,o.actor,o.source FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN event_review_outcomes o ON o.review_id=r.id WHERE t.id=?1",[trace_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?,r.get(14)?,r.get(15)?,r.get(16)?))).map_err(|e|e.to_string())?;
        let (source,observer,tp,op,ts,os):(String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>)=tx.query_row("SELECT trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature FROM event_attestation_receipts WHERE id=?1",[attestation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(|e|e.to_string())?;
        let (reviewer, pp, ps): (String, Vec<u8>, Vec<u8>) = tx
            .query_row(
                "SELECT authority,payload,signature FROM event_preference_receipts WHERE id=?1",
                [preference_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let value = serde_json::json!({"schema":"sese-synthetic-signed-evidence-v1","export_kind":"inspection_only","candidate":{"sha256":candidate,"replay_id":replay,"fixture_version":fixture_version,"fixture_sha256":fixture_digest},"decision":{"review_id":review,"action":action,"actor":review_actor},"trace":{"id":trace_id,"actor":trace_actor,"source":trace_source,"prompt":prompt,"response":response,"origin":"caller_asserted_synthetic","model_origin_verified":false},"preference":{"preferred":preferred,"rejected":rejected,"source":pair_source,"comparability_verified":false},"outcome":{"status":status,"actor":outcome_actor,"source":outcome_source,"evidence":"caller_asserted","truth_verified":false},"signatures":{"trace":{"authority":source,"payload_hex":hex(&tp),"signature_hex":hex(&ts)},"outcome":{"authority":observer,"payload_hex":hex(&op),"signature_hex":hex(&os)},"preference":{"authority":reviewer,"payload_hex":hex(&pp),"signature_hex":hex(&ps)}},"attestation_receipt_id":attestation_id,"preference_receipt_id":preference_id,"checkpoint":{"seq":checkpoint.0,"hash":checkpoint.1},"checkpoint_custody_verified":false,"authority_config_sha256":check["authority_config_sha256"],"authority_config_verified":false,"observer_independence_verified":false,"reviewer_independence_verified":false,"signatures_rechecked":true,"synthetic":true,"training_ready":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(format!("{value}\n"))
    }

    /// Recheck the retained signature using the same caller-pinned authority configuration.
    pub fn recheck_synthetic_preference_receipt(
        &mut self,
        id: i64,
        authorities: &crate::attestation::AuthoritySet,
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{verify_attestation, AuthorityRole};
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (trace,authority,payload,signature,config):(i64,String,Vec<u8>,Vec<u8>,String)=tx.query_row("SELECT trace_id,authority,payload,signature,authority_digest FROM event_preference_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
        let supplied = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if supplied != config {
            return Err("authority configuration mismatch".into());
        }
        let authority = authorities
            .get(&authority, AuthorityRole::PreferenceReviewer)
            .ok_or("unknown or revoked preference authority")?;
        let signature: [u8; 64] = signature
            .try_into()
            .map_err(|_| "invalid preference signature")?;
        if !verify_attestation(
            authority,
            AuthorityRole::PreferenceReviewer,
            &payload,
            &signature,
        ) {
            return Err("preference signature mismatch".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-preference-recheck-v1","receipt_id":id,"trace_id":trace,"signature_rechecked":true,"authority_config_digest_matched":true,"authority_config_verified":false,"comparability_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Atomically check and retain caller-pinned synthetic signed claims and a ledger receipt.
    pub fn record_synthetic_attestation_receipt(
        &mut self,
        trace_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        trace: crate::attestation::NamedSignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticOutcomeEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        use crate::attestation::{AuthorityRole, SignedClaim};
        let source = authorities
            .get(trace.authority_id, AuthorityRole::TraceSource)
            .ok_or("unknown or revoked trace authority")?;
        let observer = authorities
            .get(outcome.authority_id, AuthorityRole::OutcomeObserver)
            .ok_or("unknown or revoked outcome authority")?;
        let trace_payload = trace.evidence.canonical_bytes()?;
        let outcome_payload = outcome.evidence.canonical_bytes()?;
        let config_digest = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        Self::check_synthetic_attestation_links_conn(
            &tx,
            trace_id,
            SignedClaim {
                evidence: trace.evidence,
                authority: source,
                signature: trace.signature,
            },
            SignedClaim {
                evidence: outcome.evidence,
                authority: observer,
                signature: outcome.signature,
            },
            checkpoint,
        )?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_attestation_receipts(trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![trace_id,trace.authority_id,outcome.authority_id,trace_payload,outcome_payload,trace.signature.as_slice(),outcome.signature.as_slice(),config_digest,checkpoint.0,checkpoint.1,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_attestation_receipt(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Recheck retained signatures against a caller-pinned authority set and verified receipt.
    pub fn recheck_synthetic_attestation_receipt(
        &mut self,
        id: i64,
        authorities: &crate::attestation::AuthoritySet,
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{verify_attestation, AuthorityRole};
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (trace,source,observer,tp,op,ts,os,config):(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,String)=tx.query_row("SELECT trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest FROM event_attestation_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).map_err(|e|e.to_string())?;
        let supplied = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if supplied != config {
            return Err("authority configuration mismatch".into());
        }
        let source = authorities
            .get(&source, AuthorityRole::TraceSource)
            .ok_or("unknown or revoked trace authority")?;
        let observer = authorities
            .get(&observer, AuthorityRole::OutcomeObserver)
            .ok_or("unknown or revoked outcome authority")?;
        let trace_signature: [u8; 64] = ts.try_into().map_err(|_| "invalid trace signature")?;
        let outcome_signature: [u8; 64] = os.try_into().map_err(|_| "invalid outcome signature")?;
        if !verify_attestation(source, AuthorityRole::TraceSource, &tp, &trace_signature)
            || !verify_attestation(
                observer,
                AuthorityRole::OutcomeObserver,
                &op,
                &outcome_signature,
            )
        {
            return Err("attestation signature mismatch".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-attestation-recheck-v1","receipt_id":id,"trace_id":trace,"signature_rechecked":true,"authority_config_digest_matched":true,"authority_config_verified":false,"outcome_truth_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Inspect a ledger-bound receipt. It does not recheck signatures or authority custody.
    pub fn inspect_synthetic_attestation_receipt(
        &mut self,
        id: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (trace,source,observer,tp,op,config,seq,hash,time):(i64,String,String,Vec<u8>,Vec<u8>,String,i64,String,String)=tx.query_row("SELECT trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns FROM event_attestation_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(
            serde_json::json!({"schema":"sese-synthetic-attestation-receipt-v1","id":id,"trace_id":trace,"trace_authority":source,"outcome_authority":observer,"trace_payload_sha256":digest(&tp),"outcome_payload_sha256":digest(&op),"authority_config_sha256":config,"checkpoint":{"seq":seq,"hash":hash},"created_unix_ns":time,"signature_rechecked":false,"authority_config_verified":false,"outcome_truth_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Check that signed synthetic outcome evidence names and matches a preselected fixture receipt.
    /// This does not prove fixture independence, pin custody or real-world outcome truth.
    pub fn check_signed_synthetic_followup(
        &mut self,
        trace_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        trace: crate::attestation::NamedSignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticOutcomeEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let result = Self::check_signed_synthetic_followup_conn(
            &tx,
            trace_id,
            followup_id,
            authorities,
            trace,
            outcome,
            checkpoint,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    fn check_signed_synthetic_followup_conn(
        tx: &Connection,
        trace_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        trace: crate::attestation::NamedSignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticOutcomeEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{AuthorityRole, SignedClaim, SyntheticOutcome};
        if followup_id <= 0 {
            return Err("invalid follow-up receipt".into());
        }
        let source = authorities
            .get(trace.authority_id, AuthorityRole::TraceSource)
            .ok_or("unknown or revoked trace authority")?;
        let observer = authorities
            .get(outcome.authority_id, AuthorityRole::OutcomeObserver)
            .ok_or("unknown or revoked outcome authority")?;
        Self::check_synthetic_attestation_links_conn(
            tx,
            trace_id,
            SignedClaim {
                evidence: trace.evidence,
                authority: source,
                signature: trace.signature,
            },
            SignedClaim {
                evidence: outcome.evidence,
                authority: observer,
                signature: outcome.signature,
            },
            checkpoint,
        )?;
        let receipt = Self::inspect_synthetic_followup_receipt_conn(tx, followup_id)?;
        let (review, receipt_seq, receipt_hash, receipt_time): (i64, i64, String, String) = tx.query_row("SELECT f.review_id,e.seq,e.hash,f.created_unix_ns FROM event_followup_receipts f JOIN ledger_events e ON e.kind='event_followup_receipt' AND e.reference=f.id WHERE f.id=?1", [followup_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e| e.to_string())?;
        let outcome_seq: i64 = tx
            .query_row(
                "SELECT seq FROM ledger_events WHERE kind='event_outcome' AND reference=?1",
                [review],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let receipt_time: u64 = receipt_time
            .parse()
            .map_err(|_| "invalid follow-up receipt time")?;
        let observed_status = match receipt["comparison"]["status"].as_str() {
            Some("match") => "validated",
            Some("verdict_mismatch") => "falsified",
            _ => return Err("follow-up cannot support an outcome status".into()),
        };
        let signed_status = match outcome.evidence.outcome {
            SyntheticOutcome::Validated => "validated",
            SyntheticOutcome::Falsified => "falsified",
        };
        if review != outcome.evidence.reference.review_id
            || receipt_seq >= outcome_seq
            || outcome_seq > checkpoint.0
            || outcome.evidence.method != "synthetic-followup-receipt-v1"
            || outcome.evidence.evidence_reference
                != format!("sese-followup:{followup_id}:{receipt_hash}")
            || outcome.evidence.observed_unix_ns < receipt_time
            || observed_status != signed_status
        {
            return Err("signed outcome does not match follow-up receipt".into());
        }
        let result = serde_json::json!({"schema":"sese-signed-synthetic-followup-check-v1",
            "trace_id":trace_id,"review_id":review,"followup_receipt_id":followup_id,"followup_ledger_hash":receipt_hash,
            "outcome":signed_status,"observer_authority":outcome.authority_id,"signed_followup_matches":true,"signatures_checked":true,
            "authority_config_sha256":authorities.digest().iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "checkpoint":{"seq":checkpoint.0,"hash":checkpoint.1},"checkpoint_custody_verified":false,
            "authority_config_verified":false,"observer_independence_verified":false,"fixture_independence_verified":false,
            "real_world_outcome_verified":false,"synthetic":true,"training_ready":false});
        Ok(result)
    }

    /// Record signed claims only after their follow-up binding is verified in the write transaction.
    pub fn record_signed_synthetic_followup(
        &mut self,
        trace_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        trace: crate::attestation::NamedSignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticOutcomeEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<i64, String> {
        let trace_payload = trace.evidence.canonical_bytes()?;
        let outcome_payload = outcome.evidence.canonical_bytes()?;
        let config: String = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        Self::check_signed_synthetic_followup_conn(
            &tx,
            trace_id,
            followup_id,
            authorities,
            crate::attestation::NamedSignedClaim {
                evidence: trace.evidence,
                authority_id: trace.authority_id,
                signature: trace.signature,
            },
            crate::attestation::NamedSignedClaim {
                evidence: outcome.evidence,
                authority_id: outcome.authority_id,
                signature: outcome.signature,
            },
            checkpoint,
        )?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_attestation_receipts(trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![trace_id,trace.authority_id,outcome.authority_id,trace_payload,outcome_payload,trace.signature.as_slice(),outcome.signature.as_slice(),config,checkpoint.0,checkpoint.1,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_attestation_receipt(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Recheck retained canonical claims without requiring the original bundle.
    pub fn recheck_retained_signed_followup(
        &mut self,
        receipt_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let result = Self::recheck_retained_signed_followup_conn(
            &tx,
            receipt_id,
            followup_id,
            authorities,
            checkpoint,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    fn recheck_retained_signed_followup_conn(
        tx: &Connection,
        receipt_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        if receipt_id <= 0 {
            return Err("invalid attestation receipt".into());
        }
        if !crate::ledger::verify_storage(tx)? {
            return Err("ledger integrity failure".into());
        }
        let row: (i64, String, String, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, String, i64) = tx.query_row("SELECT a.trace_id,a.trace_authority,a.outcome_authority,a.trace_payload,a.outcome_payload,a.trace_signature,a.outcome_signature,a.authority_digest,e.seq FROM event_attestation_receipts a JOIN ledger_events e ON e.kind='event_attestation_receipt' AND e.reference=a.id WHERE a.id=?1", [receipt_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        let config: String = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if row.7 != config || row.8 > checkpoint.0 {
            return Err("attestation configuration or checkpoint mismatch".into());
        }
        let trace = crate::attestation::SyntheticTraceEvidence::from_canonical_bytes(&row.3)?;
        let outcome = crate::attestation::SyntheticOutcomeEvidence::from_canonical_bytes(&row.4)?;
        let trace_signature: [u8; 64] = row.5.try_into().map_err(|_| "invalid trace signature")?;
        let outcome_signature: [u8; 64] =
            row.6.try_into().map_err(|_| "invalid outcome signature")?;
        let mut result = Self::check_signed_synthetic_followup_conn(
            tx,
            row.0,
            followup_id,
            authorities,
            crate::attestation::NamedSignedClaim {
                evidence: &trace,
                authority_id: &row.1,
                signature: &trace_signature,
            },
            crate::attestation::NamedSignedClaim {
                evidence: &outcome,
                authority_id: &row.2,
                signature: &outcome_signature,
            },
            checkpoint,
        )?;
        result["attestation_receipt_id"] = serde_json::json!(receipt_id);
        result["retained_claims_rechecked"] = serde_json::json!(true);
        Ok(result)
    }

    /// Export one synthetic example only under an explicitly accepted operator-declared trust policy.
    /// External custody and independence are assumptions, not machine-verified facts.
    pub fn export_conditionally_trusted_synthetic_training(
        &mut self,
        format: &str,
        receipt_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        rule: Option<&crate::reward_rule::SyntheticRewardRule>,
        operator_accepts: bool,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let row = Self::export_conditionally_trusted_synthetic_training_conn(
            &tx,
            format,
            receipt_id,
            followup_id,
            authorities,
            policy,
            rule,
            operator_accepts,
            checkpoint,
            false,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(row)
    }

    /// Inspection-only synthetic preview. A policy that permits export is not accepted.
    pub fn preview_synthetic_training(
        &mut self,
        format: &str,
        receipt_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        rule: Option<&crate::reward_rule::SyntheticRewardRule>,
        operator_accepts: bool,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let row = Self::export_conditionally_trusted_synthetic_training_conn(
            &tx,
            format,
            receipt_id,
            followup_id,
            authorities,
            policy,
            rule,
            operator_accepts,
            checkpoint,
            true,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(row)
    }

    fn export_conditionally_trusted_synthetic_training_conn(
        tx: &Connection,
        format: &str,
        receipt_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        rule: Option<&crate::reward_rule::SyntheticRewardRule>,
        operator_accepts: bool,
        checkpoint: (i64, &str),
        preview: bool,
    ) -> Result<serde_json::Value, String> {
        if !operator_accepts {
            return Err("operator acceptance required".into());
        }
        if format != "sft" && format != "rlvr" {
            return Err("unsupported training format".into());
        }
        if preview == policy.training_export_allowed() {
            return Err(if preview {
                "preview requires a non-export policy"
            } else {
                "policy does not permit training export"
            }
            .into());
        }
        if policy.authority_digest() != authorities.digest() {
            return Err("trust policy authority mismatch".into());
        }
        if format == "rlvr" && rule.is_none() {
            return Err("reward rule required".into());
        }
        let checked = Self::recheck_retained_signed_followup_conn(
            tx,
            receipt_id,
            followup_id,
            authorities,
            checkpoint,
        )?;
        let (trace_id, source_authority, observer_authority, trace_payload, outcome_payload, trace_sig, outcome_sig): (i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>) = tx.query_row("SELECT trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature FROM event_attestation_receipts WHERE id=?1", [receipt_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        let declaration = policy
            .common_declaration(&source_authority, &observer_authority)
            .ok_or("authority relationship not declared")?;
        let trace =
            crate::attestation::SyntheticTraceEvidence::from_canonical_bytes(&trace_payload)?;
        let outcome =
            crate::attestation::SyntheticOutcomeEvidence::from_canonical_bytes(&outcome_payload)?;
        // Training traces must be committed before the outcome, and the
        // source's claimed capture time must not follow the observation.
        // This ordering is not authentication of the claimed timestamp.
        let (trace_seq, outcome_seq): (i64, i64) = tx.query_row(
            "SELECT t.seq,o.seq FROM event_training_traces tr JOIN ledger_events t ON t.kind='event_training_trace' AND t.reference=tr.id JOIN ledger_events o ON o.kind='event_outcome' AND o.reference=tr.review_id WHERE tr.id=?1",
            [trace_id], |r| Ok((r.get(0)?,r.get(1)?)),
        ).map_err(|e| e.to_string())?;
        if trace_seq >= outcome_seq || trace.captured_unix_ns > outcome.observed_unix_ns {
            return Err("training trace must precede the observed outcome".into());
        }
        let status = checked["outcome"]
            .as_str()
            .ok_or("missing checked outcome")?;
        if format == "sft" && status != "validated" {
            return Err("sft requires validated outcome".into());
        }
        let (candidate, replay, review, action, actor, fixture_version, fixture_digest, trace_actor, outcome_actor, outcome_source): (String,i64,i64,String,String,String,String,String,String,String) = tx.query_row("SELECT r.candidate_id,r.replay_id,r.id,r.action,r.actor,p.pack_version,p.pack_digest,t.actor,o.actor,o.source FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN event_review_outcomes o ON o.review_id=r.id WHERE t.id=?1", [trace_id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).map_err(|e|e.to_string())?;
        let followup = Self::inspect_synthetic_followup_receipt_conn(tx, followup_id)?;
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let mut row = serde_json::json!({
            "schema":if format == "sft" {"sese-synthetic-sft-v1"} else {"sese-synthetic-rlvr-v1"},
            "synthetic":true,"eligibility":"conditional_on_operator_declared_trust","trust_basis":"operator_declared",
            "training_export_allowed_by_policy":policy.training_export_allowed(),
            "synthetic_training_eligible_under_operator_assumptions":true,"training_eligibility_independently_verified":false,
            "operator_acceptance":"caller_asserted","operator_identity_verified":false,"observer_independence_verified":false,"fixture_independence_verified":false,
            "authority_custody_verified":false,"checkpoint_custody_verified":false,"real_world_outcome_verified":false,
            "messages":[{"role":"user","content":trace.prompt},{"role":"assistant","content":trace.response}],
            "candidate":{"sha256":candidate,"replay_id":replay,"fixture_version":fixture_version,"fixture_sha256":fixture_digest},
            "decision":{"review_id":review,"action":action,"actor":actor},
            "trace":{"id":trace_id,"actor":trace_actor,"source":trace.source,"method":trace.method,"captured_unix_ns":trace.captured_unix_ns,"authority":source_authority,"payload_hex":hex(&trace_payload),"signature_hex":hex(&trace_sig),"model_origin_verified":false},
            "outcome":{"status":status,"actor":outcome_actor,"source":outcome_source,"method":outcome.method,"evidence_reference":outcome.evidence_reference,"observed_unix_ns":outcome.observed_unix_ns,"authority":observer_authority,"payload_hex":hex(&outcome_payload),"signature_hex":hex(&outcome_sig),"evidence":"synthetic_fixture_comparison"},
            "followup":followup,"attestation_receipt_id":receipt_id,"operator_declaration":declaration,
            "authority_config_sha256":hex(&authorities.digest()),"trust_policy_sha256":hex(&policy.digest()),
            "checkpoint":{"seq":checkpoint.0,"hash":checkpoint.1},"signatures_rechecked":true
        });
        if preview {
            row["export_kind"] = serde_json::json!("inspection_only");
            row["eligibility"] = serde_json::json!("preview_ineligible");
            row["synthetic_training_eligible_under_operator_assumptions"] =
                serde_json::json!(false);
        }
        if format == "rlvr" {
            let rule = rule.ok_or("reward rule required")?;
            row["reward"] = serde_json::json!(rule.reward(status)?);
            row["reward_rule"] = serde_json::json!({"version":rule.version(),"sha256":hex(&rule.digest()),"actor":rule.actor(),"source":rule.source(),"custody_verified":false});
        }
        Ok(row)
    }

    /// Export an explicit signed pair under a separately declared reviewer relationship.
    pub fn export_conditionally_trusted_synthetic_preference(
        &mut self,
        receipt_id: i64,
        followup_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        operator_accepts: bool,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        self.synthetic_preference_row(
            receipt_id,
            followup_id,
            preference_id,
            authorities,
            policy,
            operator_accepts,
            checkpoint,
            false,
        )
    }

    /// Inspection-only pair preview under a non-export policy.
    pub fn preview_synthetic_preference(
        &mut self,
        receipt_id: i64,
        followup_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        operator_accepts: bool,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        self.synthetic_preference_row(
            receipt_id,
            followup_id,
            preference_id,
            authorities,
            policy,
            operator_accepts,
            checkpoint,
            true,
        )
    }

    fn synthetic_preference_row(
        &mut self,
        receipt_id: i64,
        followup_id: i64,
        preference_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        policy: &crate::attestation::SyntheticTrustPolicy,
        operator_accepts: bool,
        checkpoint: (i64, &str),
        preview: bool,
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{AuthorityRole, SignedClaim, SyntheticPreferenceEvidence};
        if preference_id <= 0 {
            return Err("invalid preference receipt".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let mut row = Self::export_conditionally_trusted_synthetic_training_conn(
            &tx,
            "sft",
            receipt_id,
            followup_id,
            authorities,
            policy,
            None,
            operator_accepts,
            checkpoint,
            preview,
        )?;
        let (trace_id, reviewer_id, payload, signature, config, seq, recorded): (i64,String,Vec<u8>,Vec<u8>,String,i64,String) = tx.query_row("SELECT p.trace_id,p.authority,p.payload,p.signature,p.authority_digest,e.seq,p.created_unix_ns FROM event_preference_receipts p JOIN ledger_events e ON e.kind='event_preference_receipt' AND e.reference=p.id WHERE p.id=?1", [preference_id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        if trace_id != row["trace"]["id"].as_i64().ok_or("invalid trace")?
            || config != hex(&authorities.digest())
            || seq > checkpoint.0
        {
            return Err("preference receipt or checkpoint mismatch".into());
        }
        let source = row["trace"]["authority"]
            .as_str()
            .ok_or("missing source authority")?;
        let observer = row["outcome"]["authority"]
            .as_str()
            .ok_or("missing observer authority")?;
        let reviewer_declaration = policy
            .reviewer_declaration(source, observer, &reviewer_id)
            .ok_or("reviewer relationship not declared")?;
        let evidence = SyntheticPreferenceEvidence::from_canonical_bytes(&payload)?;
        let recorded: u64 = recorded
            .parse()
            .map_err(|_| "invalid preference receipt time")?;
        if evidence.trace_id != trace_id || evidence.reviewed_unix_ns > recorded {
            return Err("preference trace or review time mismatch".into());
        }
        let signature_array: [u8; 64] = signature
            .clone()
            .try_into()
            .map_err(|_| "invalid preference signature")?;
        let reviewer = authorities
            .get(&reviewer_id, AuthorityRole::PreferenceReviewer)
            .ok_or("unknown or revoked preference authority")?;
        Self::check_synthetic_preference_link_conn(
            &tx,
            SignedClaim {
                evidence: &evidence,
                authority: reviewer,
                signature: &signature_array,
            },
            checkpoint,
        )?;
        row["schema"] = serde_json::json!("sese-synthetic-dpo-v1");
        row["prompt"] = serde_json::json!(evidence.prompt);
        row["chosen"] = serde_json::json!(evidence.preferred);
        row["rejected"] = serde_json::json!(evidence.rejected);
        row.as_object_mut()
            .ok_or("invalid export row")?
            .remove("messages");
        row["preference"] = serde_json::json!({"receipt_id":preference_id,"authority":reviewer_id,"source":evidence.source,
            "comparability_basis":evidence.comparability_basis,"reviewed_unix_ns":evidence.reviewed_unix_ns,
            "payload_hex":hex(&payload),"signature_hex":hex(&signature)});
        row["reviewer_declaration"] = reviewer_declaration;
        row["reviewer_independence_verified"] = serde_json::json!(false);
        row["comparability_verified"] = serde_json::json!(false);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(row)
    }

    /// Recheck a retained signed claim and its follow-up using the original supplied bundle.
    pub fn recheck_signed_synthetic_followup(
        &mut self,
        receipt_id: i64,
        followup_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        trace: crate::attestation::NamedSignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticOutcomeEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        if receipt_id <= 0 {
            return Err("invalid attestation receipt".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let row: (i64, String, String, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, String, i64) = tx.query_row("SELECT a.trace_id,a.trace_authority,a.outcome_authority,a.trace_payload,a.outcome_payload,a.trace_signature,a.outcome_signature,a.authority_digest,e.seq FROM event_attestation_receipts a JOIN ledger_events e ON e.kind='event_attestation_receipt' AND e.reference=a.id WHERE a.id=?1", [receipt_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        let config: String = authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if row.1 != trace.authority_id
            || row.2 != outcome.authority_id
            || row.3 != trace.evidence.canonical_bytes()?
            || row.4 != outcome.evidence.canonical_bytes()?
            || row.5 != trace.signature
            || row.6 != outcome.signature
            || row.7 != config
            || row.8 > checkpoint.0
        {
            return Err("attestation receipt or checkpoint mismatch".into());
        }
        let mut result = Self::check_signed_synthetic_followup_conn(
            &tx,
            row.0,
            followup_id,
            authorities,
            trace,
            outcome,
            checkpoint,
        )?;
        result["attestation_receipt_id"] = serde_json::json!(receipt_id);
        result["retained_claim_matches"] = serde_json::json!(true);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    /// Resolve two claims against a caller-pinned authority set. Pin custody is not verified.
    pub fn check_synthetic_attestation_links_in_set(
        &mut self,
        trace_id: i64,
        authorities: &crate::attestation::AuthoritySet,
        trace: crate::attestation::NamedSignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::NamedSignedClaim<
            '_,
            crate::attestation::SyntheticOutcomeEvidence,
        >,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{AuthorityRole, SignedClaim};
        let source = authorities
            .get(trace.authority_id, AuthorityRole::TraceSource)
            .ok_or("unknown or revoked trace authority")?;
        let observer = authorities
            .get(outcome.authority_id, AuthorityRole::OutcomeObserver)
            .ok_or("unknown or revoked outcome authority")?;
        let mut result = self.check_synthetic_attestation_links(
            trace_id,
            SignedClaim {
                evidence: trace.evidence,
                authority: source,
                signature: trace.signature,
            },
            SignedClaim {
                evidence: outcome.evidence,
                authority: observer,
                signature: outcome.signature,
            },
            checkpoint,
        )?;
        result["authority_config_digest_matched"] = serde_json::json!(true);
        result["authority_config_sha256"] = serde_json::json!(authorities
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>());
        Ok(result)
    }

    /// Check signatures and links to checkpointed synthetic records. This is not a trust or eligibility decision.
    pub fn check_synthetic_attestation_links(
        &mut self,
        trace_id: i64,
        trace: crate::attestation::SignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::SignedClaim<'_, crate::attestation::SyntheticOutcomeEvidence>,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let result = Self::check_synthetic_attestation_links_conn(
            &tx, trace_id, trace, outcome, checkpoint,
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    fn check_synthetic_attestation_links_conn(
        tx: &Connection,
        trace_id: i64,
        trace: crate::attestation::SignedClaim<'_, crate::attestation::SyntheticTraceEvidence>,
        outcome: crate::attestation::SignedClaim<'_, crate::attestation::SyntheticOutcomeEvidence>,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        use crate::attestation::{
            verify_synthetic_outcome, verify_synthetic_trace, SyntheticOutcome,
        };
        if trace_id <= 0
            || checkpoint.0 <= 0
            || trace.authority.public_key == outcome.authority.public_key
            || trace.evidence.reference != outcome.evidence.reference
            || !verify_synthetic_trace(trace.authority, trace.evidence, trace.signature)
            || !verify_synthetic_outcome(outcome.authority, outcome.evidence, outcome.signature)
        {
            return Err("invalid synthetic attestation".into());
        }
        if !crate::ledger::verify_storage(tx)? {
            return Err("ledger integrity failure".into());
        }
        let anchor: Option<String> = tx
            .query_row(
                "SELECT hash FROM ledger_events WHERE seq=?1",
                [checkpoint.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if anchor.as_deref() != Some(checkpoint.1) {
            return Err("checkpoint mismatch".into());
        }
        let (review, prompt, response, trace_source, trace_time, candidate, replay, review_time, fixture, review_hash, trace_seq, outcome_seq, status, outcome_source, outcome_time): (i64,String,String,String,String,String,i64,String,String,String,i64,i64,String,String,String) = tx.query_row("SELECT t.review_id,t.prompt,t.response,t.source,t.created_unix_ns,r.candidate_id,r.replay_id,r.created_unix_ns,p.pack_digest,er.hash,et.seq,eo.seq,o.status,o.source,o.created_unix_ns FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN event_review_outcomes o ON o.review_id=r.id JOIN ledger_events er ON er.kind='event_review' AND er.reference=r.id JOIN ledger_events et ON et.kind='event_training_trace' AND et.reference=t.id JOIN ledger_events eo ON eo.kind='event_outcome' AND eo.reference=r.id WHERE t.id=?1",[trace_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?,r.get(14)?))).map_err(|e|e.to_string())?;
        let trace_recorded: u64 = trace_time.parse().map_err(|_| "invalid trace time")?;
        let review_recorded: u64 = review_time.parse().map_err(|_| "invalid review time")?;
        let outcome_recorded: u64 = outcome_time.parse().map_err(|_| "invalid outcome time")?;
        let replay_evidence = Self::verify_event_replay_in_verified_snapshot(tx, replay)?;
        if replay_evidence.candidate_id != candidate || replay_evidence.pack_digest != fixture {
            return Err("invalid replay provenance".into());
        }
        let expected_status = match outcome.evidence.outcome {
            SyntheticOutcome::Validated => "validated",
            SyntheticOutcome::Falsified => "falsified",
        };
        let reference = &trace.evidence.reference;
        if checkpoint.0 < trace_seq.max(outcome_seq)
            || reference.review_id != review
            || reference.replay_id != replay
            || reference.candidate_sha256 != candidate
            || reference.fixture_sha256 != fixture
            || reference.review_ledger_hash != review_hash
            || trace.evidence.prompt != prompt
            || trace.evidence.response != response
            || trace.evidence.source != trace_source
            || outcome.evidence.source != outcome_source
            || status != expected_status
            || trace.evidence.captured_unix_ns > trace_recorded
            || outcome.evidence.observed_unix_ns < review_recorded
            || outcome.evidence.observed_unix_ns > outcome_recorded
        {
            return Err("attestation does not match checkpointed evidence".into());
        }
        Ok(
            serde_json::json!({"schema":"sese-synthetic-attestation-link-check-v1","trace_id":trace_id,"review_id":review,"signature_and_record_links_match":true,"authority_config_verified":false,"observer_independence_verified":false,"outcome_truth_verified":false,"synthetic":true,"training_ready":false}),
        )
    }

    /// Store a caller-asserted synthetic trace; this does not establish training eligibility.
    pub fn record_synthetic_event_trace(
        &mut self,
        review: i64,
        input: &SyntheticTraceInput,
    ) -> Result<i64, String> {
        if input.schema != "sese-synthetic-training-trace-v1"
            || [&input.actor, &input.source, &input.prompt, &input.response]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 16_384)
        {
            return Err("invalid synthetic trace".into());
        }
        let (preferred, rejected, pair_source) = match input.preference.as_ref() {
            None => (None, None, None),
            Some(pair)
                if [&pair.preferred, &pair.rejected, &pair.source]
                    .iter()
                    .all(|s| !s.trim().is_empty() && s.len() <= 16_384)
                    && pair.preferred != pair.rejected =>
            {
                (
                    Some(pair.preferred.as_str()),
                    Some(pair.rejected.as_str()),
                    Some(pair.source.as_str()),
                )
            }
            _ => return Err("invalid explicit preference pair".into()),
        };
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_training_traces(review_id,actor,source,prompt,response,preferred,rejected,pair_source,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![review,input.actor,input.source,input.prompt,input.response,preferred,rejected,pair_source,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_training_trace(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn inspect_synthetic_event_trace(&mut self, id: i64) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (review,actor,source,prompt,response,preferred,rejected,pair_source,time):(i64,String,String,String,String,Option<String>,Option<String>,Option<String>,String)=tx.query_row("SELECT review_id,actor,source,prompt,response,preferred,rejected,pair_source,created_unix_ns FROM event_training_traces WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
        let outcome: Option<String> = tx
            .query_row(
                "SELECT status FROM event_review_outcomes WHERE review_id=?1",
                [review],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let preference = match (preferred, rejected, pair_source) {
            (Some(p), Some(r), Some(s)) => {
                Some(serde_json::json!({"preferred":p,"rejected":r,"source":s}))
            }
            (None, None, None) => None,
            _ => return Err("invalid preference pair".into()),
        };
        let value = serde_json::json!({"schema":"sese-synthetic-training-trace-inspection-v1","id":id,"review_id":review,"actor":actor,"source":source,"prompt":prompt,"response":response,"preference":preference,"created_unix_ns":time,"outcome":outcome.unwrap_or_else(||"pending".into()),"provenance":"caller_asserted_synthetic","training_ready":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    /// Compare a trace response interpreted as WAT with its reviewed artifact, without executing it.
    pub fn compare_event_trace_wat(&mut self, id: i64) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (review,candidate,response):(i64,String,String)=tx.query_row("SELECT t.review_id,r.candidate_id,t.response FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id WHERE t.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let (status, compiled_digest) = match wat::parse_str(&response) {
            Err(_) => ("invalid_wat", None),
            Ok(bytes) if bytes.len() > 65_536 => ("output_too_large", None),
            Ok(bytes) => {
                let hash = digest(&bytes);
                (
                    if hash == candidate {
                        "match"
                    } else {
                        "mismatch"
                    },
                    Some(hash),
                )
            }
        };
        let value = serde_json::json!({"schema":"sese-synthetic-trace-wat-comparison-v1","trace_id":id,"review_id":review,"candidate_id":candidate,"status":status,"compiled_sha256":compiled_digest,"executed":false,"provenance":"caller_asserted_synthetic","training_ready":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    /// Explain why current caller-asserted synthetic records do not meet training gates.
    pub fn synthetic_event_trace_eligibility(
        &mut self,
        id: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (review, has_pair, outcome):(i64,bool,Option<String>)=tx.query_row("SELECT t.review_id,t.preferred IS NOT NULL,o.status FROM event_training_traces t LEFT JOIN event_review_outcomes o ON o.review_id=t.review_id WHERE t.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let mut common = vec!["trace_origin_caller_asserted"];
        match outcome.as_deref() {
            None => common.push("outcome_pending"),
            Some("validated" | "falsified") => common.push("outcome_is_caller_asserted"),
            _ => return Err("invalid outcome".into()),
        }
        let mut sft = common.clone();
        let mut dpo = common.clone();
        let mut rlvr = common;
        if outcome.as_deref() == Some("falsified") {
            sft.push("validated_outcome_required");
            dpo.push("validated_outcome_required");
        }
        dpo.push(if has_pair {
            "preference_comparability_unverified"
        } else {
            "preference_pair_missing"
        });
        rlvr.push("reward_derivation_missing");
        let value = serde_json::json!({"schema":"sese-synthetic-training-eligibility-v1","trace_id":id,"review_id":review,"outcome":outcome.as_deref().unwrap_or("pending"),"outcome_evidence":if outcome.is_some() { "caller_asserted" } else { "none" },"provenance":"caller_asserted_synthetic","preference_recorded":has_pair,"formats":{"sft":{"eligible":false,"blockers":sft},"dpo":{"eligible":false,"blockers":dpo},"rlvr":{"eligible":false,"blockers":rlvr}},"training_ready":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    /// Select a separate synthetic replay case for later comparison. This is not fixture authority.
    pub fn pin_synthetic_event_followup(
        &mut self,
        domain: &str,
        replay: i64,
        case_id: &str,
        actor: &str,
    ) -> Result<i64, String> {
        if [domain, case_id, actor]
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256)
        {
            return Err("invalid follow-up selection".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let follow = Self::verify_event_replay_conn(&tx, replay)?;
        if !follow.passed {
            return Err("follow-up replay failed".into());
        }
        let primary: i64 = tx
            .query_row(
                "SELECT replay_id FROM event_fixture_pins WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let main = Self::verify_event_replay_conn(&tx, primary)?;
        if main.candidate_id != follow.candidate_id
            || main.state_semantics != follow.state_semantics
            || main.pack_digest == follow.pack_digest
        {
            return Err(
                "follow-up fixture must differ and target the same candidate and semantics".into(),
            );
        }
        let (main_worker,follow_worker):(Option<String>,Option<String>)=tx.query_row("SELECT (SELECT worker_sha256 FROM event_replay_runs WHERE id=?1),(SELECT worker_sha256 FROM event_replay_runs WHERE id=?2)",params![primary,replay],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        if main_worker.is_none() || main_worker != follow_worker {
            return Err("follow-up worker differs from reviewed fixture worker".into());
        }
        let (window,event):(String,String)=tx.query_row("SELECT window_json,event_json FROM event_replay_cases WHERE run_id=?1 AND case_id=?2",params![replay,case_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM event_replay_cases WHERE run_id=?1 AND window_json=?2 AND event_json=?3)",params![primary,window,event],|r|r.get(0)).map_err(|e|e.to_string())?;
        if duplicate {
            return Err("follow-up input duplicates a reviewed fixture case".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_followup_pins(domain,candidate_id,replay_id,case_id,actor,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6)",params![domain,follow.candidate_id,replay,case_id,actor,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_followup_pin(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Compare a post-promotion synthetic observation with a predeclared replay case.
    pub fn check_synthetic_event_followup(
        &mut self,
        review: i64,
        observation: i64,
        case_id: &str,
    ) -> Result<serde_json::Value, String> {
        self.check_synthetic_event_followup_inner(review, observation, Some(case_id))
    }
    pub fn check_selected_synthetic_event_followup(
        &mut self,
        review: i64,
        observation: i64,
    ) -> Result<serde_json::Value, String> {
        self.check_synthetic_event_followup_inner(review, observation, None)
    }
    fn check_synthetic_event_followup_inner(
        &mut self,
        review: i64,
        observation: i64,
        selected_case: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let value =
            Self::check_synthetic_event_followup_conn(&tx, review, observation, selected_case)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    // Call only after verifying this transaction's stable snapshot.
    fn check_synthetic_event_followup_conn(
        tx: &Connection,
        review: i64,
        observation: i64,
        selected_case: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        if selected_case.is_some_and(|case_id| case_id.trim().is_empty() || case_id.len() > 256) {
            return Err("invalid case id".into());
        }
        let (candidate,replay,action,domain):(String,i64,String,Option<String>)=tx.query_row("SELECT r.candidate_id,r.replay_id,r.action,o.domain FROM event_reviews r JOIN event_observations o ON o.id=r.observation_id WHERE r.id=?1",[review],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
        let domain = domain.ok_or("review observation has no domain")?;
        let verified = Self::verify_event_replay_in_verified_snapshot(&tx, replay)?;
        if action != "approve" || !verified.passed || verified.candidate_id != candidate {
            return Err("review is not an approved passing synthetic replay".into());
        }
        if !crate::ledger::event_observation_binding_matches(&tx, observation)? {
            return Err("observation binding failure".into());
        }
        let (champion,observation_domain,prior,event,actual,error,observation_worker):(String,Option<String>,String,String,Option<String>,Option<String>,Option<String>)=tx.query_row("SELECT champion_id,domain,prior_json,event_json,champion_json,champion_error,worker_sha256 FROM event_observations WHERE id=?1",[observation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        let replay_worker: Option<String> = tx
            .query_row(
                "SELECT worker_sha256 FROM event_replay_runs WHERE id=?1",
                [replay],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if champion != candidate
            || observation_domain.as_deref() != Some(domain.as_str())
            || replay_worker.is_none()
            || replay_worker != observation_worker
        {
            return Err("follow-up candidate, domain, or worker does not match review".into());
        }
        let seq = |kind: &str, reference: i64| -> Result<i64, String> {
            tx.query_row(
                "SELECT seq FROM ledger_events WHERE kind=?1 AND reference=?2",
                params![kind, reference],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        };
        let replay_seq = seq("event_replay", replay)?;
        let review_seq = seq("event_review", review)?;
        let observation_seq = seq("event_observation", observation)?;
        if !(replay_seq < review_seq && review_seq < observation_seq) {
            return Err("follow-up is not after the reviewed fixture and decision".into());
        }
        let (kind,reference,next,transition_seq):(String,i64,String,i64)=tx.query_row("SELECT t.kind,t.reference,t.next_id,e.seq FROM event_champion_transitions t JOIN ledger_events e ON e.kind='event_champion_transition' AND e.reference=t.id WHERE t.domain=?1 AND e.seq<?2 ORDER BY e.seq DESC LIMIT 1",params![domain,observation_seq],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
        if kind != "promote"
            || reference != review
            || next != candidate
            || transition_seq <= review_seq
        {
            return Err("follow-up lacks the review's active promotion".into());
        }
        let (case_replay, case_id, case_fixture, evidence_kind, selection_id) = if let Some(
            case_id,
        ) = selected_case
        {
            (
                replay,
                case_id.to_owned(),
                verified,
                "synthetic_post_promotion_fixture_comparison",
                None,
            )
        } else {
            let (pin,follow_replay,case_id):(i64,i64,String)=tx.query_row("SELECT id,replay_id,case_id FROM event_followup_pins WHERE domain=?1 AND candidate_id=?2",params![domain,candidate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
            let pin_seq = seq("event_followup_pin", pin)?;
            let follow_seq = seq("event_replay", follow_replay)?;
            if !(follow_seq < pin_seq && pin_seq < review_seq) {
                return Err("follow-up selection was not recorded before the decision".into());
            }
            let follow = Self::verify_event_replay_in_verified_snapshot(&tx, follow_replay)?;
            if follow.candidate_id != candidate
                || follow.state_semantics != verified.state_semantics
                || follow.pack_digest == verified.pack_digest
            {
                return Err("follow-up fixture does not differ from reviewed fixture".into());
            }
            let follow_worker: Option<String> = tx
                .query_row(
                    "SELECT worker_sha256 FROM event_replay_runs WHERE id=?1",
                    [follow_replay],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if follow_worker != replay_worker {
                return Err("follow-up worker differs from reviewed fixture worker".into());
            }
            (
                follow_replay,
                case_id,
                follow,
                "synthetic_preselected_separate_fixture_comparison",
                Some(pin),
            )
        };
        let (window,fixture_event,expected):(String,String,String)=tx.query_row("SELECT window_json,event_json,expected_json FROM event_replay_cases WHERE run_id=?1 AND case_id=?2",params![case_replay,case_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        if selected_case.is_none() {
            let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM event_replay_cases WHERE run_id=?1 AND window_json=?2 AND event_json=?3)",params![replay,window,fixture_event],|r|r.get(0)).map_err(|e|e.to_string())?;
            if duplicate {
                return Err("follow-up input duplicates a reviewed fixture case".into());
            }
        }
        let status = crate::synthetic_followup::compare(
            &window,
            &fixture_event,
            &expected,
            &prior,
            &event,
            actual.as_deref(),
            error.as_deref(),
        )?;
        let value = serde_json::json!({"schema":"sese-synthetic-followup-check-v2","evidence_kind":evidence_kind,"selection_id":selection_id,"review_id":review,"review_replay_id":replay,"replay_id":case_replay,"observation_id":observation,"case_id":case_id,"candidate_id":candidate,"domain":domain,"fixture_version":case_fixture.pack_version,"fixture_digest":case_fixture.pack_digest,"status":status,"observation_error":error,"synthetic":true,"records_outcome":false,"training_ready":false});
        Ok(value)
    }
    /// Record a deterministic synthetic follow-up comparison, not a real-world outcome.
    pub fn record_selected_synthetic_followup(
        &mut self,
        review: i64,
        observation: i64,
    ) -> Result<i64, String> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let comparison = Self::check_synthetic_event_followup_conn(&tx, review, observation, None)?;
        let pin = comparison["selection_id"]
            .as_i64()
            .ok_or("missing follow-up selection")?;
        let status = comparison["status"]
            .as_str()
            .ok_or("missing follow-up status")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_followup_receipts(review_id,observation_id,pin_id,status,created_unix_ns) VALUES (?1,?2,?3,?4,?5)",params![review,observation,pin,status,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_followup_receipt(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Recompute the comparison as well as verifying its ledger binding.
    pub fn inspect_synthetic_followup_receipt(
        &mut self,
        id: i64,
    ) -> Result<serde_json::Value, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let value = Self::inspect_synthetic_followup_receipt_conn(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }

    /// Compare a caller-asserted outcome to a separately selected synthetic fixture result.
    /// Matching does not establish causation, independence, real-world truth, or eligibility.
    pub fn check_synthetic_followup_outcome(
        &mut self,
        receipt_id: i64,
        checkpoint: (i64, &str),
    ) -> Result<serde_json::Value, String> {
        if receipt_id <= 0 || checkpoint.0 <= 0 {
            return Err("invalid follow-up outcome query".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let anchor: Option<String> = tx
            .query_row(
                "SELECT hash FROM ledger_events WHERE seq=?1",
                [checkpoint.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if anchor.as_deref() != Some(checkpoint.1) {
            return Err("checkpoint mismatch".into());
        }
        let receipt = Self::inspect_synthetic_followup_receipt_conn(&tx, receipt_id)?;
        let (review, receipt_seq, receipt_hash): (i64, i64, String) = tx.query_row("SELECT f.review_id,e.seq,e.hash FROM event_followup_receipts f JOIN ledger_events e ON e.kind='event_followup_receipt' AND e.reference=f.id WHERE f.id=?1", [receipt_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e| e.to_string())?;
        let (status, actor, source, outcome_seq, outcome_hash): (String, String, String, i64, String) = tx.query_row("SELECT o.status,o.actor,o.source,e.seq,e.hash FROM event_review_outcomes o JOIN ledger_events e ON e.kind='event_outcome' AND e.reference=o.review_id WHERE o.review_id=?1", [review], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e| e.to_string())?;
        if !(receipt_seq < outcome_seq && outcome_seq <= checkpoint.0) {
            return Err("outcome does not follow covered receipt".into());
        }
        let comparison = receipt["comparison"]["status"]
            .as_str()
            .ok_or("missing comparison status")?;
        let expected = match comparison {
            "match" => "validated",
            "verdict_mismatch" => "falsified",
            _ => return Err("comparison cannot support an outcome status".into()),
        };
        let value = serde_json::json!({"schema":"sese-synthetic-followup-outcome-consistency-v1",
            "review_id":review,"receipt_id":receipt_id,"receipt_ledger_hash":receipt_hash,
            "outcome":status,"outcome_actor":actor,"outcome_source":source,"outcome_ledger_hash":outcome_hash,
            "outcome_evidence":"caller_asserted","comparison":comparison,"outcome_matches_comparison":status == expected,
            "checkpoint":{"seq":checkpoint.0,"hash":checkpoint.1},"checkpoint_custody_verified":false,
            "fixture_independence_verified":false,"real_world_outcome_verified":false,"synthetic":true,"training_ready":false});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    // Call only after verifying this transaction's stable snapshot.
    fn inspect_synthetic_followup_receipt_conn(
        tx: &Connection,
        id: i64,
    ) -> Result<serde_json::Value, String> {
        let (review,observation,pin,status,time):(i64,i64,i64,String,String)=tx.query_row("SELECT review_id,observation_id,pin_id,status,created_unix_ns FROM event_followup_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
        let comparison = Self::check_synthetic_event_followup_conn(&tx, review, observation, None)?;
        if comparison["selection_id"].as_i64() != Some(pin)
            || comparison["status"].as_str() != Some(status.as_str())
        {
            return Err("follow-up receipt does not match recomputed comparison".into());
        }
        let value = serde_json::json!({"schema":"sese-synthetic-followup-receipt-v2","id":id,"created_unix_ns":time,"comparison":comparison});
        Ok(value)
    }
    /// Records a caller assertion about the decision, not an independently verified outcome.
    pub fn resolve_event_review(
        &mut self,
        review: i64,
        status: &str,
        actor: &str,
        source: &str,
    ) -> Result<(), String> {
        if !["validated", "falsified"].contains(&status)
            || [actor, source]
                .iter()
                .any(|v| v.trim().is_empty() || v.len() > 16_384)
        {
            return Err("invalid event outcome".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_review_outcomes(review_id,status,actor,source,created_unix_ns) VALUES (?1,?2,?3,?4,?5)",params![review,status,actor,source,now]).map_err(|e|e.to_string())?;
        crate::ledger::append_event_outcome(&tx, review)?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Export resolved synthetic decision evidence; no row is asserted to be training-ready.
    pub fn export_event_outcomes_jsonl(&mut self) -> Result<String, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let output = {
            let mut stmt=tx.prepare("SELECT r.id,r.candidate_id,r.replay_id,r.observation_id,r.actor,r.action,o.status,o.actor,o.source,o.created_unix_ns FROM event_reviews r JOIN event_review_outcomes o ON o.review_id=r.id ORDER BY r.id").map_err(|e|e.to_string())?;
            let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
            let mut output = String::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: i64 = row.get(0).map_err(|e| e.to_string())?;
                let candidate: String = row.get(1).map_err(|e| e.to_string())?;
                let replay: i64 = row.get(2).map_err(|e| e.to_string())?;
                let observation: i64 = row.get(3).map_err(|e| e.to_string())?;
                let review_actor: String = row.get(4).map_err(|e| e.to_string())?;
                let decision: String = row.get(5).map_err(|e| e.to_string())?;
                let status: String = row.get(6).map_err(|e| e.to_string())?;
                let outcome_actor: String = row.get(7).map_err(|e| e.to_string())?;
                let source: String = row.get(8).map_err(|e| e.to_string())?;
                let time: String = row.get(9).map_err(|e| e.to_string())?;
                if outcome_actor.trim().is_empty()
                    || source.trim().is_empty()
                    || !["validated", "falsified"].contains(&status.as_str())
                {
                    return Err("ineligible outcome provenance".into());
                }
                let evidence = Self::verify_event_replay_in_verified_snapshot(&tx, replay)?;
                if evidence.candidate_id != candidate {
                    return Err("invalid candidate provenance".into());
                }
                let worker_hash: Option<String> = tx
                    .query_row(
                        "SELECT worker_sha256 FROM event_replay_runs WHERE id=?1",
                        [replay],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let observation_worker_hash: Option<String> = tx
                    .query_row(
                        "SELECT worker_sha256 FROM event_observations WHERE id=?1",
                        [observation],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let domain: Option<String> = tx
                    .query_row(
                        "SELECT domain FROM event_observations WHERE id=?1",
                        [observation],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let followups = {
                    let mut receipt_stmt=tx.prepare("SELECT id FROM event_followup_receipts WHERE review_id=?1 ORDER BY id LIMIT 1001").map_err(|e|e.to_string())?;
                    let mut receipt_rows = receipt_stmt.query([id]).map_err(|e| e.to_string())?;
                    let mut values = Vec::new();
                    while let Some(receipt) = receipt_rows.next().map_err(|e| e.to_string())? {
                        if values.len() == 1000 {
                            return Err("too many follow-up receipts for export".into());
                        }
                        let receipt_id: i64 = receipt.get(0).map_err(|e| e.to_string())?;
                        values.push(Self::inspect_synthetic_followup_receipt_conn(
                            &tx, receipt_id,
                        )?);
                    }
                    values
                };
                let v = serde_json::json!({"schema":"sese-synthetic-event-outcome-v1","review_id":id,"candidate_id":candidate,"replay_id":replay,"fixture_version":evidence.pack_version,"fixture_digest":evidence.pack_digest,"state_semantics":evidence.state_semantics,"replay_worker_sha256":worker_hash,"observation_worker_sha256":observation_worker_hash,"observation_id":observation,"domain":domain,"review_actor":review_actor,"decision":decision,"outcome":status,"outcome_actor":outcome_actor,"outcome_source":source,"outcome_unix_ns":time,"outcome_evidence":"caller_asserted","synthetic_followup_receipts":followups,"synthetic":true,"training_ready":false});
                output.push_str(&v.to_string());
                output.push('\n');
            }
            output
        };
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }
    /// Records a local synthetic fixture selection, not external authority or fixture quality.
    pub fn pin_event_fixture(
        &mut self,
        domain: &str,
        replay: i64,
        actor: &str,
    ) -> Result<(), String> {
        if [domain, actor]
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 256)
        {
            return Err("invalid event fixture pin".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let evidence = Self::verify_event_replay_conn(&tx, replay)?;
        if !evidence.passed {
            return Err("cannot pin a failed replay".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO event_fixture_pins(domain,replay_id,pack_version,pack_digest,actor,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6)",params![domain,replay,evidence.pack_version,evidence.pack_digest,actor,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_fixture_pin(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn event_replay_matches_pin(&mut self, domain: &str, replay: i64) -> Result<bool, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let evidence = Self::verify_event_replay_conn(&tx, replay)?;
        let (version, digest): (String, String) = tx
            .query_row(
                "SELECT pack_version,pack_digest FROM event_fixture_pins WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let matches = (version, digest) == (evidence.pack_version, evidence.pack_digest);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(matches)
    }
    /// Metadata only: this experimental selection does not authorize untrusted execution.
    pub fn bootstrap_event_champion(
        &mut self,
        domain: &str,
        champion: &str,
        actor: &str,
    ) -> Result<(), String> {
        if [domain, actor]
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 256)
        {
            return Err("invalid event champion".into());
        }
        self.artifact_for_abi(champion, 3)?;
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let pin: Option<i64> = tx
            .query_row(
                "SELECT replay_id FROM event_fixture_pins WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let pinned_replay = pin.ok_or("fixture pin missing")?;
        if !Self::verify_event_replay_in_verified_snapshot(&tx, pinned_replay)?.passed {
            return Err("pinned replay failed".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute(
            "INSERT INTO event_champions VALUES (?1,?2,0)",
            params![domain, champion],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO event_champion_transitions(domain,previous_id,next_id,reference,actor,created_unix_ns,kind) VALUES (?1,'',?2,0,?3,?4,'bootstrap')",params![domain,champion,actor,now]).map_err(|e|e.to_string())?;
        crate::ledger::append_event_champion_transition(&tx, tx.last_insert_rowid())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn current_event_champion(&mut self, domain: &str) -> Result<String, String> {
        Ok(self.event_champion_snapshot(domain)?.0)
    }
    /// Return the state semantics selected by the domain's pinned fixture.
    pub fn event_domain_semantics(&mut self, domain: &str) -> Result<String, String> {
        Ok(self.event_champion_snapshot(domain)?.2)
    }
    pub(crate) fn event_champion_snapshot(
        &mut self,
        domain: &str,
    ) -> Result<(String, i64, String, Option<String>), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let state = tx
            .query_row(
                "SELECT c.champion_id,c.generation,r.state_semantics,r.worker_sha256 FROM event_champions c JOIN event_fixture_pins p ON p.domain=c.domain JOIN event_replay_runs r ON r.id=p.replay_id WHERE c.domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(state)
    }
    pub fn promote_event(
        &mut self,
        domain: &str,
        expected: &str,
        review: i64,
        actor: &str,
    ) -> Result<(), String> {
        if actor.trim().is_empty() || actor.len() > 256 {
            return Err("invalid actor".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let current: String = tx
            .query_row(
                "SELECT champion_id FROM event_champions WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current != expected {
            return Err("stale champion".into());
        }
        let (candidate,replay,observation):(String,i64,i64)=tx.query_row("SELECT candidate_id,replay_id,observation_id FROM event_reviews WHERE id=?1 AND action='approve'",[review],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let synthetic: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM synthetic_event_candidates WHERE candidate_id=?1)",
                [&candidate],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !synthetic {
            return Err("candidate not authorized for synthetic execution".into());
        }
        if candidate == current {
            return Err("candidate already champion".into());
        }
        let evidence = Self::verify_event_replay_conn(&tx, replay)?;
        // Older databases may contain a selection made before failed follow-ups
        // were rejected. Recheck it at promotion rather than trusting its age.
        let followup: Option<(i64, i64)> = tx
            .query_row(
                "SELECT id,replay_id FROM event_followup_pins WHERE domain=?1 AND candidate_id=?2",
                params![domain, candidate],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((followup_id, followup_replay)) = followup {
            if !Self::verify_event_replay_conn(&tx, followup_replay)?.passed {
                return Err("follow-up replay failed".into());
            }
            let (selection_seq, review_seq): (i64, i64) = tx.query_row(
                "SELECT p.seq,r.seq FROM ledger_events p JOIN ledger_events r ON r.kind='event_review' AND r.reference=?2 WHERE p.kind='event_followup_pin' AND p.reference=?1",
                params![followup_id, review], |r| Ok((r.get(0)?, r.get(1)?)),
            ).map_err(|e| e.to_string())?;
            if selection_seq >= review_seq {
                return Err("follow-up selection must precede approval".into());
            }
        }
        let pin: (String, String) = tx
            .query_row(
                "SELECT pack_version,pack_digest FROM event_fixture_pins WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let domain_semantics:String=tx.query_row("SELECT r.state_semantics FROM event_fixture_pins p JOIN event_replay_runs r ON r.id=p.replay_id WHERE p.domain=?1",[domain],|r|r.get(0)).map_err(|e|e.to_string())?;
        if evidence.state_semantics != domain_semantics {
            return Err("fixture semantics mismatch".into());
        }
        if !evidence.passed
            || evidence.candidate_id != candidate
            || (evidence.pack_version, evidence.pack_digest) != pin
        {
            return Err("failed or mismatched fixture".into());
        }
        let (replay_worker,observation_worker):(Option<String>,Option<String>)=tx.query_row("SELECT r.worker_sha256,o.worker_sha256 FROM event_replay_runs r JOIN event_observations o ON o.id=?2 WHERE r.id=?1",params![replay,observation],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        if replay_worker.is_none() || replay_worker != observation_worker {
            return Err("worker provenance missing or mismatched".into());
        }
        let fixture_worker:Option<String>=tx.query_row("SELECT r.worker_sha256 FROM event_fixture_pins p JOIN event_replay_runs r ON r.id=p.replay_id WHERE p.domain=?1",[domain],|r|r.get(0)).map_err(|e|e.to_string())?;
        if fixture_worker.is_none() || fixture_worker != replay_worker {
            return Err("worker differs from fixture selection".into());
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM event_observations WHERE id=?1 AND champion_id=?2 AND shadow_id=?3 AND domain=?6 AND champion_json IS NOT NULL AND champion_error IS NULL AND shadow_json IS NOT NULL AND shadow_error IS NULL) AND ?1=(SELECT MAX(id) FROM event_observations WHERE shadow_id=?3 AND domain=?6) AND ?4=(SELECT MAX(id) FROM event_replay_runs WHERE candidate_id=?3) AND ?5=(SELECT MAX(r.id) FROM event_reviews r JOIN event_observations o ON o.id=r.observation_id WHERE r.candidate_id=?3 AND o.domain=?6)",params![observation,current,candidate,replay,review,domain],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !valid {
            return Err("missing or stale review evidence".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let changed=tx.execute("UPDATE event_champions SET champion_id=?1,generation=generation+1 WHERE domain=?2 AND champion_id=?3 AND generation<9223372036854775807",params![candidate,domain,current]).map_err(|e|e.to_string())?;
        if changed != 1 {
            return Err("stale champion".into());
        }
        tx.execute("INSERT INTO event_champion_transitions(domain,previous_id,next_id,reference,actor,created_unix_ns,kind) VALUES (?1,?2,?3,?4,?5,?6,'promote')",params![domain,current,candidate,review,actor,now]).map_err(|e|e.to_string())?;
        crate::ledger::append_event_champion_transition(&tx, tx.last_insert_rowid())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn rollback_event(
        &mut self,
        domain: &str,
        expected: &str,
        actor: &str,
    ) -> Result<(), String> {
        if actor.trim().is_empty() || actor.len() > 256 {
            return Err("invalid actor".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let current: String = tx
            .query_row(
                "SELECT champion_id FROM event_champions WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current != expected {
            return Err("stale champion".into());
        }
        let (previous_id,previous,kind):(i64,String,String)=tx.query_row("SELECT id,previous_id,kind FROM event_champion_transitions WHERE domain=?1 ORDER BY id DESC LIMIT 1",[domain],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        if kind != "promote" || previous.is_empty() {
            return Err("no promotion to roll back".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let changed=tx.execute("UPDATE event_champions SET champion_id=?1,generation=generation+1 WHERE domain=?2 AND champion_id=?3 AND generation<9223372036854775807",params![previous,domain,current]).map_err(|e|e.to_string())?;
        if changed != 1 {
            return Err("stale champion".into());
        }
        tx.execute("INSERT INTO event_champion_transitions(domain,previous_id,next_id,reference,actor,created_unix_ns,kind) VALUES (?1,?2,?3,?4,?5,?6,'rollback')",params![domain,current,previous,previous_id,actor,now]).map_err(|e|e.to_string())?;
        crate::ledger::append_event_champion_transition(&tx, tx.last_insert_rowid())?;
        tx.commit().map_err(|e| e.to_string())
    }
    // Empty scope denotes explicit unscoped processing. No prior observations yields None.
    fn stream_scope_conn(
        conn: &Connection,
        kind: &str,
        name: &str,
    ) -> Result<Option<String>, String> {
        let sql=match kind {"legacy"=>"SELECT o.domain FROM event_stream_receipts r JOIN event_observations o ON o.id=r.observation_id WHERE r.name=?1 ORDER BY r.generation", "measured"=>"SELECT o.domain FROM measured_stream_receipts r JOIN event_observations o ON o.id=r.observation_id WHERE r.name=?1 ORDER BY r.generation",_=>return Err("invalid stream kind".into())};
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let mut rows = stmt.query([name]).map_err(|e| e.to_string())?;
        let mut scope: Option<String> = None;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let domain: Option<String> = row.get(0).map_err(|e| e.to_string())?;
            let value = domain.unwrap_or_default();
            if scope.as_ref().is_some_and(|s| s != &value) {
                return Err("mixed stream domains".into());
            }
            scope = Some(value);
        }
        Ok(scope)
    }
    pub(crate) fn load_event_stream(
        &mut self,
        name: &str,
        capacity: usize,
    ) -> Result<(i64, Vec<f64>, Option<String>), String> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO event_streams VALUES (?1,?2,0,x'')",
                params![name, capacity as i64],
            )
            .map_err(|e| e.to_string())?;
        if inserted == 1 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
                .to_string();
            tx.execute("INSERT INTO event_stream_receipts(name,capacity,generation,prior,next,observation_id,created_unix_ns) VALUES (?1,?2,0,x'',x'',NULL,?3)",params![name,capacity as i64,now]).map_err(|e|e.to_string())?;
            crate::ledger::append_event_stream_receipt(&tx, tx.last_insert_rowid())?;
        } else {
            let linked: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM event_stream_receipts WHERE name=?1)",
                    [name],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !linked {
                return Err("legacy event stream lacks ledger receipts".into());
            }
        }
        let (stored_capacity, generation, bytes): (i64, i64, Vec<u8>) = tx
            .query_row(
                "SELECT capacity,generation,latencies FROM event_streams WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if stored_capacity != capacity as i64
            || generation < 0
            || bytes.len() % 8 != 0
            || bytes.len() / 8 > capacity
        {
            return Err("invalid event stream".into());
        }
        let values: Vec<f64> = bytes
            .chunks_exact(8)
            .map(|b| f64::from_bits(u64::from_be_bytes(b.try_into().unwrap())))
            .collect();
        if values
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1_000_000_000_000.0).contains(v))
        {
            return Err("invalid event stream values".into());
        }
        let scope = Self::stream_scope_conn(&tx, "legacy", name)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((generation, values, scope))
    }
    pub(crate) fn load_measured_stream(
        &mut self,
        name: &str,
        capacity: usize,
    ) -> Result<(i64, crate::event_state::EventState, Option<String>), String> {
        if name.trim().is_empty() || name.len() > 256 {
            return Err("invalid measured stream name".into());
        }
        let empty = crate::event_state::EventState::new(capacity)?;
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO measured_streams VALUES (?1,?2,0,?3)",
                params![name, capacity as i64, empty.encode()],
            )
            .map_err(|e| e.to_string())?;
        if inserted == 1 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
                .to_string();
            tx.execute("INSERT INTO measured_stream_receipts(name,capacity,generation,prior,next,observation_id,created_unix_ns) VALUES (?1,?2,0,?3,?3,NULL,?4)",params![name,capacity as i64,empty.encode(),now]).map_err(|e|e.to_string())?;
            crate::ledger::append_measured_stream_receipt(&tx, tx.last_insert_rowid())?;
        }
        let (stored, generation, bytes): (i64, i64, Vec<u8>) = tx
            .query_row(
                "SELECT capacity,generation,state FROM measured_streams WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        let linked: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM measured_stream_receipts WHERE name=?1)",
                [name],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let state = crate::event_state::EventState::decode(&bytes)?;
        if !linked || stored != capacity as i64 || state.capacity() != capacity || generation < 0 {
            return Err("invalid measured stream".into());
        }
        let scope = Self::stream_scope_conn(&tx, "measured", name)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((generation, state, scope))
    }
    pub(crate) fn save_event_observation(
        &mut self,
        champion: &str,
        shadow: &str,
        prior: &str,
        event: &str,
        c: &Option<String>,
        ce: &Option<String>,
        s: &Option<String>,
        se: &Option<String>,
        cu: i64,
        su: i64,
        stream: Option<(&str, i64, usize, &[f64], &[f64])>,
        champion_guard: Option<(&str, &str, i64, &str)>,
        measured_stream: Option<(&str, i64, &[u8], &[u8])>,
        worker: &crate::worker_identity::WorkerIdentity,
    ) -> Result<i64, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if let Some((domain, expected, generation, selected_worker)) = champion_guard {
            if !crate::ledger::verify_storage(&tx)? {
                return Err("ledger integrity failure".into());
            }
            let matches:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM event_champions WHERE domain=?1 AND champion_id=?2 AND generation=?3)",params![domain,expected,generation],|r|r.get(0)).map_err(|e|e.to_string())?;
            let pinned:Option<String>=tx.query_row("SELECT r.worker_sha256 FROM event_fixture_pins p JOIN event_replay_runs r ON r.id=p.replay_id WHERE p.domain=?1",[domain],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !matches
                || pinned.as_deref() != Some(selected_worker)
                || selected_worker != worker.sha256
            {
                return Err("stale champion or worker selection".into());
            }
        }
        let desired = champion_guard.map(|(domain, _, _, _)| domain).unwrap_or("");
        if let Some((name, _, _, _, _)) = stream {
            if Self::stream_scope_conn(&tx, "legacy", name)?
                .as_deref()
                .is_some_and(|s| s != desired)
            {
                return Err("event stream domain mismatch".into());
            }
        }
        if let Some((name, _, _, _)) = measured_stream {
            if Self::stream_scope_conn(&tx, "measured", name)?
                .as_deref()
                .is_some_and(|s| s != desired)
            {
                return Err("measured stream domain mismatch".into());
            }
        }
        tx.execute("INSERT INTO event_observations(champion_id,shadow_id,prior_json,event_json,champion_json,champion_error,shadow_json,shadow_error,champion_us,shadow_us,observed_unix_ns,domain) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![champion,shadow,prior,event,c,ce,s,se,cu,su,now,champion_guard.map(|(domain,_,_,_)|domain)]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_event_observation(&tx, id, &now)?;
        tx.execute(
            "UPDATE event_observations SET worker_path=?1,worker_sha256=?2 WHERE id=?3",
            params![worker.path, worker.sha256, id],
        )
        .map_err(|e| e.to_string())?;
        crate::ledger::append_event_worker_provenance(&tx, "event_observation_worker", id, &now)?;
        if let Some((name, generation, capacity, old, next)) = stream {
            let encode = |values: &[f64]| {
                values
                    .iter()
                    .flat_map(|v| v.to_bits().to_be_bytes())
                    .collect::<Vec<u8>>()
            };
            let changed=tx.execute("UPDATE event_streams SET latencies=?1,generation=generation+1 WHERE name=?2 AND generation=?3 AND capacity=?4 AND latencies=?5 AND generation < 9223372036854775807",params![encode(next),name,generation,capacity as i64,encode(old)]).map_err(|e|e.to_string())?;
            if changed != 1 {
                return Err("stale event stream".into());
            }
            tx.execute("INSERT INTO event_stream_receipts(name,capacity,generation,prior,next,observation_id,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![name,capacity as i64,generation+1,encode(old),encode(next),id,now]).map_err(|e|e.to_string())?;
            crate::ledger::append_event_stream_receipt(&tx, tx.last_insert_rowid())?;
        }

        if let Some((name, generation, old, next)) = measured_stream {
            let changed=tx.execute("UPDATE measured_streams SET state=?1,generation=generation+1 WHERE name=?2 AND generation=?3 AND state=?4 AND generation<9223372036854775807",params![next,name,generation,old]).map_err(|e|e.to_string())?;
            if changed != 1 {
                return Err("stale measured stream".into());
            }
            let capacity = crate::event_state::EventState::decode(old)?.capacity();
            tx.execute("INSERT INTO measured_stream_receipts(name,capacity,generation,prior,next,observation_id,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![name,capacity as i64,generation+1,old,next,id,now]).map_err(|e|e.to_string())?;
            crate::ledger::append_measured_stream_receipt(&tx, tx.last_insert_rowid())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Checks the stored observation against its ledger link; this does not authenticate the chain.
    pub fn verify_event_observation(&mut self, id: i64) -> Result<bool, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let valid = crate::ledger::verify_storage(&tx)?
            && crate::ledger::event_observation_binding_matches(&tx, id).unwrap_or(false);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(valid)
    }
    pub(crate) fn artifact_for_abi(&self, id: &str, abi: i64) -> Result<Vec<u8>, String> {
        let (bytes, runtime, actual): (Vec<u8>, String, i64) = self
            .conn
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if digest(&bytes) != id || runtime != "wasmi-experimental" || actual != abi {
            return Err("artifact identity or ABI mismatch".into());
        }
        if abi == 1 {
            let synthetic:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM synthetic_scalar_candidates WHERE candidate_id=?1)",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !synthetic || !crate::ledger::verify_storage(&self.conn)? {
                return Err("candidate not authorized for synthetic execution".into());
            }
        }
        if abi == 2 {
            let synthetic:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM synthetic_window_candidates WHERE candidate_id=?1)",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !synthetic || !crate::ledger::verify_storage(&self.conn)? {
                return Err("candidate not authorized for synthetic execution".into());
            }
        }
        if abi == 3 {
            let synthetic: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM synthetic_event_candidates WHERE candidate_id=?1)",
                    [id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !synthetic || !crate::ledger::verify_storage(&self.conn)? {
                return Err("candidate not authorized for synthetic execution".into());
            }
        }
        Ok(bytes)
    }
    pub(crate) fn save_window_observation(
        &self,
        champion: &str,
        shadow: &str,
        window: &[i32],
        input: i32,
        o: &crate::shadow::Observation,
    ) -> Result<i64, String> {
        let bytes: Vec<u8> = window.iter().flat_map(|v| v.to_le_bytes()).collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        self.conn.execute("INSERT INTO window_observations(champion_id,shadow_id,prior_window,input,champion_result,champion_error,shadow_result,shadow_error,champion_us,shadow_us,observed_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![champion,shadow,bytes,input,o.champion,o.champion_error,o.shadow,o.shadow_error,o.champion_us,o.shadow_us,now]).map_err(|e|e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn artifact(&self, id: &str) -> Result<Vec<u8>, String> {
        let bytes: Vec<u8> = self
            .conn
            .query_row("SELECT artifact FROM candidates WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("candidate not found")?;
        if digest(&bytes) != id {
            return Err("candidate integrity mismatch".into());
        }
        Ok(bytes)
    }
    pub(crate) fn save_replay(
        &mut self,
        candidate: &str,
        version: &str,
        pack_digest: &str,
        started_at: &str,
        cases: &[crate::replay::CaseResult],
    ) -> Result<i64, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let passed = !cases.is_empty() && cases.iter().all(|c| c.passed);
        tx.execute("INSERT INTO replay_runs(candidate_id,pack_version,pack_digest,passed,started_at_unix_ns,environment) VALUES (?1,?2,?3,?4,?5,'runtime unverified; scalar ABI v1; synthetic')", params![candidate, version, pack_digest, passed, started_at]).map_err(|e| e.to_string())?;
        let id = tx.last_insert_rowid();
        for c in cases {
            tx.execute(
                "INSERT INTO replay_cases VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    id,
                    c.case.id,
                    c.case.input,
                    c.case.expected,
                    c.actual,
                    c.error,
                    c.passed
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn replay_case_count(&self, run_id: i64) -> Result<i64, String> {
        self.conn
            .query_row(
                "SELECT count(*) FROM replay_cases WHERE run_id=?1",
                [run_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub(crate) fn save_shadow(
        &self,
        champion: &str,
        shadow: &str,
        input: i32,
        observation: &crate::shadow::Observation,
        timestamp: &str,
    ) -> Result<i64, String> {
        self.conn.execute("INSERT INTO shadow_observations(champion_id,shadow_id,input,champion_result,champion_error,shadow_result,shadow_error,champion_us,shadow_us,observed_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![champion,shadow,input,observation.champion,observation.champion_error,observation.shadow,observation.shadow_error,observation.champion_us,observation.shadow_us,timestamp]).map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }
    /// Experimental structured review; not a promotion authorization.
    pub fn record_review(
        &mut self,
        candidate: &str,
        replay: i64,
        shadow: i64,
        actor: &str,
        action: &str,
    ) -> Result<i64, String> {
        if actor.trim().is_empty() || !["approve", "reject"].contains(&action) {
            return Err("invalid review".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = tx
            .query_row(
                "SELECT artifact FROM candidates WHERE id=?1",
                [candidate],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if digest(&bytes) != candidate {
            return Err("candidate integrity mismatch".into());
        }
        if !crate::ledger::verify_storage(&tx)? {
            return Err("review ledger integrity failure".into());
        }
        let synthetic: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM synthetic_scalar_candidates WHERE candidate_id=?1)",
                [candidate],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !synthetic {
            return Err("candidate not authorized for synthetic execution".into());
        }
        let matches: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM replay_runs r JOIN shadow_observations s ON s.id=?3 WHERE r.id=?2 AND r.candidate_id=?1 AND s.shadow_id=?1)", params![candidate,replay,shadow], |r| r.get(0)).map_err(|e| e.to_string())?;
        if !matches {
            return Err("evidence does not match candidate".into());
        }
        if action == "approve" {
            let eligible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM replay_runs r JOIN shadow_observations s ON s.id=?2 WHERE r.id=?1 AND r.passed=1 AND EXISTS(SELECT 1 FROM replay_cases c WHERE c.run_id=r.id) AND NOT EXISTS(SELECT 1 FROM replay_cases c WHERE c.run_id=r.id AND (c.passed<>1 OR c.error IS NOT NULL OR c.actual IS NULL OR c.actual<>c.expected)) AND s.champion_result IS NOT NULL AND s.shadow_result IS NOT NULL AND s.champion_error IS NULL AND s.shadow_error IS NULL)", params![replay,shadow], |r| r.get(0)).map_err(|e| e.to_string())?;
            if !eligible {
                return Err("evidence is incomplete or failed".into());
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO review_records(candidate_id,replay_id,shadow_id,actor,action,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6)", params![candidate,replay,shadow,actor,action,now]).map_err(|e| e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_review(&tx, id, candidate, replay, shadow, actor, action, &now)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn verify_ledger(&self) -> Result<bool, String> {
        crate::ledger::verify_storage(&self.conn)
    }
    pub fn outcome_status(&self, review: i64) -> Result<String, String> {
        self.conn.query_row("SELECT COALESCE(o.status,'pending') FROM review_records r LEFT JOIN review_outcomes o ON o.review_id=r.id WHERE r.id=?1", [review], |r| r.get(0)).map_err(|e| e.to_string())
    }
    pub fn resolve_review(
        &mut self,
        review: i64,
        status: &str,
        actor: &str,
        source: &str,
    ) -> Result<(), String> {
        if !["validated", "falsified"].contains(&status)
            || actor.trim().is_empty()
            || source.trim().is_empty()
        {
            return Err("invalid outcome".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO review_outcomes(review_id,status,actor,source,created_unix_ns) VALUES (?1,?2,?3,?4,?5)", params![review,status,actor,source,now]).map_err(|e| e.to_string())?;
        crate::ledger::append_outcome(&tx, review, status, actor, source, &now)?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn window_outcome_status(&self, review: i64) -> Result<String, String> {
        self.conn.query_row("SELECT COALESCE(o.status,'pending') FROM window_review_records r LEFT JOIN window_review_outcomes o ON o.review_id=r.id WHERE r.id=?1", [review], |r| r.get(0)).map_err(|e| e.to_string())
    }
    pub fn resolve_window_review(
        &mut self,
        review: i64,
        status: &str,
        actor: &str,
        source: &str,
    ) -> Result<(), String> {
        if !["validated", "falsified"].contains(&status)
            || actor.trim().is_empty()
            || source.trim().is_empty()
        {
            return Err("invalid outcome".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO window_review_outcomes(review_id,status,actor,source,created_unix_ns) VALUES (?1,?2,?3,?4,?5)", params![review,status,actor,source,now]).map_err(|e| e.to_string())?;
        crate::ledger::append_window_outcome(&tx, review, status, actor, source, &now)?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Export synthetic outcome evidence, not a complete training dataset.
    pub fn export_rlvr_jsonl(&mut self) -> Result<String, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let output = {
            let mut stmt = tx.prepare("SELECT r.id,r.candidate_id,r.replay_id,r.shadow_id,r.actor,r.action,o.status,o.actor,o.source,o.created_unix_ns,c.artifact FROM review_records r JOIN review_outcomes o ON o.review_id=r.id JOIN candidates c ON c.id=r.candidate_id ORDER BY r.id").map_err(|e| e.to_string())?;
            let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
            let mut output = String::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: i64 = row.get(0).map_err(|e| e.to_string())?;
                let candidate: String = row.get(1).map_err(|e| e.to_string())?;
                let replay: i64 = row.get(2).map_err(|e| e.to_string())?;
                let shadow: i64 = row.get(3).map_err(|e| e.to_string())?;
                let review_actor: String = row.get(4).map_err(|e| e.to_string())?;
                let action: String = row.get(5).map_err(|e| e.to_string())?;
                let status: String = row.get(6).map_err(|e| e.to_string())?;
                let outcome_actor: String = row.get(7).map_err(|e| e.to_string())?;
                let source: String = row.get(8).map_err(|e| e.to_string())?;
                let time: String = row.get(9).map_err(|e| e.to_string())?;
                let artifact: Vec<u8> = row.get(10).map_err(|e| e.to_string())?;
                if digest(&artifact) != candidate
                    || source.trim().is_empty()
                    || outcome_actor.trim().is_empty()
                {
                    return Err("ineligible or corrupt provenance".into());
                }
                let reward = match status.as_str() {
                    "validated" => 1,
                    "falsified" => -1,
                    _ => return Err("invalid outcome".into()),
                };
                let (fixture_version, fixture_digest, environment): (String, String, String) = tx.query_row("SELECT pack_version,pack_digest,environment FROM replay_runs WHERE id=?1 AND candidate_id=?2", params![replay,candidate], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e| e.to_string())?;
                let value = serde_json::json!({"schema":"sese-synthetic-outcome-v1","review_id":id,"candidate_id":candidate,"replay_id":replay,"fixture_version":fixture_version,"fixture_digest":fixture_digest,"replay_environment":environment,"runtime_provenance_verified":false,"shadow_id":shadow,"review_actor":review_actor,"decision":action,"outcome":status,"outcome_actor":outcome_actor,"outcome_source":source,"outcome_unix_ns":time,"outcome_evidence":"caller_asserted","reward":reward,"reward_derivation":"validated=1;falsified=-1","synthetic":true,"training_ready":false});
                output.push_str(&serde_json::to_string(&value).map_err(|e| e.to_string())?);
                output.push('\n');
            }
            output
        };
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }
    /// Export synthetic outcome evidence, not a complete training dataset.
    pub fn export_window_rlvr_jsonl(&mut self) -> Result<String, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let output = {
            let mut stmt = tx.prepare("SELECT r.id,r.candidate_id,r.replay_id,r.shadow_id,r.actor,r.action,o.status,o.actor,o.source,o.created_unix_ns,c.artifact FROM window_review_records r JOIN window_review_outcomes o ON o.review_id=r.id JOIN candidates c ON c.id=r.candidate_id ORDER BY r.id").map_err(|e| e.to_string())?;
            let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
            let mut output = String::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: i64 = row.get(0).map_err(|e| e.to_string())?;
                let candidate: String = row.get(1).map_err(|e| e.to_string())?;
                let replay: i64 = row.get(2).map_err(|e| e.to_string())?;
                let shadow: i64 = row.get(3).map_err(|e| e.to_string())?;
                let review_actor: String = row.get(4).map_err(|e| e.to_string())?;
                let action: String = row.get(5).map_err(|e| e.to_string())?;
                let status: String = row.get(6).map_err(|e| e.to_string())?;
                let outcome_actor: String = row.get(7).map_err(|e| e.to_string())?;
                let source: String = row.get(8).map_err(|e| e.to_string())?;
                let time: String = row.get(9).map_err(|e| e.to_string())?;
                let artifact: Vec<u8> = row.get(10).map_err(|e| e.to_string())?;
                if digest(&artifact) != candidate
                    || source.trim().is_empty()
                    || outcome_actor.trim().is_empty()
                {
                    return Err("ineligible or corrupt provenance".into());
                }
                let reward = match status.as_str() {
                    "validated" => 1,
                    "falsified" => -1,
                    _ => return Err("invalid outcome".into()),
                };
                let (fixture_version, fixture_digest, environment): (String, String, String) = tx.query_row("SELECT pack_version,pack_digest,environment FROM window_replay_runs WHERE id=?1 AND candidate_id=?2", params![replay,candidate], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e| e.to_string())?;
                let value = serde_json::json!({"schema":"sese-synthetic-window-outcome-v1","review_id":id,"candidate_id":candidate,"replay_id":replay,"fixture_version":fixture_version,"fixture_digest":fixture_digest,"replay_environment":environment,"runtime_provenance_verified":false,"shadow_id":shadow,"review_actor":review_actor,"decision":action,"outcome":status,"outcome_actor":outcome_actor,"outcome_source":source,"outcome_unix_ns":time,"outcome_evidence":"caller_asserted","reward":reward,"reward_derivation":"validated=1;falsified=-1","synthetic":true,"training_ready":false});
                output.push_str(&serde_json::to_string(&value).map_err(|e| e.to_string())?);
                output.push('\n');
            }
            output
        };
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }
    pub(crate) fn save_window_replay(
        &mut self,
        candidate: &str,
        version: &str,
        digest: &str,
        started: &str,
        cases: &[crate::replay::WindowCaseResult],
    ) -> Result<i64, String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let passed = !cases.is_empty() && cases.iter().all(|c| c.passed);
        tx.execute("INSERT INTO window_replay_runs(candidate_id,pack_version,pack_digest,passed,started_unix_ns,environment) VALUES (?1,?2,?3,?4,?5,'runtime unverified; window ABI v1; synthetic')",params![candidate,version,digest,passed,started]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        for (ordinal, result) in cases.iter().enumerate() {
            let window: Vec<u8> = result
                .case
                .window
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            tx.execute(
                "INSERT INTO window_replay_cases(run_id,case_id,prior_window,input,expected,actual,error,passed,ordinal) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    id,
                    result.case.id,
                    window,
                    result.case.input,
                    result.case.expected,
                    result.actual,
                    result.error,
                    result.passed,
                    ordinal as i64
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Synthetic review only; this does not promote a candidate.
    pub fn record_window_review(
        &mut self,
        candidate: &str,
        replay: i64,
        shadow: i64,
        actor: &str,
        action: &str,
    ) -> Result<i64, String> {
        self.record_window_review_inner(candidate, replay, shadow, actor, action, None)
    }
    pub fn record_window_review_with_card(
        &mut self,
        candidate: &str,
        replay: i64,
        shadow: i64,
        actor: &str,
        action: &str,
        human: &str,
        bridge: &str,
        proposal: &str,
    ) -> Result<i64, String> {
        if [human, bridge, proposal]
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 16_384)
        {
            return Err("invalid review card".into());
        }
        self.record_window_review_inner(
            candidate,
            replay,
            shadow,
            actor,
            action,
            Some((human, bridge, proposal)),
        )
    }
    fn record_window_review_inner(
        &mut self,
        candidate: &str,
        replay: i64,
        shadow: i64,
        actor: &str,
        action: &str,
        card: Option<(&str, &str, &str)>,
    ) -> Result<i64, String> {
        if actor.trim().is_empty() || !["approve", "reject"].contains(&action) {
            return Err("invalid review".into());
        }
        self.artifact_for_abi(candidate, 2)?;
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let matches:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM window_replay_runs r JOIN window_observations s ON s.id=?3 WHERE r.id=?2 AND r.candidate_id=?1 AND s.shadow_id=?1)",params![candidate,replay,shadow],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !matches {
            return Err("evidence does not match candidate".into());
        }
        if action == "approve" {
            let eligible:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM window_replay_runs r JOIN window_observations s ON s.id=?2 WHERE r.id=?1 AND r.passed=1 AND EXISTS(SELECT 1 FROM window_replay_cases c WHERE c.run_id=r.id) AND NOT EXISTS(SELECT 1 FROM window_replay_cases c WHERE c.run_id=r.id AND (c.passed<>1 OR c.error IS NOT NULL OR c.actual IS NULL OR c.actual<>c.expected)) AND s.champion_result IS NOT NULL AND s.shadow_result IS NOT NULL AND s.champion_error IS NULL AND s.shadow_error IS NULL)",params![replay,shadow],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !eligible {
                return Err("evidence is incomplete or failed".into());
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute("INSERT INTO window_review_records(candidate_id,replay_id,shadow_id,actor,action,created_unix_ns) VALUES (?1,?2,?3,?4,?5,?6)",params![candidate,replay,shadow,actor,action,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_window_review(
            &tx,
            id,
            candidate,
            replay,
            shadow,
            actor,
            action,
            &now,
            card.is_some(),
        )?;
        if let Some((human, bridge, proposal)) = card {
            tx.execute(
                "INSERT INTO window_review_cards VALUES (?1,?2,?3,?4,?5)",
                params![id, human, bridge, proposal, now],
            )
            .map_err(|e| e.to_string())?;
            crate::ledger::append_window_card(&tx, id, human, bridge, proposal, &now)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// Inspect synthetic window evidence from a single verified database snapshot.
    pub fn inspect_window_review(&mut self, id: i64) -> Result<serde_json::Value, String> {
        fn window(bytes: Vec<u8>) -> Result<Vec<i32>, String> {
            if bytes.len() % 4 != 0 || bytes.len() > 16_384 {
                return Err("invalid window encoding".into());
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]))
                .collect())
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (candidate,replay,shadow,actor,action,created,outcome):(String,i64,i64,String,String,String,String)=tx.query_row("SELECT r.candidate_id,r.replay_id,r.shadow_id,r.actor,r.action,r.created_unix_ns,COALESCE(o.status,'pending') FROM window_review_records r LEFT JOIN window_review_outcomes o ON o.review_id=r.id WHERE r.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        let (version, digest, passed): (String, String, bool) = tx
            .query_row(
                "SELECT pack_version,pack_digest,passed FROM window_replay_runs WHERE id=?1",
                [replay],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        let (champ,prior,input,champ_result,champ_error,shadow_result,shadow_error):(String,Vec<u8>,i32,Option<i32>,Option<String>,Option<i32>,Option<String>)=tx.query_row("SELECT champion_id,prior_window,input,champion_result,champion_error,shadow_result,shadow_error FROM window_observations WHERE id=?1",[shadow],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
        let mut cases = Vec::new();
        {
            let mut stmt=tx.prepare("SELECT case_id,prior_window,input,expected,actual,error,passed FROM window_replay_cases WHERE run_id=?1 ORDER BY case_id COLLATE BINARY").map_err(|e|e.to_string())?;
            let mut rows = stmt.query([replay]).map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let case_id: String = row.get(0).map_err(|e| e.to_string())?;
                let bytes: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
                let input: i32 = row.get(2).map_err(|e| e.to_string())?;
                let expected: i32 = row.get(3).map_err(|e| e.to_string())?;
                let actual: Option<i32> = row.get(4).map_err(|e| e.to_string())?;
                let error: Option<String> = row.get(5).map_err(|e| e.to_string())?;
                let passed: bool = row.get(6).map_err(|e| e.to_string())?;
                cases.push(serde_json::json!({"id":case_id,"prior_window":window(bytes)?,"input":input,"expected":expected,"actual":actual,"error":error,"passed":passed}));
            }
        }
        let outcome_detail:Option<(String,String,String)>=tx.query_row("SELECT actor,source,created_unix_ns FROM window_review_outcomes WHERE review_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|e|e.to_string())?;
        let outcome_detail=outcome_detail.map(|(actor,source,created)|serde_json::json!({"actor":actor,"source":source,"created_unix_ns":created}));
        let card: Option<(String, String, String)> = tx
            .query_row(
                "SELECT human,bridge,proposal FROM window_review_cards WHERE review_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let card=card.map(|(human,bridge,proposal)|serde_json::json!({"human":human,"bridge":bridge,"proposal":proposal}));
        let result = serde_json::json!({"schema":"sese-window-review-inspection-v1","synthetic":true,"review_id":id,"candidate_id":candidate,"actor":actor,"decision":action,"created_unix_ns":created,"outcome":outcome,"outcome_detail":outcome_detail,"card":card,"replay":{"id":replay,"pack_version":version,"pack_digest":digest,"passed":passed},"cases":cases,"shadow":{"id":shadow,"champion_id":champ,"prior_window":window(prior)?,"input":input,"champion_result":champ_result,"champion_error":champ_error,"shadow_result":shadow_result,"shadow_error":shadow_error}});
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
    pub(crate) fn load_window_stream(
        &mut self,
        name: &str,
        capacity: usize,
    ) -> Result<(i64, Vec<i32>), String> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT OR IGNORE INTO window_streams VALUES (?1,?2,0,x'')",
            params![name, capacity as i64],
        )
        .map_err(|e| e.to_string())?;
        let (stored, generation, bytes): (i64, i64, Vec<u8>) = tx
            .query_row(
                "SELECT capacity,generation,window FROM window_streams WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if stored != capacity as i64
            || generation < 0
            || bytes.len() % 4 != 0
            || bytes.len() > capacity * 4
        {
            return Err("invalid stored window or capacity mismatch".into());
        }
        let values = bytes
            .chunks_exact(4)
            .map(|x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]))
            .collect();
        tx.commit().map_err(|e| e.to_string())?;
        Ok((generation, values))
    }
    pub(crate) fn commit_window_stream(
        &mut self,
        name: &str,
        generation: i64,
        capacity: usize,
        prior: &[i32],
        next: &[i32],
        champion: &str,
        shadow: &str,
        input: i32,
        o: &crate::shadow::Observation,
        domain_guard: Option<(&str, &str, i64)>,
    ) -> Result<i64, String> {
        let old: Vec<u8> = prior.iter().flat_map(|v| v.to_le_bytes()).collect();
        let new: Vec<u8> = next.iter().flat_map(|v| v.to_le_bytes()).collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if let Some((domain, expected, generation)) = domain_guard {
            if !crate::ledger::verify_storage(&tx)? {
                return Err("ledger integrity failure".into());
            }
            let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM champion_domains WHERE domain=?1 AND champion_id=?2 AND generation=?3)",params![domain,expected,generation],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !valid {
                return Err("champion changed during execution".into());
            }
        }
        let changed=tx.execute("UPDATE window_streams SET window=?1,generation=generation+1 WHERE name=?2 AND generation=?3 AND capacity=?4 AND window=?5",params![new,name,generation,capacity as i64,old]).map_err(|e|e.to_string())?;
        if changed != 1 {
            return Err("stale window stream".into());
        }
        tx.execute("INSERT INTO window_observations(champion_id,shadow_id,prior_window,input,champion_result,champion_error,shadow_result,shadow_error,champion_us,shadow_us,observed_unix_ns) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![champion,shadow,old,input,o.champion,o.champion_error,o.shadow,o.shadow_error,o.champion_us,o.shadow_us,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    /// A checkpoint must be kept outside this database by the operator.
    pub fn ledger_checkpoint(&mut self) -> Result<(i64, String), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let result = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("cannot checkpoint empty ledger")?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
    pub fn verify_ledger_checkpoint(&mut self, seq: i64, hash: &str) -> Result<bool, String> {
        if seq <= 0
            || hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid checkpoint".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Ok(false);
        }
        let actual: Option<String> = tx
            .query_row("SELECT hash FROM ledger_events WHERE seq=?1", [seq], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        let matches = actual.as_deref() == Some(hash);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(matches)
    }
    /// Pins are local operator selections and do not establish source or fixture quality.
    pub fn pin_window_fixture(
        &mut self,
        domain: &str,
        version: &str,
        pack_digest: &str,
        actor: &str,
    ) -> Result<(), String> {
        if [domain, version, actor]
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256)
            || pack_digest.len() != 64
            || !pack_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid fixture pin".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        tx.execute("INSERT INTO window_fixture_pins(domain,pack_version,pack_digest,actor,created_unix_ns) VALUES (?1,?2,?3,?4,?5)",params![domain,version,pack_digest,actor,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_fixture_pin(&tx, id, domain, version, pack_digest, actor, &now)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn window_replay_identity(&self, run: i64) -> Result<(String, String), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let identity = Self::window_replay_identity_conn(&tx, run)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(identity)
    }
    fn window_replay_identity_conn(
        conn: &Connection,
        run: i64,
    ) -> Result<(String, String), String> {
        let (version, stored): (String, String) = conn
            .query_row(
                "SELECT pack_version,pack_digest FROM window_replay_runs WHERE id=?1",
                [run],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let mut stmt=conn.prepare("SELECT case_id,prior_window,input,expected,ordinal FROM window_replay_cases WHERE run_id=?1 ORDER BY ordinal").map_err(|e|e.to_string())?;
        let mut rows = stmt.query([run]).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        hash.update(b"sese-window-pack-v1\0");
        let mut count = 0usize;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: String = row.get(0).map_err(|e| e.to_string())?;
            let bytes: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
            let input: i32 = row.get(2).map_err(|e| e.to_string())?;
            let expected: i32 = row.get(3).map_err(|e| e.to_string())?;
            let ordinal: Option<i64> = row.get(4).map_err(|e| e.to_string())?;
            if ordinal != Some(count as i64) {
                return Err("invalid fixture ordering".into());
            }
            if id.trim().is_empty()
                || id.len() > 256
                || bytes.len() % 4 != 0
                || bytes.len() > 16_384
            {
                return Err("invalid stored fixture".into());
            }
            hash.update((id.len() as u64).to_be_bytes());
            hash.update(id.as_bytes());
            hash.update(((bytes.len() / 4) as u64).to_be_bytes());
            for chunk in bytes.chunks_exact(4) {
                hash.update(
                    i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]).to_be_bytes(),
                );
            }
            hash.update(input.to_be_bytes());
            hash.update(expected.to_be_bytes());
            count += 1;
        }
        if count == 0 || count > 1000 || format!("{:x}", hash.finalize()) != stored {
            return Err("fixture digest mismatch".into());
        }
        Ok((version, stored))
    }
    pub fn window_replay_matches_pin(&self, domain: &str, run: i64) -> Result<bool, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let pinned: Option<(String, String)> = tx
            .query_row(
                "SELECT pack_version,pack_digest FROM window_fixture_pins WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let pin = pinned.ok_or("fixture pin missing")?;
        let identity = Self::window_replay_identity_conn(&tx, run)?;
        let matches = pin == identity;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(matches)
    }
    /// Synthetic metadata only; does not activate a runtime slot.
    pub fn bootstrap_champion(
        &mut self,
        domain: &str,
        champion: &str,
        actor: &str,
    ) -> Result<(), String> {
        if domain.trim().is_empty() || actor.trim().is_empty() {
            return Err("invalid bootstrap".into());
        }
        self.artifact_for_abi(champion, 2)?;
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let (bytes, runtime, abi): (Vec<u8>, String, i64) = tx
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [champion],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if digest(&bytes) != champion || runtime != "wasmi-experimental" || abi != 2 {
            return Err("champion artifact invalid".into());
        }
        let pin: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM window_fixture_pins WHERE domain=?1)",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !pin {
            return Err("fixture pin missing".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        tx.execute(
            "INSERT INTO champion_domains VALUES (?1,?2,0)",
            params![domain, champion],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO champion_transitions(domain,previous_id,next_id,review_id,actor,created_unix_ns,kind) VALUES (?1,'',?2,0,?3,?4,'bootstrap')",params![domain,champion,actor,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_champion_transition(
            &tx,
            id,
            domain,
            "",
            champion,
            0,
            actor,
            &now,
            "bootstrap",
        )?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn current_champion(&self, domain: &str) -> Result<String, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let result = tx
            .query_row(
                "SELECT champion_id FROM champion_domains WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
    /// Only updates local synthetic metadata; never mounts or executes a candidate.
    pub fn promote_window(
        &mut self,
        domain: &str,
        expected_champion: &str,
        review: i64,
        actor: &str,
    ) -> Result<(), String> {
        if actor.trim().is_empty() {
            return Err("actor required".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let current: String = tx
            .query_row(
                "SELECT champion_id FROM champion_domains WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current != expected_champion {
            return Err("stale champion".into());
        }
        let (candidate,replay,shadow):(String,i64,i64)=tx.query_row("SELECT candidate_id,replay_id,shadow_id FROM window_review_records WHERE id=?1 AND action='approve'",[review],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let synthetic: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM synthetic_window_candidates WHERE candidate_id=?1)",
                [&candidate],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !synthetic {
            return Err("candidate not authorized for synthetic execution".into());
        }
        if candidate == current {
            return Err("candidate already champion".into());
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM window_review_cards c JOIN ledger_events e ON e.reference=c.review_id AND e.kind='window_review_card_bound' WHERE c.review_id=?1) AND EXISTS(SELECT 1 FROM window_observations s WHERE s.id=?2 AND s.champion_id=?3 AND s.shadow_id=?4 AND s.champion_result IS NOT NULL AND s.shadow_result IS NOT NULL AND s.champion_error IS NULL AND s.shadow_error IS NULL) AND EXISTS(SELECT 1 FROM window_replay_runs r WHERE r.id=?5 AND r.candidate_id=?4 AND r.passed=1) AND ?5=(SELECT MAX(id) FROM window_replay_runs WHERE candidate_id=?4) AND ?2=(SELECT MAX(id) FROM window_observations WHERE shadow_id=?4) AND ?1=(SELECT MAX(id) FROM window_review_records WHERE candidate_id=?4)",params![review,shadow,current,candidate,replay],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !valid {
            return Err("missing, failed or stale review evidence".into());
        }
        let pin: (String, String) = tx
            .query_row(
                "SELECT pack_version,pack_digest FROM window_fixture_pins WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if Self::window_replay_identity_conn(&tx, replay)? != pin {
            return Err("fixture mismatch".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let changed=tx.execute("UPDATE champion_domains SET champion_id=?1,generation=generation+1 WHERE domain=?2 AND champion_id=?3",params![candidate,domain,current]).map_err(|e|e.to_string())?;
        if changed != 1 {
            return Err("stale champion".into());
        }
        tx.execute("INSERT INTO champion_transitions(domain,previous_id,next_id,review_id,actor,created_unix_ns,kind) VALUES (?1,?2,?3,?4,?5,?6,'promote')",params![domain,current,candidate,review,actor,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_champion_transition(
            &tx, id, domain, &current, &candidate, review, actor, &now, "promote",
        )?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Roll back the latest synthetic metadata promotion only; no runtime activation occurs.
    pub fn rollback_window(
        &mut self,
        domain: &str,
        expected_champion: &str,
        actor: &str,
    ) -> Result<(), String> {
        if actor.trim().is_empty() {
            return Err("actor required".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let current: String = tx
            .query_row(
                "SELECT champion_id FROM champion_domains WHERE domain=?1",
                [domain],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current != expected_champion {
            return Err("stale champion".into());
        }
        let (previous_transition,previous,kind):(i64,String,String)=tx.query_row("SELECT id,previous_id,kind FROM champion_transitions WHERE domain=?1 ORDER BY id DESC LIMIT 1",[domain],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        if kind != "promote" || previous.is_empty() {
            return Err("no promotion to roll back".into());
        }
        let (bytes, runtime, abi): (Vec<u8>, String, i64) = tx
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [&previous],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if digest(&bytes) != previous || runtime != "wasmi-experimental" || abi != 2 {
            return Err("prior artifact invalid".into());
        }
        let synthetic: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM synthetic_window_candidates WHERE candidate_id=?1)",
                [&previous],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !synthetic {
            return Err("prior candidate not authorized for synthetic execution".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let changed=tx.execute("UPDATE champion_domains SET champion_id=?1,generation=generation+1 WHERE domain=?2 AND champion_id=?3",params![previous,domain,current]).map_err(|e|e.to_string())?;
        if changed != 1 {
            return Err("stale champion".into());
        }
        tx.execute("INSERT INTO champion_transitions(domain,previous_id,next_id,review_id,actor,created_unix_ns,kind) VALUES (?1,?2,?3,?4,?5,?6,'rollback')",params![domain,current,previous,previous_transition,actor,now]).map_err(|e|e.to_string())?;
        let id = tx.last_insert_rowid();
        crate::ledger::append_champion_transition(
            &tx,
            id,
            domain,
            &current,
            &previous,
            previous_transition,
            actor,
            &now,
            "rollback",
        )?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub(crate) fn champion_snapshot(&self, domain: &str) -> Result<(String, i64), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        if !crate::ledger::verify_storage(&tx)? {
            return Err("ledger integrity failure".into());
        }
        let result = tx
            .query_row(
                "SELECT champion_id,generation FROM champion_domains WHERE domain=?1",
                [domain],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }
}
