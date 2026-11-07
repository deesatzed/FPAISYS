//! Non-executing typed-event fixture intake.
use crate::event_abi::{Decision, Event, Verdict, Window};
use crate::event_replay::{self, Case};
use serde::Deserialize;

pub const MAX_FIXTURE_BYTES: usize = 1_048_576;

pub fn load(path: impl AsRef<std::path::Path>) -> Result<FixturePack, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("fixture must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_FIXTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    parse(&bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePack {
    schema: String,
    version: String,
    state_semantics: String,
    cases: Vec<FileCase>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCase {
    id: String,
    window: FileWindow,
    event: FileEvent,
    expected: FileVerdict,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileWindow {
    count: u64,
    mean: f64,
    p99_us: u64,
    last_error: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEvent {
    timestamp_ns: u64,
    entity_id: u64,
    metric: f64,
    flags: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileVerdict {
    decision: u32,
    score: f64,
    reason: u32,
}

/// A validated fixture description; parsing it never executes candidate code.
pub struct FixturePack {
    pub version: String,
    pub state_semantics: String,
    pub digest: String,
    pub cases: Vec<Case>,
}

impl FixturePack {
    /// Execute only caller-asserted synthetic candidates; this is not containment.
    pub fn replay_synthetic(
        &self,
        db: &mut crate::storage::Storage,
        worker: &str,
        candidate: &str,
        timeout: std::time::Duration,
    ) -> Result<event_replay::Report, String> {
        match self.state_semantics.as_str() {
            "legacy-v1" => {
                event_replay::replay(db, worker, candidate, &self.version, &self.cases, timeout)
            }
            "measured-v2" => event_replay::replay_measured(
                db,
                worker,
                candidate,
                &self.version,
                &self.cases,
                timeout,
            ),
            _ => Err("unknown state semantics".into()),
        }
    }
}

pub fn parse(bytes: &[u8]) -> Result<FixturePack, String> {
    if bytes.is_empty() || bytes.len() > MAX_FIXTURE_BYTES {
        return Err("invalid fixture file size".into());
    }
    let pack: FilePack =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid fixture JSON: {e}"))?;
    if pack.schema != "sese-event-fixture-file-v1"
        || pack.version.trim().is_empty()
        || pack.version.len() > 256
    {
        return Err("invalid fixture schema or version".into());
    }
    let mut cases = Vec::with_capacity(pack.cases.len());
    for case in pack.cases {
        let decision = match case.expected.decision {
            0 => Decision::Allow,
            1 => Decision::Throttle,
            2 => Decision::Quarantine,
            3 => Decision::Escalate,
            _ => return Err("invalid fixture decision".into()),
        };
        cases.push(Case {
            id: case.id,
            window: Window {
                count: case.window.count,
                mean: case.window.mean,
                p99_us: case.window.p99_us,
                last_error: case.window.last_error,
            },
            event: Event {
                timestamp_ns: case.event.timestamp_ns,
                entity_id: case.event.entity_id,
                metric: case.event.metric,
                flags: case.event.flags,
            },
            expected: Verdict {
                decision,
                score: case.expected.score,
                reason: case.expected.reason,
            },
        });
    }
    let digest = event_replay::pack_digest(&cases, &pack.state_semantics)?;
    Ok(FixturePack {
        version: pack.version,
        state_semantics: pack.state_semantics,
        digest,
        cases,
    })
}
