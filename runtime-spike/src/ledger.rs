//! Experimental standalone ledger. A chain is not a signature or an external anchor.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Ledger {
    conn: Connection,
}
fn encode(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|s| format!("{}:{}", s.len(), s))
        .collect()
}
fn hash(prev: &str, kind: &str, reference: i64, payload: &str, time: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            encode(&[
                "sese-ledger-v1",
                prev,
                kind,
                &reference.to_string(),
                payload,
                time
            ])
            .as_bytes()
        )
    )
}
fn verify_conn(conn: &Connection) -> Result<bool, String> {
    let mut statement = conn.prepare("SELECT seq,kind,reference,payload,created_ns,prev,hash FROM ledger_events ORDER BY seq").map_err(|e| e.to_string())?;
    let mut rows = statement.query([]).map_err(|e| e.to_string())?;
    let mut prev = "GENESIS".to_string();
    let mut seq = 0;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let n: i64 = row.get(0).map_err(|e| e.to_string())?;
        let kind: String = row.get(1).map_err(|e| e.to_string())?;
        let reference: i64 = row.get(2).map_err(|e| e.to_string())?;
        let payload: String = row.get(3).map_err(|e| e.to_string())?;
        let time: String = row.get(4).map_err(|e| e.to_string())?;
        let link: String = row.get(5).map_err(|e| e.to_string())?;
        let recorded: String = row.get(6).map_err(|e| e.to_string())?;
        if n != seq + 1
            || link != prev
            || recorded != hash(&prev, &kind, reference, &payload, &time)
        {
            return Ok(false);
        }
        seq = n;
        prev = recorded;
    }
    Ok(true)
}
impl Ledger {
    pub fn new(conn: Connection) -> Result<Self, String> {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS ledger_events (seq INTEGER PRIMARY KEY, kind TEXT NOT NULL, reference INTEGER NOT NULL, payload TEXT NOT NULL, created_ns TEXT NOT NULL, prev TEXT NOT NULL, hash TEXT NOT NULL)").map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }
    pub fn verify(&self) -> Result<bool, String> {
        verify_conn(&self.conn)
    }
    fn append(&mut self, kind: &str, reference: i64, payload: &str) -> Result<i64, String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if !verify_conn(&tx)? {
            return Err("ledger integrity failure".into());
        }
        if kind == "outcome" {
            let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ledger_events WHERE seq=?1 AND kind='decision') AND NOT EXISTS(SELECT 1 FROM ledger_events WHERE reference=?1 AND kind='outcome')", [reference], |r| r.get(0)).map_err(|e| e.to_string())?;
            if !valid {
                return Err("unknown or already resolved decision".into());
            }
        }
        let last: Option<(i64, String)> = tx
            .query_row(
                "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let (seq, prev) = last
            .map(|(n, h)| (n + 1, h))
            .unwrap_or((1, "GENESIS".into()));
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let digest = hash(&prev, kind, reference, payload, &time);
        tx.execute(
            "INSERT INTO ledger_events VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![seq, kind, reference, payload, time, prev, digest],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(seq)
    }
    pub fn decision(
        &mut self,
        candidate: &str,
        actor: &str,
        action: &str,
        human: &str,
        bridge: &str,
        proposal: &str,
    ) -> Result<i64, String> {
        if [candidate, actor, human, bridge, proposal]
            .iter()
            .any(|v| v.trim().is_empty())
            || !["approve", "reject"].contains(&action)
        {
            return Err("invalid decision".into());
        }
        self.append(
            "decision",
            0,
            &encode(&[
                candidate,
                actor,
                action,
                human,
                bridge,
                proposal,
                "synthetic",
            ]),
        )
    }
    pub fn outcome(
        &mut self,
        decision: i64,
        status: &str,
        actor: &str,
        source: &str,
    ) -> Result<i64, String> {
        if !["validated", "falsified"].contains(&status)
            || actor.trim().is_empty()
            || source.trim().is_empty()
        {
            return Err("invalid outcome".into());
        }
        self.append(
            "outcome",
            decision,
            &encode(&[status, actor, source, "synthetic"]),
        )
    }
}

// Versioned, typed, length-delimited serialization of stored evidence.
fn evidence_digest(conn: &Connection, replay: i64, shadow: i64) -> Result<String, String> {
    use rusqlite::types::ValueRef;
    let present: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM replay_runs r JOIN candidates rc ON rc.id=r.candidate_id JOIN shadow_observations s ON s.id=?2 JOIN candidates cc ON cc.id=s.champion_id JOIN candidates sc ON sc.id=s.shadow_id WHERE r.id=?1)", params![replay,shadow], |r|r.get(0)).map_err(|e|e.to_string())?;
    if !present {
        return Err("missing evidence artifact".into());
    }
    let mut check = conn.prepare("SELECT DISTINCT c.id,c.artifact,c.runtime,c.abi_version FROM candidates c WHERE c.id IN (SELECT candidate_id FROM replay_runs WHERE id=?1 UNION SELECT champion_id FROM shadow_observations WHERE id=?2 UNION SELECT shadow_id FROM shadow_observations WHERE id=?2)").map_err(|e|e.to_string())?;
    let mut artifacts = check
        .query(params![replay, shadow])
        .map_err(|e| e.to_string())?;
    let mut count = 0;
    while let Some(row) = artifacts.next().map_err(|e| e.to_string())? {
        let id: String = row.get(0).map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
        let runtime: String = row.get(2).map_err(|e| e.to_string())?;
        let abi: i64 = row.get(3).map_err(|e| e.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != id
            || runtime != "wasmi-experimental"
            || abi != 1
        {
            return Err("artifact integrity failure".into());
        }
        count += 1;
    }
    if count == 0 {
        return Err("missing artifact".into());
    }
    let mut hasher = Sha256::new();
    hasher.update(b"sese-evidence-v1\0");
    for (tag, query, id, required) in [
        (b"run".as_slice(), "SELECT id,candidate_id,pack_version,pack_digest,passed,started_at_unix_ns,environment FROM replay_runs WHERE id=?1", replay, true),
        (b"cases".as_slice(), "SELECT case_id,input,expected,actual,error,passed FROM replay_cases WHERE run_id=?1 ORDER BY case_id COLLATE BINARY", replay, true),
        (b"shadow".as_slice(), "SELECT id,champion_id,shadow_id,input,champion_result,champion_error,shadow_result,shadow_error,champion_us,shadow_us,observed_unix_ns FROM shadow_observations WHERE id=?1", shadow, true),
    ] {
        hasher.update((tag.len() as u64).to_be_bytes()); hasher.update(tag);
        let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;
        let cols = stmt.column_count();
        let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
        let mut count = 0_u64;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            count += 1; hasher.update(b"row");
            for i in 0..cols {
                match row.get_ref(i).map_err(|e| e.to_string())? {
                    ValueRef::Null => hasher.update([0]),
                    ValueRef::Integer(v) => { hasher.update([1]); hasher.update(v.to_be_bytes()); },
                    ValueRef::Text(v) => { hasher.update([2]); hasher.update((v.len() as u64).to_be_bytes()); hasher.update(v); },
                    _ => return Err("unsupported evidence type".into()),
                }
            }
        }
        if required && count == 0 { return Err("missing evidence".into()); }
        hasher.update(count.to_be_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn window_evidence_digest(conn: &Connection, replay: i64, shadow: i64) -> Result<String, String> {
    use rusqlite::types::ValueRef;
    let present: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM window_replay_runs r JOIN candidates rc ON rc.id=r.candidate_id JOIN window_observations s ON s.id=?2 JOIN candidates cc ON cc.id=s.champion_id JOIN candidates sc ON sc.id=s.shadow_id WHERE r.id=?1)", params![replay,shadow], |r|r.get(0)).map_err(|e|e.to_string())?;
    if !present {
        return Err("missing evidence artifact".into());
    }
    let mut check = conn.prepare("SELECT DISTINCT c.id,c.artifact,c.runtime,c.abi_version FROM candidates c WHERE c.id IN (SELECT candidate_id FROM window_replay_runs WHERE id=?1 UNION SELECT champion_id FROM window_observations WHERE id=?2 UNION SELECT shadow_id FROM window_observations WHERE id=?2)").map_err(|e|e.to_string())?;
    let mut artifacts = check
        .query(params![replay, shadow])
        .map_err(|e| e.to_string())?;
    let mut count = 0;
    while let Some(row) = artifacts.next().map_err(|e| e.to_string())? {
        let id: String = row.get(0).map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
        let runtime: String = row.get(2).map_err(|e| e.to_string())?;
        let abi: i64 = row.get(3).map_err(|e| e.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != id
            || runtime != "wasmi-experimental"
            || abi != 2
        {
            return Err("artifact integrity failure".into());
        }
        count += 1;
    }
    if count == 0 {
        return Err("missing artifact".into());
    }
    let mut hasher = Sha256::new();
    hasher.update(b"sese-window-evidence-v1\0");
    for (tag, query, id, required) in [
        (b"run".as_slice(), "SELECT id,candidate_id,pack_version,pack_digest,passed,started_unix_ns,environment FROM window_replay_runs WHERE id=?1", replay, true),
        (b"cases".as_slice(), "SELECT case_id,prior_window,input,expected,actual,error,passed FROM window_replay_cases WHERE run_id=?1 ORDER BY case_id COLLATE BINARY", replay, true),
        (b"shadow".as_slice(), "SELECT id,champion_id,shadow_id,prior_window,input,champion_result,champion_error,shadow_result,shadow_error,champion_us,shadow_us,observed_unix_ns FROM window_observations WHERE id=?1", shadow, true),
    ] {
        hasher.update((tag.len() as u64).to_be_bytes()); hasher.update(tag);
        let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;
        let cols = stmt.column_count();
        let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
        let mut count = 0_u64;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            count += 1; hasher.update(b"row");
            for i in 0..cols {
                match row.get_ref(i).map_err(|e| e.to_string())? {
                    ValueRef::Null => hasher.update([0]),
                    ValueRef::Integer(v) => { hasher.update([1]); hasher.update(v.to_be_bytes()); },
                    ValueRef::Text(v) => { hasher.update([2]); hasher.update((v.len() as u64).to_be_bytes()); hasher.update(v); },
                    ValueRef::Blob(v) => { hasher.update([3]); hasher.update((v.len() as u64).to_be_bytes()); hasher.update(v); },
                    _ => return Err("unsupported evidence type".into()),
                }
            }
        }
        if required && count == 0 { return Err("missing evidence".into()); }
        hasher.update(count.to_be_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn append_review(
    conn: &Connection,
    review: i64,
    candidate: &str,
    replay: i64,
    shadow: i64,
    actor: &str,
    action: &str,
    time: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let evidence = evidence_digest(conn, replay, shadow)?;
    let payload = encode(&[
        candidate,
        &replay.to_string(),
        &shadow.to_string(),
        actor,
        action,
        "synthetic-evidence-v1",
        &evidence,
    ]);
    let digest = hash(&prev, "review", review, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'review',?2,?3,?4,?5,?6)",
        params![seq, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn append_window_review(
    conn: &Connection,
    review: i64,
    candidate: &str,
    replay: i64,
    shadow: i64,
    actor: &str,
    action: &str,
    time: &str,
    card_required: bool,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let evidence = window_evidence_digest(conn, replay, shadow)?;
    let payload = encode(&[
        candidate,
        &replay.to_string(),
        &shadow.to_string(),
        actor,
        action,
        "synthetic-window-evidence-v1",
        &evidence,
    ]);
    let kind = if card_required {
        "window_review_card_bound"
    } else {
        "window_review"
    };
    let digest = hash(&prev, kind, review, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![seq, kind, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn verify_storage(conn: &Connection) -> Result<bool, String> {
    if !verify_conn(conn)? {
        return Ok(false);
    }
    // Legacy typed-event runs without a link remain untrusted by their dedicated verifier.
    let mut event_stmt = conn
        .prepare("SELECT reference FROM ledger_events WHERE kind='event_replay'")
        .map_err(|e| e.to_string())?;
    let mut event_rows = event_stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = event_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_replay_binding_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let mut observation_stmt = conn
        .prepare("SELECT reference FROM ledger_events WHERE kind='event_observation'")
        .map_err(|e| e.to_string())?;
    let mut observation_rows = observation_stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = observation_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_observation_binding_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_reviews r ON e.reference=r.id WHERE e.kind='event_review' AND r.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut review_stmt = conn
        .prepare("SELECT id FROM event_reviews")
        .map_err(|e| e.to_string())?;
    let mut review_rows = review_stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = review_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_review_binding_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_review_outcomes o ON e.reference=o.review_id WHERE e.kind='event_outcome' AND o.review_id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut outcome_stmt = conn
        .prepare("SELECT review_id FROM event_review_outcomes")
        .map_err(|e| e.to_string())?;
    let mut outcome_rows = outcome_stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = outcome_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_outcome_binding_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_training_traces t ON t.id=e.reference WHERE e.kind='event_training_trace' AND t.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut traces = conn
        .prepare("SELECT id FROM event_training_traces")
        .map_err(|e| e.to_string())?;
    let mut trace_rows = traces.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = trace_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_training_trace_binding_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_preference_receipts a ON a.id=e.reference WHERE e.kind='event_preference_receipt' AND a.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut pref_stmt = conn
        .prepare("SELECT id FROM event_preference_receipts")
        .map_err(|e| e.to_string())?;
    let mut pref_rows = pref_stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = pref_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_preference_receipt_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_attestation_receipts a ON a.id=e.reference WHERE e.kind='event_attestation_receipt' AND a.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut attestations = conn
        .prepare("SELECT id FROM event_attestation_receipts")
        .map_err(|e| e.to_string())?;
    let mut attestation_rows = attestations.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = attestation_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_attestation_receipt_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_followup_pins p ON p.id=e.reference WHERE e.kind='event_followup_pin' AND p.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt = conn
        .prepare("SELECT id FROM event_followup_pins")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_followup_pin_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_followup_receipts r ON r.id=e.reference WHERE e.kind='event_followup_receipt' AND r.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt = conn
        .prepare("SELECT id FROM event_followup_receipts")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_followup_receipt_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_fixture_pins p ON e.reference=p.id WHERE e.kind='event_fixture_pin' AND p.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut pin_stmt = conn
        .prepare("SELECT id FROM event_fixture_pins")
        .map_err(|e| e.to_string())?;
    let mut pin_rows = pin_stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = pin_rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !event_fixture_pin_binding_matches(conn, id).unwrap_or(false) {
            return Ok(false);
        }
    }
    if !verify_event_champions(conn)? {
        return Ok(false);
    }
    if !verify_event_streams(conn)? {
        return Ok(false);
    }
    if !verify_measured_streams(conn)? {
        return Ok(false);
    }
    if !verify_synthetic_event_candidates(conn)? {
        return Ok(false);
    }
    if !verify_synthetic_window_candidates(conn)? {
        return Ok(false);
    }
    if !verify_synthetic_scalar_candidates(conn)? {
        return Ok(false);
    }
    if !verify_event_worker_provenance(conn)? {
        return Ok(false);
    }
    let orphan_events: i64 = conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN review_records r ON r.id=e.reference WHERE e.kind='review' AND r.id IS NULL", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if orphan_events != 0 {
        return Ok(false);
    }
    let mut stmt = conn.prepare("SELECT r.id,r.candidate_id,r.replay_id,r.shadow_id,r.actor,r.action,r.created_unix_ns,e.payload,e.created_ns FROM review_records r LEFT JOIN ledger_events e ON e.kind='review' AND e.reference=r.id ORDER BY r.id").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let candidate: String = row.get(1).map_err(|e| e.to_string())?;
        let replay: i64 = row.get(2).map_err(|e| e.to_string())?;
        let shadow: i64 = row.get(3).map_err(|e| e.to_string())?;
        let actor: String = row.get(4).map_err(|e| e.to_string())?;
        let action: String = row.get(5).map_err(|e| e.to_string())?;
        let time: String = row.get(6).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(7).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(8).map_err(|e| e.to_string())?;
        let evidence = match evidence_digest(conn, replay, shadow) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        let expected = encode(&[
            &candidate,
            &replay.to_string(),
            &shadow.to_string(),
            &actor,
            &action,
            "synthetic-evidence-v1",
            &evidence,
        ]);
        if payload.as_deref() != Some(expected.as_str())
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
    }
    let window_orphans: i64 = conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN window_review_records r ON r.id=e.reference WHERE e.kind IN ('window_review','window_review_card_bound') AND r.id IS NULL", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if window_orphans != 0 {
        return Ok(false);
    }
    let mut stmt = conn.prepare("SELECT r.id,r.candidate_id,r.replay_id,r.shadow_id,r.actor,r.action,r.created_unix_ns,e.payload,e.created_ns,e.kind FROM window_review_records r LEFT JOIN ledger_events e ON e.kind IN ('window_review','window_review_card_bound') AND e.reference=r.id ORDER BY r.id").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let candidate: String = row.get(1).map_err(|e| e.to_string())?;
        let replay: i64 = row.get(2).map_err(|e| e.to_string())?;
        let shadow: i64 = row.get(3).map_err(|e| e.to_string())?;
        let actor: String = row.get(4).map_err(|e| e.to_string())?;
        let action: String = row.get(5).map_err(|e| e.to_string())?;
        let time: String = row.get(6).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(7).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(8).map_err(|e| e.to_string())?;
        let kind: Option<String> = row.get(9).map_err(|e| e.to_string())?;
        if kind.as_deref() == Some("window_review_card_bound") {
            let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM window_review_cards c JOIN ledger_events e ON e.kind='window_card' AND e.reference=c.review_id WHERE c.review_id=?1)",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !exists {
                return Ok(false);
            }
        }
        let evidence = match window_evidence_digest(conn, replay, shadow) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        let expected = encode(&[
            &candidate,
            &replay.to_string(),
            &shadow.to_string(),
            &actor,
            &action,
            "synthetic-window-evidence-v1",
            &evidence,
        ]);
        if payload.as_deref() != Some(expected.as_str())
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
    }
    let orphan_cards:i64=conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN window_review_cards c ON c.review_id=e.reference WHERE e.kind='window_card' AND c.review_id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan_cards != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT c.review_id,c.human,c.bridge,c.proposal,c.created_unix_ns,e.payload,e.created_ns FROM window_review_cards c LEFT JOIN ledger_events e ON e.kind='window_card' AND e.reference=c.review_id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let human: String = row.get(1).map_err(|e| e.to_string())?;
        let bridge: String = row.get(2).map_err(|e| e.to_string())?;
        let proposal: String = row.get(3).map_err(|e| e.to_string())?;
        let time: String = row.get(4).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(5).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        if payload.as_deref()
            != Some(encode(&[&human, &bridge, &proposal, "synthetic-window-card-v1"]).as_str())
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
    }
    let orphan: i64 = conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN review_outcomes o ON o.review_id=e.reference WHERE e.kind='review_outcome' AND o.review_id IS NULL", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT o.review_id,o.status,o.actor,o.source,o.created_unix_ns,e.payload,e.created_ns FROM review_outcomes o LEFT JOIN ledger_events e ON e.kind='review_outcome' AND e.reference=o.review_id").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let status: String = row.get(1).map_err(|e| e.to_string())?;
        let actor: String = row.get(2).map_err(|e| e.to_string())?;
        let source: String = row.get(3).map_err(|e| e.to_string())?;
        let time: String = row.get(4).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(5).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        if payload.as_deref()
            != Some(encode(&[&status, &actor, &source, "synthetic-outcome-v1"]).as_str())
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
    }
    let orphan: i64 = conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN window_review_outcomes o ON o.review_id=e.reference WHERE e.kind='window_review_outcome' AND o.review_id IS NULL", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT o.review_id,o.status,o.actor,o.source,o.created_unix_ns,e.payload,e.created_ns FROM window_review_outcomes o LEFT JOIN ledger_events e ON e.kind='window_review_outcome' AND e.reference=o.review_id").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let status: String = row.get(1).map_err(|e| e.to_string())?;
        let actor: String = row.get(2).map_err(|e| e.to_string())?;
        let source: String = row.get(3).map_err(|e| e.to_string())?;
        let time: String = row.get(4).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(5).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        if payload.as_deref()
            != Some(encode(&[&status, &actor, &source, "synthetic-window-outcome-v1"]).as_str())
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN window_fixture_pins p ON p.id=e.reference WHERE e.kind='fixture_pin' AND p.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT p.id,p.domain,p.pack_version,p.pack_digest,p.actor,p.created_unix_ns,e.payload,e.created_ns FROM window_fixture_pins p LEFT JOIN ledger_events e ON e.kind='fixture_pin' AND e.reference=p.id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let domain: String = row.get(1).map_err(|e| e.to_string())?;
        let version: String = row.get(2).map_err(|e| e.to_string())?;
        let digest: String = row.get(3).map_err(|e| e.to_string())?;
        let actor: String = row.get(4).map_err(|e| e.to_string())?;
        let time: String = row.get(5).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(7).map_err(|e| e.to_string())?;
        if payload.as_deref()
            != Some(
                encode(&[
                    &domain,
                    &version,
                    &digest,
                    &actor,
                    "synthetic-fixture-pin-v1",
                ])
                .as_str(),
            )
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
    }
    let orphan:i64=conn.query_row("SELECT COUNT(*) FROM ledger_events e LEFT JOIN champion_transitions t ON t.id=e.reference WHERE e.kind='champion_transition' AND t.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT t.id,t.domain,t.previous_id,t.next_id,t.review_id,t.actor,t.created_unix_ns,t.kind,e.payload,e.created_ns FROM champion_transitions t LEFT JOIN ledger_events e ON e.kind='champion_transition' AND e.reference=t.id ORDER BY t.id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    let mut states: std::collections::HashMap<String, (String, i64)> =
        std::collections::HashMap::new();
    let mut last_transition: std::collections::HashMap<String, (i64, String, String)> =
        std::collections::HashMap::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let domain: String = row.get(1).map_err(|e| e.to_string())?;
        let prev: String = row.get(2).map_err(|e| e.to_string())?;
        let next: String = row.get(3).map_err(|e| e.to_string())?;
        let review: i64 = row.get(4).map_err(|e| e.to_string())?;
        let actor: String = row.get(5).map_err(|e| e.to_string())?;
        let time: String = row.get(6).map_err(|e| e.to_string())?;
        let kind: String = row.get(7).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(8).map_err(|e| e.to_string())?;
        let event_time: Option<String> = row.get(9).map_err(|e| e.to_string())?;
        if payload.as_deref()
            != Some(
                encode(&[
                    &domain,
                    &prev,
                    &next,
                    &review.to_string(),
                    &actor,
                    &kind,
                    "synthetic-champion-v1",
                ])
                .as_str(),
            )
            || event_time.as_deref() != Some(time.as_str())
        {
            return Ok(false);
        }
        let generation = match states.get(&domain) {
            None if kind == "bootstrap" && prev.is_empty() && review == 0 => 0,
            Some((old, n)) if kind == "promote" && old == &prev && review > 0 && prev != next => {
                n + 1
            }
            Some((old, n))
                if kind == "rollback"
                    && old == &prev
                    && prev != next
                    && last_transition
                        .get(&domain)
                        .is_some_and(|(last, prior, last_kind)| {
                            *last == review && prior == &next && last_kind == "promote"
                        }) =>
            {
                n + 1
            }
            _ => return Ok(false),
        };
        last_transition.insert(domain.clone(), (id, prev, kind));
        states.insert(domain, (next, generation));
    }
    let mut stmt = conn
        .prepare("SELECT domain,champion_id,generation FROM champion_domains")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut count = 0usize;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let domain: String = row.get(0).map_err(|e| e.to_string())?;
        let id: String = row.get(1).map_err(|e| e.to_string())?;
        let generation: i64 = row.get(2).map_err(|e| e.to_string())?;
        let artifact: Option<(Vec<u8>, String, i64)> = conn
            .query_row(
                "SELECT artifact,runtime,abi_version FROM candidates WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match artifact {
            Some((bytes, runtime, abi))
                if format!("{:x}", Sha256::digest(&bytes)) == id
                    && runtime == "wasmi-experimental"
                    && abi == 2 =>
            {
                ()
            }
            _ => return Ok(false),
        }
        if states.get(&domain) != Some(&(id, generation)) {
            return Ok(false);
        }
        count += 1;
    }
    if count != states.len() {
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn append_outcome(
    conn: &Connection,
    review: i64,
    status: &str,
    actor: &str,
    source: &str,
    time: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&[status, actor, source, "synthetic-outcome-v1"]);
    let digest = hash(&prev, "review_outcome", review, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'review_outcome',?2,?3,?4,?5,?6)",
        params![seq, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn append_window_outcome(
    conn: &Connection,
    review: i64,
    status: &str,
    actor: &str,
    source: &str,
    time: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&[status, actor, source, "synthetic-window-outcome-v1"]);
    let digest = hash(&prev, "window_review_outcome", review, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'window_review_outcome',?2,?3,?4,?5,?6)",
        params![seq, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn append_window_card(
    conn: &Connection,
    review: i64,
    human: &str,
    bridge: &str,
    proposal: &str,
    time: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&[human, bridge, proposal, "synthetic-window-card-v1"]);
    let digest = hash(&prev, "window_card", review, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'window_card',?2,?3,?4,?5,?6)",
        params![seq, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn append_fixture_pin(
    conn: &Connection,
    review: i64,
    domain: &str,
    version: &str,
    pack_digest: &str,
    actor: &str,
    time: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&[
        domain,
        version,
        pack_digest,
        actor,
        "synthetic-fixture-pin-v1",
    ]);
    let digest = hash(&prev, "fixture_pin", review, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'fixture_pin',?2,?3,?4,?5,?6)",
        params![seq, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn append_champion_transition(
    conn: &Connection,
    id: i64,
    domain: &str,
    prev: &str,
    next: &str,
    review: i64,
    actor: &str,
    time: &str,
    kind: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, previous) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&[
        domain,
        prev,
        next,
        &review.to_string(),
        actor,
        kind,
        "synthetic-champion-v1",
    ]);
    let digest = hash(&previous, "champion_transition", id, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'champion_transition',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, previous, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// All persisted replay fields and candidate bytes are bound, including failed results.
fn event_replay_evidence(conn: &Connection, id: i64) -> Result<String, String> {
    use rusqlite::types::ValueRef;
    let mut hasher = Sha256::new();
    hasher.update(b"sese-event-replay-evidence-v1\0");
    let semantics: String = conn
        .query_row(
            "SELECT state_semantics FROM event_replay_runs WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    match semantics.as_str() {
        "legacy-v1" => (),
        "measured-v2" => hasher.update(b"measured-v2\0"),
        _ => return Err("invalid replay semantics".into()),
    };

    for (tag,sql) in [
        ("run", "SELECT r.id,r.candidate_id,r.pack_version,r.pack_digest,r.passed,r.started_unix_ns,r.environment,c.artifact,c.runtime,c.abi_version FROM event_replay_runs r JOIN candidates c ON c.id=r.candidate_id WHERE r.id=?1"),
        ("cases", "SELECT ordinal,case_id,window_json,event_json,expected_json,actual_json,error,passed FROM event_replay_cases WHERE run_id=?1 ORDER BY ordinal"),
    ] {
        hasher.update((tag.len() as u64).to_be_bytes()); hasher.update(tag.as_bytes());
        let mut stmt=conn.prepare(sql).map_err(|e|e.to_string())?;
        let cols=stmt.column_count(); let mut rows=stmt.query([id]).map_err(|e|e.to_string())?; let mut count=0u64;
        while let Some(row)=rows.next().map_err(|e|e.to_string())? {
            count+=1; hasher.update(b"row");
            for i in 0..cols { match row.get_ref(i).map_err(|e|e.to_string())? {
                ValueRef::Null=>hasher.update([0]),
                ValueRef::Integer(v)=>{hasher.update([1]);hasher.update(v.to_be_bytes());},
                ValueRef::Text(v)=>{hasher.update([2]);hasher.update((v.len() as u64).to_be_bytes());hasher.update(v);},
                ValueRef::Blob(v)=>{hasher.update([3]);hasher.update((v.len() as u64).to_be_bytes());hasher.update(v);},
                _=>return Err("invalid event replay evidence".into()),
            }}
        }
        if count==0 || (tag=="run" && count!=1) || (tag=="cases" && count>1000) {return Err("invalid event replay evidence count".into());}
        hasher.update(count.to_be_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn append_event_replay(conn: &Connection, id: i64, time: &str) -> Result<(), String> {
    if !verify_storage(conn)? {
        return Err("ledger integrity failure".into());
    }
    let evidence = event_replay_evidence(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&["synthetic-event-replay-v1", &evidence]);
    let digest = hash(&prev, "event_replay", id, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_replay',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Append a caller assertion; it is never interpreted as replay or promotion evidence.
pub(crate) fn append_container_comparison_assertion(
    conn: &Connection,
    replay: i64,
    payload: &str,
    time: &str,
) -> Result<i64, String> {
    let (last, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let seq = last.checked_add(1).ok_or("ledger sequence overflow")?;
    let digest = hash(
        &prev,
        "container_comparison_assertion",
        replay,
        payload,
        time,
    );
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'container_comparison_assertion',?2,?3,?4,?5,?6)",
        params![seq, replay, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(seq)
}

/// Record the local proposal source and artifact identity, not execution evidence.
pub(crate) fn append_event_proposal(
    conn: &Connection,
    payload: &str,
    time: &str,
) -> Result<i64, String> {
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = match last {
        Some((n, h)) => (n.checked_add(1).ok_or("ledger sequence overflow")?, h),
        None => (1, "GENESIS".into()),
    };
    let digest = hash(&prev, "event_proposal", 0, payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_proposal',0,?2,?3,?4,?5)",
        params![seq, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(seq)
}

#[cfg(unix)]
pub(crate) fn append_container_replay_experiment(
    conn: &Connection,
    payload: &str,
    time: &str,
) -> Result<i64, String> {
    let (last, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let seq = last.checked_add(1).ok_or("ledger sequence overflow")?;
    let digest = hash(&prev, "container_replay_experiment", 0, payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'container_replay_experiment',0,?2,?3,?4,?5)",
        params![seq, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(seq)
}

#[cfg(unix)]
pub(crate) fn append_container_shadow_experiment(
    conn: &Connection,
    payload: &str,
    time: &str,
) -> Result<i64, String> {
    let (last, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let seq = last.checked_add(1).ok_or("ledger sequence overflow")?;
    let digest = hash(&prev, "container_shadow_experiment", 0, payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'container_shadow_experiment',0,?2,?3,?4,?5)",
        params![seq, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(seq)
}

#[cfg(unix)]
pub(crate) fn append_container_observation_experiment(
    conn: &Connection,
    payload: &str,
    time: &str,
) -> Result<i64, String> {
    let (last, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let seq = last.checked_add(1).ok_or("ledger sequence overflow")?;
    let digest = hash(&prev, "container_observation_experiment", 0, payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'container_observation_experiment',0,?2,?3,?4,?5)",
        params![seq, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(seq)
}

pub(crate) fn event_replay_binding_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_replay' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let payload: String = row.get(0).map_err(|e| e.to_string())?;
    let time: String = row.get(1).map_err(|e| e.to_string())?;
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    let started: String = conn
        .query_row(
            "SELECT started_unix_ns FROM event_replay_runs WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(time == started
        && payload
            == encode(&[
                "synthetic-event-replay-v1",
                &event_replay_evidence(conn, id)?,
            ]))
}

fn event_observation_evidence(conn: &Connection, id: i64) -> Result<String, String> {
    use rusqlite::types::ValueRef;
    let mut stmt=conn.prepare("SELECT o.id,o.champion_id,o.shadow_id,o.prior_json,o.event_json,o.champion_json,o.champion_error,o.shadow_json,o.shadow_error,o.champion_us,o.shadow_us,o.observed_unix_ns,c.artifact,c.runtime,c.abi_version,s.artifact,s.runtime,s.abi_version FROM event_observations o JOIN candidates c ON c.id=o.champion_id JOIN candidates s ON s.id=o.shadow_id WHERE o.id=?1").map_err(|e|e.to_string())?;
    let columns = stmt.column_count();
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let row = rows
        .next()
        .map_err(|e| e.to_string())?
        .ok_or("missing observation")?;
    for (id_col, bytes_col, runtime_col, abi_col) in [(1, 12, 13, 14), (2, 15, 16, 17)] {
        let candidate: String = row.get(id_col).map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = row.get(bytes_col).map_err(|e| e.to_string())?;
        let runtime: String = row.get(runtime_col).map_err(|e| e.to_string())?;
        let abi: i64 = row.get(abi_col).map_err(|e| e.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != candidate
            || runtime != "wasmi-experimental"
            || abi != 3
        {
            return Err("observation artifact integrity failure".into());
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(b"sese-event-observation-evidence-v1\0");
    let domain: Option<String> = conn
        .query_row(
            "SELECT domain FROM event_observations WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if let Some(domain) = domain {
        hasher.update(b"domain-v1\0");
        hasher.update((domain.len() as u64).to_be_bytes());
        hasher.update(domain.as_bytes());
    }

    for i in 0..columns {
        match row.get_ref(i).map_err(|e| e.to_string())? {
            ValueRef::Null => hasher.update([0]),
            ValueRef::Integer(v) => {
                hasher.update([1]);
                hasher.update(v.to_be_bytes());
            }
            ValueRef::Text(v) => {
                hasher.update([2]);
                hasher.update((v.len() as u64).to_be_bytes());
                hasher.update(v);
            }
            ValueRef::Blob(v) => {
                hasher.update([3]);
                hasher.update((v.len() as u64).to_be_bytes());
                hasher.update(v);
            }
            _ => return Err("invalid observation evidence".into()),
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn append_event_observation(
    conn: &Connection,
    id: i64,
    time: &str,
) -> Result<(), String> {
    if !verify_storage(conn)? {
        return Err("ledger integrity failure".into());
    }
    let evidence = event_observation_evidence(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let payload = encode(&["synthetic-event-observation-v1", &evidence]);
    let digest = hash(&prev, "event_observation", id, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_observation',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn event_observation_binding_matches(
    conn: &Connection,
    id: i64,
) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_observation' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let payload: String = row.get(0).map_err(|e| e.to_string())?;
    let time: String = row.get(1).map_err(|e| e.to_string())?;
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    let observed: String = conn
        .query_row(
            "SELECT observed_unix_ns FROM event_observations WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(time == observed
        && payload
            == encode(&[
                "synthetic-event-observation-v1",
                &event_observation_evidence(conn, id)?,
            ]))
}

fn event_review_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (candidate,replay,observation,actor,action,human,bridge,proposal,time):(String,i64,i64,String,String,String,String,String,String)=conn.query_row("SELECT candidate_id,replay_id,observation_id,actor,action,human,bridge,proposal,created_unix_ns FROM event_reviews WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
    if !event_replay_binding_matches(conn, replay)?
        || !event_observation_binding_matches(conn, observation)?
    {
        return Err("missing evidence binding".into());
    }
    let replay_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_replay' AND reference=?1",
            [replay],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let observation_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_observation' AND reference=?1",
            [observation],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok((
        encode(&[
            "synthetic-event-review-v1",
            &candidate,
            &replay.to_string(),
            &observation.to_string(),
            &actor,
            &action,
            &human,
            &bridge,
            &proposal,
            &replay_hash,
            &observation_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_review(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_review_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_review", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_review',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn event_review_binding_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_review' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    Ok(actual == event_review_payload(conn, id)?)
}

fn event_outcome_payload(conn: &Connection, review: i64) -> Result<(String, String), String> {
    let (status,actor,source,time):(String,String,String,String)=conn.query_row("SELECT status,actor,source,created_unix_ns FROM event_review_outcomes WHERE review_id=?1",[review],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
    if !event_review_binding_matches(conn, review)? {
        return Err("missing event review binding".into());
    }
    let review_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_review' AND reference=?1",
            [review],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok((
        encode(&[
            "synthetic-event-outcome-v1",
            &status,
            &actor,
            &source,
            &review_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_outcome(conn: &Connection, review: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_outcome_payload(conn, review)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_outcome", review, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_outcome',?2,?3,?4,?5,?6)",
        params![seq, review, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn event_outcome_binding_matches(conn: &Connection, review: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_outcome' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([review]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    Ok(actual == event_outcome_payload(conn, review)?)
}

fn event_preference_receipt_payload(
    conn: &Connection,
    id: i64,
) -> Result<(String, String, i64), String> {
    let (trace,authority,payload,signature,config,anchor_seq,anchor_hash,time):(i64,String,Vec<u8>,Vec<u8>,String,i64,String,String)=conn.query_row("SELECT trace_id,authority,payload,signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns FROM event_preference_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).map_err(|e|e.to_string())?;
    let valid_hash = |v: &str| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if authority.trim().is_empty()
        || authority.len() > 256
        || payload.is_empty()
        || payload.len() > 131_072
        || signature.len() != 64
        || !valid_hash(&config)
        || !valid_hash(&anchor_hash)
        || anchor_seq <= 0
    {
        return Err("invalid preference receipt".into());
    }
    if !event_training_trace_binding_matches(conn, trace)? {
        return Err("invalid trace binding".into());
    }
    let (trace_seq, trace_hash): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events WHERE kind='event_training_trace' AND reference=?1",
            [trace],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let stored_anchor: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE seq=?1",
            [anchor_seq],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if anchor_seq < trace_seq || stored_anchor != anchor_hash {
        return Err("invalid preference anchor".into());
    }
    let fields = encode(&[
        "synthetic-preference-receipt-v1",
        &trace.to_string(),
        &authority,
        &format!("{:x}", Sha256::digest(&payload)),
        &format!("{:x}", Sha256::digest(&signature)),
        &config,
        &anchor_seq.to_string(),
        &anchor_hash,
        &trace_hash,
    ]);
    Ok((fields, time, anchor_seq))
}

pub(crate) fn append_event_preference_receipt(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time, anchor) = event_preference_receipt_payload(conn, id)?;
    let (seq, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if seq < anchor {
        return Err("invalid preference anchor".into());
    }
    let next = seq.checked_add(1).ok_or("ledger sequence overflow")?;
    let digest = hash(&prev, "event_preference_receipt", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_preference_receipt',?2,?3,?4,?5,?6)",
        params![next, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn event_preference_receipt_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT seq,payload,created_ns FROM ledger_events WHERE kind='event_preference_receipt' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (i64, String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
        row.get(2).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    let (payload, time, anchor) = event_preference_receipt_payload(conn, id)?;
    Ok(actual.0 > anchor && actual.1 == payload && actual.2 == time)
}

fn event_attestation_receipt_payload(
    conn: &Connection,
    id: i64,
) -> Result<(String, String, i64), String> {
    let (trace,source,observer,tp,op,ts,os,config,anchor_seq,anchor_hash,time):(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,String,i64,String,String)=conn.query_row("SELECT trace_id,trace_authority,outcome_authority,trace_payload,outcome_payload,trace_signature,outcome_signature,authority_digest,checkpoint_seq,checkpoint_hash,created_unix_ns FROM event_attestation_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?))).map_err(|e|e.to_string())?;
    let valid_hash = |v: &str| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if [source.as_str(), observer.as_str()]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 256)
        || tp.is_empty()
        || op.is_empty()
        || tp.len() > 131_072
        || op.len() > 131_072
        || ts.len() != 64
        || os.len() != 64
        || !valid_hash(&config)
        || !valid_hash(&anchor_hash)
        || anchor_seq <= 0
    {
        return Err("invalid attestation receipt".into());
    }
    if !event_training_trace_binding_matches(conn, trace)? {
        return Err("invalid trace binding".into());
    }
    let review: i64 = conn
        .query_row(
            "SELECT review_id FROM event_training_traces WHERE id=?1",
            [trace],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !event_outcome_binding_matches(conn, review)? {
        return Err("invalid outcome binding".into());
    }
    let (trace_seq, trace_hash): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events WHERE kind='event_training_trace' AND reference=?1",
            [trace],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let (outcome_seq, outcome_hash): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events WHERE kind='event_outcome' AND reference=?1",
            [review],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let stored_anchor: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE seq=?1",
            [anchor_seq],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if anchor_seq < trace_seq.max(outcome_seq) || stored_anchor != anchor_hash {
        return Err("invalid attestation anchor".into());
    }
    let payload = encode(&[
        "synthetic-attestation-receipt-v1",
        &trace.to_string(),
        &source,
        &observer,
        &format!("{:x}", Sha256::digest(&tp)),
        &format!("{:x}", Sha256::digest(&op)),
        &format!("{:x}", Sha256::digest(&ts)),
        &format!("{:x}", Sha256::digest(&os)),
        &config,
        &anchor_seq.to_string(),
        &anchor_hash,
        &trace_hash,
        &outcome_hash,
    ]);
    Ok((payload, time, anchor_seq))
}

pub(crate) fn append_event_attestation_receipt(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time, anchor) = event_attestation_receipt_payload(conn, id)?;
    let (seq, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if seq < anchor {
        return Err("invalid attestation anchor".into());
    }
    let next = seq.checked_add(1).ok_or("ledger sequence overflow")?;
    let digest = hash(&prev, "event_attestation_receipt", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_attestation_receipt',?2,?3,?4,?5,?6)",
        params![next, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn event_attestation_receipt_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT seq,payload,created_ns FROM ledger_events WHERE kind='event_attestation_receipt' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (i64, String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
        row.get(2).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    let (payload, time, anchor) = event_attestation_receipt_payload(conn, id)?;
    Ok(actual.0 > anchor && actual.1 == payload && actual.2 == time)
}

fn event_training_trace_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (review,actor,source,prompt,response,preferred,rejected,pair_source,time):(i64,String,String,String,String,Option<String>,Option<String>,Option<String>,String)=conn.query_row("SELECT review_id,actor,source,prompt,response,preferred,rejected,pair_source,created_unix_ns FROM event_training_traces WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(|e|e.to_string())?;
    if !event_review_binding_matches(conn, review)? {
        return Err("missing trace review binding".into());
    }
    if [&actor, &source, &prompt, &response]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 16_384)
    {
        return Err("invalid trace fields".into());
    }
    let (pair_tag, preferred, rejected, pair_source) = match (
        preferred.as_deref(),
        rejected.as_deref(),
        pair_source.as_deref(),
    ) {
        (None, None, None) => ("no-pair", "", "", ""),
        (Some(p), Some(r), Some(s))
            if [p, r, s]
                .iter()
                .all(|v| !v.trim().is_empty() && v.len() <= 16_384)
                && p != r =>
        {
            ("explicit-pair", p, r, s)
        }
        _ => return Err("invalid preference pair".into()),
    };
    let review_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_review' AND reference=?1",
            [review],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok((
        encode(&[
            "caller-asserted-synthetic-training-trace-v1",
            &review.to_string(),
            &actor,
            &source,
            &prompt,
            &response,
            pair_tag,
            preferred,
            rejected,
            pair_source,
            &review_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_training_trace(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_training_trace_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_training_trace", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_training_trace',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn event_training_trace_binding_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_training_trace' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    Ok(actual == event_training_trace_payload(conn, id)?)
}

fn event_followup_pin_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (domain,candidate,replay,case_id,actor,time):(String,String,i64,String,String,String)=conn.query_row("SELECT domain,candidate_id,replay_id,case_id,actor,created_unix_ns FROM event_followup_pins WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(|e|e.to_string())?;
    if [
        domain.as_str(),
        candidate.as_str(),
        case_id.as_str(),
        actor.as_str(),
    ]
    .iter()
    .any(|s| s.trim().is_empty() || s.len() > 256)
        || !event_replay_binding_matches(conn, replay)?
    {
        return Err("invalid follow-up selection".into());
    }
    let (run_candidate,run_hash):(String,String)=conn.query_row("SELECT r.candidate_id,e.hash FROM event_replay_runs r JOIN ledger_events e ON e.kind='event_replay' AND e.reference=r.id WHERE r.id=?1",[replay],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM event_replay_cases WHERE run_id=?1 AND case_id=?2)",
            params![replay, case_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if run_candidate != candidate || !exists {
        return Err("invalid follow-up replay or case".into());
    }
    Ok((
        encode(&[
            "synthetic-event-followup-selection-v1",
            &domain,
            &candidate,
            &replay.to_string(),
            &case_id,
            &actor,
            &run_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_followup_pin(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_followup_pin_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_followup_pin", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_followup_pin',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn event_followup_pin_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_followup_pin' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    Ok(actual == event_followup_pin_payload(conn, id)?)
}

fn event_followup_receipt_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (review,observation,pin,status,time):(i64,i64,i64,String,String)=conn.query_row("SELECT review_id,observation_id,pin_id,status,created_unix_ns FROM event_followup_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    if ![
        "match",
        "verdict_mismatch",
        "input_mismatch",
        "worker_error",
    ]
    .contains(&status.as_str())
        || !event_review_binding_matches(conn, review)?
        || !event_observation_binding_matches(conn, observation)?
        || !event_followup_pin_matches(conn, pin)?
    {
        return Err("invalid follow-up receipt evidence".into());
    }
    let parent_hash = |kind: &str, reference: i64| -> Result<String, String> {
        conn.query_row(
            "SELECT hash FROM ledger_events WHERE kind=?1 AND reference=?2",
            params![kind, reference],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    };
    let review_hash = parent_hash("event_review", review)?;
    let observation_hash = parent_hash("event_observation", observation)?;
    let pin_hash = parent_hash("event_followup_pin", pin)?;
    Ok((
        encode(&[
            "synthetic-followup-receipt-v1",
            &review.to_string(),
            &observation.to_string(),
            &pin.to_string(),
            &status,
            &review_hash,
            &observation_hash,
            &pin_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_followup_receipt(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_followup_receipt_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_followup_receipt", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_followup_receipt',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn event_followup_receipt_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_followup_receipt' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    Ok(actual == event_followup_receipt_payload(conn, id)?)
}

fn event_fixture_pin_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (domain,replay,version,digest,actor,time):(String,i64,String,String,String,String)=conn.query_row("SELECT domain,replay_id,pack_version,pack_digest,actor,created_unix_ns FROM event_fixture_pins WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(|e|e.to_string())?;
    if !event_replay_binding_matches(conn, replay)? {
        return Err("missing replay binding".into());
    }
    let (run_version,run_digest,run_hash):(String,String,String)=conn.query_row("SELECT r.pack_version,r.pack_digest,e.hash FROM event_replay_runs r JOIN ledger_events e ON e.kind='event_replay' AND e.reference=r.id WHERE r.id=?1",[replay],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    if version != run_version || digest != run_digest {
        return Err("fixture pin mismatch".into());
    }
    Ok((
        encode(&[
            "synthetic-event-fixture-pin-v1",
            &domain,
            &replay.to_string(),
            &version,
            &digest,
            &actor,
            &run_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_fixture_pin(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_fixture_pin_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_fixture_pin", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_fixture_pin',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn event_fixture_pin_binding_matches(conn: &Connection, id: i64) -> Result<bool, String> {
    let mut stmt=conn.prepare("SELECT payload,created_ns FROM ledger_events WHERE kind='event_fixture_pin' AND reference=?1").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([id]).map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let actual: (String, String) = (
        row.get(0).map_err(|e| e.to_string())?,
        row.get(1).map_err(|e| e.to_string())?,
    );
    if rows.next().map_err(|e| e.to_string())?.is_some() {
        return Ok(false);
    }
    Ok(actual == event_fixture_pin_payload(conn, id)?)
}

fn event_champion_transition_payload(
    conn: &Connection,
    id: i64,
) -> Result<(String, String), String> {
    let (domain,previous,next,reference,actor,time,kind):(String,String,String,i64,String,String,String)=conn.query_row("SELECT domain,previous_id,next_id,reference,actor,created_unix_ns,kind FROM event_champion_transitions WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
    Ok((
        encode(&[
            "synthetic-event-champion-v1",
            &domain,
            &previous,
            &next,
            &reference.to_string(),
            &actor,
            &kind,
        ]),
        time,
    ))
}
pub(crate) fn append_event_champion_transition(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_champion_transition_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_champion_transition", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_champion_transition',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_event_champions(conn: &Connection) -> Result<bool, String> {
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_champion_transitions t ON e.reference=t.id WHERE e.kind='event_champion_transition' AND t.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut states: std::collections::HashMap<String, (String, i64, i64, String, String)> =
        std::collections::HashMap::new();
    let mut stmt=conn.prepare("SELECT t.id,t.domain,t.previous_id,t.next_id,t.reference,t.actor,t.kind,e.payload,e.created_ns FROM event_champion_transitions t LEFT JOIN ledger_events e ON e.reference=t.id AND e.kind='event_champion_transition' ORDER BY t.id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let domain: String = row.get(1).map_err(|e| e.to_string())?;
        let previous: String = row.get(2).map_err(|e| e.to_string())?;
        let next: String = row.get(3).map_err(|e| e.to_string())?;
        let reference: i64 = row.get(4).map_err(|e| e.to_string())?;
        let actor: String = row.get(5).map_err(|e| e.to_string())?;
        let kind: String = row.get(6).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(7).map_err(|e| e.to_string())?;
        let time: Option<String> = row.get(8).map_err(|e| e.to_string())?;
        let expected = event_champion_transition_payload(conn, id)?;
        if payload.as_deref() != Some(expected.0.as_str())
            || time.as_deref() != Some(expected.1.as_str())
            || actor.trim().is_empty()
        {
            return Ok(false);
        }
        let generation = match states.get(&domain) {
            None if kind == "bootstrap" && previous.is_empty() && reference == 0 => 0,
            Some((old, n, _, _, _))
                if kind == "promote"
                    && old == &previous
                    && next != previous
                    && reference > 0
                    && event_review_binding_matches(conn, reference).unwrap_or(false) =>
            {
                n + 1
            }
            Some((old, n, last, last_previous, last_kind))
                if kind == "rollback"
                    && old == &previous
                    && next == *last_previous
                    && *last == reference
                    && last_kind == "promote" =>
            {
                n + 1
            }
            _ => return Ok(false),
        };
        states.insert(domain, (next, generation, id, previous, kind));
    }
    let mut stmt=conn.prepare("SELECT d.domain,d.champion_id,d.generation,c.artifact,c.runtime,c.abi_version FROM event_champions d JOIN candidates c ON c.id=d.champion_id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut count = 0;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let domain: String = row.get(0).map_err(|e| e.to_string())?;
        let id: String = row.get(1).map_err(|e| e.to_string())?;
        let generation: i64 = row.get(2).map_err(|e| e.to_string())?;
        let artifact: Vec<u8> = row.get(3).map_err(|e| e.to_string())?;
        let runtime: String = row.get(4).map_err(|e| e.to_string())?;
        let abi: i64 = row.get(5).map_err(|e| e.to_string())?;
        if format!("{:x}", Sha256::digest(artifact)) != id
            || runtime != "wasmi-experimental"
            || abi != 3
            || !states
                .get(&domain)
                .is_some_and(|(v, n, _, _, _)| v == &id && *n == generation)
        {
            return Ok(false);
        }
        count += 1;
    }
    Ok(count == states.len())
}

fn event_stream_receipt_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (name,capacity,generation,prior,next,observation,time):(String,i64,i64,Vec<u8>,Vec<u8>,Option<i64>,String)=conn.query_row("SELECT name,capacity,generation,prior,next,observation_id,created_unix_ns FROM event_stream_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
    let evidence = if let Some(obs) = observation {
        if !event_observation_binding_matches(conn, obs)? {
            return Err("missing observation binding".into());
        }
        conn.query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_observation' AND reference=?1",
            [obs],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    Ok((
        encode(&[
            "synthetic-event-stream-v1",
            &name,
            &capacity.to_string(),
            &generation.to_string(),
            &format!("{:x}", Sha256::digest(&prior)),
            &format!("{:x}", Sha256::digest(&next)),
            &observation.map(|v| v.to_string()).unwrap_or_default(),
            &evidence,
        ]),
        time,
    ))
}
pub(crate) fn append_event_stream_receipt(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = event_stream_receipt_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "event_stream_receipt", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_stream_receipt',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_event_streams(conn: &Connection) -> Result<bool, String> {
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN event_stream_receipts r ON e.reference=r.id WHERE e.kind='event_stream_receipt' AND r.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut states: std::collections::HashMap<String, (i64, i64, Vec<u8>)> =
        std::collections::HashMap::new();
    let mut stmt=conn.prepare("SELECT r.id,r.name,r.capacity,r.generation,r.prior,r.next,r.observation_id,e.payload,e.created_ns FROM event_stream_receipts r LEFT JOIN ledger_events e ON e.reference=r.id AND e.kind='event_stream_receipt' ORDER BY r.name,r.generation").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let name: String = row.get(1).map_err(|e| e.to_string())?;
        let capacity: i64 = row.get(2).map_err(|e| e.to_string())?;
        let generation: i64 = row.get(3).map_err(|e| e.to_string())?;
        let prior: Vec<u8> = row.get(4).map_err(|e| e.to_string())?;
        let next: Vec<u8> = row.get(5).map_err(|e| e.to_string())?;
        let observation: Option<i64> = row.get(6).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(7).map_err(|e| e.to_string())?;
        let time: Option<String> = row.get(8).map_err(|e| e.to_string())?;
        let expected = match event_stream_receipt_payload(conn, id) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if payload.as_deref() != Some(expected.0.as_str())
            || time.as_deref() != Some(expected.1.as_str())
            || !(1..=4096).contains(&capacity)
            || prior.len() % 8 != 0
            || next.len() % 8 != 0
            || prior.len() > capacity as usize * 8
            || next.len() > capacity as usize * 8
        {
            return Ok(false);
        }
        match states.get(&name) {
            None if generation == 0
                && prior.is_empty()
                && next.is_empty()
                && observation.is_none() =>
            {
                ()
            }
            Some((old_capacity, old_generation, old_next))
                if *old_capacity == capacity
                    && old_generation.checked_add(1) == Some(generation)
                    && *old_next == prior
                    && observation.is_some() =>
            {
                let json: String = conn
                    .query_row(
                        "SELECT event_json FROM event_observations WHERE id=?1",
                        [observation.unwrap()],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let event: serde_json::Value =
                    serde_json::from_str(&json).map_err(|e| e.to_string())?;
                let metric = event
                    .get("metric")
                    .and_then(|v| v.as_f64())
                    .ok_or("invalid stream metric")?;
                if !metric.is_finite() || !(0.0..=1_000_000_000_000.0).contains(&metric) {
                    return Ok(false);
                }
                let mut computed = prior.clone();
                if computed.len() == capacity as usize * 8 {
                    computed.drain(..8);
                }
                computed.extend_from_slice(&metric.to_bits().to_be_bytes());
                if computed != next {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
        states.insert(name, (capacity, generation, next));
    }
    for (name, (capacity, generation, next)) in states {
        let actual: Option<(i64, i64, Vec<u8>)> = conn
            .query_row(
                "SELECT capacity,generation,latencies FROM event_streams WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if actual != Some((capacity, generation, next)) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn measured_stream_receipt_payload(conn: &Connection, id: i64) -> Result<(String, String), String> {
    let (name,capacity,generation,prior,next,observation,time):(String,i64,i64,Vec<u8>,Vec<u8>,Option<i64>,String)=conn.query_row("SELECT name,capacity,generation,prior,next,observation_id,created_unix_ns FROM measured_stream_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
    let evidence = if let Some(obs) = observation {
        if !event_observation_binding_matches(conn, obs)? {
            return Err("missing observation binding".into());
        }
        conn.query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_observation' AND reference=?1",
            [obs],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    Ok((
        encode(&[
            "synthetic-measured-stream-v2",
            &name,
            &capacity.to_string(),
            &generation.to_string(),
            &format!("{:x}", Sha256::digest(&prior)),
            &format!("{:x}", Sha256::digest(&next)),
            &observation.map(|v| v.to_string()).unwrap_or_default(),
            &evidence,
        ]),
        time,
    ))
}
pub(crate) fn append_measured_stream_receipt(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = measured_stream_receipt_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "measured_stream_receipt", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'measured_stream_receipt',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_measured_streams(conn: &Connection) -> Result<bool, String> {
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN measured_stream_receipts r ON e.reference=r.id WHERE e.kind='measured_stream_receipt' AND r.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut states: std::collections::HashMap<String, (i64, i64, Vec<u8>)> =
        std::collections::HashMap::new();
    let mut stmt=conn.prepare("SELECT r.id,r.name,r.capacity,r.generation,r.prior,r.next,r.observation_id,e.payload,e.created_ns FROM measured_stream_receipts r LEFT JOIN ledger_events e ON e.reference=r.id AND e.kind='measured_stream_receipt' ORDER BY r.name,r.generation").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let name: String = row.get(1).map_err(|e| e.to_string())?;
        let capacity: i64 = row.get(2).map_err(|e| e.to_string())?;
        let generation: i64 = row.get(3).map_err(|e| e.to_string())?;
        let prior: Vec<u8> = row.get(4).map_err(|e| e.to_string())?;
        let next: Vec<u8> = row.get(5).map_err(|e| e.to_string())?;
        let observation: Option<i64> = row.get(6).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(7).map_err(|e| e.to_string())?;
        let time: Option<String> = row.get(8).map_err(|e| e.to_string())?;
        let expected = match measured_stream_receipt_payload(conn, id) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if payload.as_deref() != Some(expected.0.as_str())
            || time.as_deref() != Some(expected.1.as_str())
            || !(1..=4096).contains(&capacity)
        {
            return Ok(false);
        }
        let mut prior_state = match crate::event_state::EventState::decode(&prior) {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        if prior_state.capacity() != capacity as usize {
            return Ok(false);
        }
        match states.get(&name) {
            None if generation == 0
                && observation.is_none()
                && prior == next
                && prior_state.snapshot().count == 0 =>
            {
                ()
            }
            Some((old_capacity, old_generation, old_next))
                if *old_capacity == capacity
                    && old_generation.checked_add(1) == Some(generation)
                    && *old_next == prior
                    && observation.is_some() =>
            {
                let (prior_json,event_json,duration,error):(String,String,i64,Option<String>)=conn.query_row("SELECT prior_json,event_json,champion_us,champion_error FROM event_observations WHERE id=?1",[observation.unwrap()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
                let p: serde_json::Value =
                    serde_json::from_str(&prior_json).map_err(|e| e.to_string())?;
                let event: serde_json::Value =
                    serde_json::from_str(&event_json).map_err(|e| e.to_string())?;
                let snapshot = prior_state.snapshot();
                if p.get("state_semantics").and_then(|v| v.as_str()) != Some("measured-v2")
                    || p.get("count").and_then(|v| v.as_u64()) != Some(snapshot.count)
                    || p.get("mean").and_then(|v| v.as_f64()).map(f64::to_bits)
                        != Some(snapshot.mean.to_bits())
                    || p.get("p99_us").and_then(|v| v.as_u64()) != Some(snapshot.p99_us)
                    || p.get("last_error").and_then(|v| v.as_u64())
                        != Some(snapshot.last_error as u64)
                    || duration < 0
                {
                    return Ok(false);
                }
                let metric = event
                    .get("metric")
                    .and_then(|v| v.as_f64())
                    .ok_or("invalid measured metric")?;
                if prior_state
                    .record(
                        metric,
                        duration as u64,
                        if error.is_some() { 500 } else { 0 },
                    )
                    .is_err()
                    || prior_state.encode() != next
                {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
        states.insert(name, (capacity, generation, next));
    }
    let mut stmt = conn
        .prepare("SELECT name,capacity,generation,state FROM measured_streams")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut count = 0usize;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let name: String = row.get(0).map_err(|e| e.to_string())?;
        let actual: (i64, i64, Vec<u8>) = (
            row.get(1).map_err(|e| e.to_string())?,
            row.get(2).map_err(|e| e.to_string())?,
            row.get(3).map_err(|e| e.to_string())?,
        );
        if states.get(&name) != Some(&actual) {
            return Ok(false);
        }
        count += 1;
    }
    Ok(count == states.len())
}

fn synthetic_event_candidate_payload(
    conn: &Connection,
    id: i64,
) -> Result<(String, String), String> {
    let (candidate,time,artifact,runtime,abi):(String,String,Vec<u8>,String,i64)=conn.query_row("SELECT s.candidate_id,s.created_unix_ns,c.artifact,c.runtime,c.abi_version FROM synthetic_event_candidates s JOIN candidates c ON c.id=s.candidate_id WHERE s.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    if format!("{:x}", Sha256::digest(artifact)) != candidate
        || runtime != "wasmi-experimental"
        || abi != 3
    {
        return Err("invalid synthetic candidate".into());
    }
    Ok((
        encode(&["caller-asserted-synthetic-event-v1", &candidate]),
        time,
    ))
}
pub(crate) fn append_synthetic_event_candidate(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = synthetic_event_candidate_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "synthetic_event_candidate", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'synthetic_event_candidate',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_synthetic_event_candidates(conn: &Connection) -> Result<bool, String> {
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN synthetic_event_candidates s ON s.id=e.reference WHERE e.kind='synthetic_event_candidate' AND s.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT s.id,e.payload,e.created_ns FROM synthetic_event_candidates s LEFT JOIN ledger_events e ON e.kind='synthetic_event_candidate' AND e.reference=s.id ORDER BY s.id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(1).map_err(|e| e.to_string())?;
        let time: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let expected = match synthetic_event_candidate_payload(conn, id) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if payload.as_deref() != Some(expected.0.as_str())
            || time.as_deref() != Some(expected.1.as_str())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn synthetic_window_candidate_payload(
    conn: &Connection,
    id: i64,
) -> Result<(String, String), String> {
    let (candidate,time,artifact,runtime,abi):(String,String,Vec<u8>,String,i64)=conn.query_row("SELECT s.candidate_id,s.created_unix_ns,c.artifact,c.runtime,c.abi_version FROM synthetic_window_candidates s JOIN candidates c ON c.id=s.candidate_id WHERE s.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    if format!("{:x}", Sha256::digest(artifact)) != candidate
        || runtime != "wasmi-experimental"
        || abi != 2
    {
        return Err("invalid synthetic candidate".into());
    }
    Ok((
        encode(&["caller-asserted-synthetic-window-v1", &candidate]),
        time,
    ))
}
pub(crate) fn append_synthetic_window_candidate(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = synthetic_window_candidate_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "synthetic_window_candidate", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'synthetic_window_candidate',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_synthetic_window_candidates(conn: &Connection) -> Result<bool, String> {
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN synthetic_window_candidates s ON s.id=e.reference WHERE e.kind='synthetic_window_candidate' AND s.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT s.id,e.payload,e.created_ns FROM synthetic_window_candidates s LEFT JOIN ledger_events e ON e.kind='synthetic_window_candidate' AND e.reference=s.id ORDER BY s.id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(1).map_err(|e| e.to_string())?;
        let time: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let expected = match synthetic_window_candidate_payload(conn, id) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if payload.as_deref() != Some(expected.0.as_str())
            || time.as_deref() != Some(expected.1.as_str())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn synthetic_scalar_candidate_payload(
    conn: &Connection,
    id: i64,
) -> Result<(String, String), String> {
    let (candidate,time,artifact,runtime,abi):(String,String,Vec<u8>,String,i64)=conn.query_row("SELECT s.candidate_id,s.created_unix_ns,c.artifact,c.runtime,c.abi_version FROM synthetic_scalar_candidates s JOIN candidates c ON c.id=s.candidate_id WHERE s.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    if format!("{:x}", Sha256::digest(artifact)) != candidate
        || runtime != "wasmi-experimental"
        || abi != 1
    {
        return Err("invalid synthetic candidate".into());
    }
    Ok((
        encode(&["caller-asserted-synthetic-scalar-v1", &candidate]),
        time,
    ))
}
pub(crate) fn append_synthetic_scalar_candidate(conn: &Connection, id: i64) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, time) = synthetic_scalar_candidate_payload(conn, id)?;
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, "synthetic_scalar_candidate", id, &payload, &time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'synthetic_scalar_candidate',?2,?3,?4,?5,?6)",
        params![seq, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_synthetic_scalar_candidates(conn: &Connection) -> Result<bool, String> {
    let orphan:i64=conn.query_row("SELECT count(*) FROM ledger_events e LEFT JOIN synthetic_scalar_candidates s ON s.id=e.reference WHERE e.kind='synthetic_scalar_candidate' AND s.id IS NULL",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if orphan != 0 {
        return Ok(false);
    }
    let mut stmt=conn.prepare("SELECT s.id,e.payload,e.created_ns FROM synthetic_scalar_candidates s LEFT JOIN ledger_events e ON e.kind='synthetic_scalar_candidate' AND e.reference=s.id ORDER BY s.id").map_err(|e|e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let payload: Option<String> = row.get(1).map_err(|e| e.to_string())?;
        let time: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        if !seen.insert(id) {
            return Ok(false);
        }
        let expected = match synthetic_scalar_candidate_payload(conn, id) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        if payload.as_deref() != Some(expected.0.as_str())
            || time.as_deref() != Some(expected.1.as_str())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn event_worker_payload(
    conn: &Connection,
    kind: &str,
    id: i64,
) -> Result<(String, String), String> {
    let (table, parent) = match kind {
        "event_replay_worker" => ("event_replay_runs", "event_replay"),
        "event_observation_worker" => ("event_observations", "event_observation"),
        _ => return Err("invalid worker provenance kind".into()),
    };
    let time_column = if kind == "event_replay_worker" {
        "started_unix_ns"
    } else {
        "observed_unix_ns"
    };
    let (path, sha, time): (String, String, String) = conn
        .query_row(
            &format!("SELECT worker_path,worker_sha256,{time_column} FROM {table} WHERE id=?1"),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    if path.is_empty()
        || sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid worker provenance".into());
    }
    let parent_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind=?1 AND reference=?2",
            params![parent, id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok((
        encode(&[
            "synthetic-worker-provenance-v1",
            kind,
            &path,
            &sha,
            &parent_hash,
        ]),
        time,
    ))
}
pub(crate) fn append_event_worker_provenance(
    conn: &Connection,
    kind: &str,
    id: i64,
    time: &str,
) -> Result<(), String> {
    if !verify_conn(conn)? {
        return Err("ledger integrity failure".into());
    }
    let (payload, expected_time) = event_worker_payload(conn, kind, id)?;
    if expected_time != time {
        return Err("worker provenance timestamp mismatch".into());
    }
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (seq, prev) = last
        .map(|(n, h)| (n + 1, h))
        .unwrap_or((1, "GENESIS".into()));
    let digest = hash(&prev, kind, id, &payload, time);
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![seq, kind, id, payload, time, prev, digest],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_event_worker_provenance(conn: &Connection) -> Result<bool, String> {
    for (table, kind) in [
        ("event_replay_runs", "event_replay_worker"),
        ("event_observations", "event_observation_worker"),
    ] {
        let orphans:i64=conn.query_row(&format!("SELECT count(*) FROM ledger_events e LEFT JOIN {table} r ON r.id=e.reference WHERE e.kind=?1 AND r.id IS NULL"),[kind],|r|r.get(0)).map_err(|e|e.to_string())?;
        if orphans != 0 {
            return Ok(false);
        }
        let mut stmt=conn.prepare(&format!("SELECT r.id,r.worker_path,r.worker_sha256,e.payload,e.created_ns FROM {table} r LEFT JOIN ledger_events e ON e.kind=?1 AND e.reference=r.id")).map_err(|e|e.to_string())?;
        let mut rows = stmt.query([kind]).map_err(|e| e.to_string())?;
        let mut seen = std::collections::HashSet::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: i64 = row.get(0).map_err(|e| e.to_string())?;
            let path: Option<String> = row.get(1).map_err(|e| e.to_string())?;
            let sha: Option<String> = row.get(2).map_err(|e| e.to_string())?;
            let payload: Option<String> = row.get(3).map_err(|e| e.to_string())?;
            let time: Option<String> = row.get(4).map_err(|e| e.to_string())?;
            if !seen.insert(id) {
                return Ok(false);
            }
            if path.is_none() && sha.is_none() && payload.is_none() && time.is_none() {
                continue;
            }
            let expected = match event_worker_payload(conn, kind, id) {
                Ok(v) => v,
                Err(_) => return Ok(false),
            };
            if payload.as_deref() != Some(expected.0.as_str())
                || time.as_deref() != Some(expected.1.as_str())
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
