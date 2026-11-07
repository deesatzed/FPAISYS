//! Synthetic typed-event replay. This is not an untrusted-code sandbox or promotion gate.
use crate::{
    event_abi::{Decision, Event, Verdict, Window},
    storage::Storage,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub window: Window,
    pub event: Event,
    pub expected: Verdict,
}
#[derive(Debug)]
pub struct CaseResult {
    pub case: Case,
    pub actual: Option<Verdict>,
    pub error: Option<String>,
    pub passed: bool,
}
#[derive(Debug)]
pub struct Report {
    pub run_id: i64,
    pub passed: bool,
    pub cases: Vec<CaseResult>,
}

/// Separate experimental result; its sequence is not an application replay ID.
#[cfg(unix)]
pub struct ContainerReplayReport {
    pub seq: i64,
    pub passed: bool,
}

/// Execute only fixed built-ins in experimental containers and retain their case results.
/// The event is deliberately ineligible for fixture selection and promotion.
#[cfg(unix)]
pub fn replay_container_synthetic(
    db: &mut Storage,
    executor: &crate::event_executor::SyntheticContainerExecutor,
    candidate: &str,
    pack: &crate::fixture_file::FixturePack,
    timeout: Duration,
) -> Result<ContainerReplayReport, String> {
    if pack.version.trim().is_empty()
        || pack.version.len() > 256
        || pack_digest(&pack.cases, &pack.state_semantics)? != pack.digest
    {
        return Err("invalid container fixture".into());
    }
    let artifact = db.artifact_for_abi(candidate, 3)?;
    let checkpoint = db.ledger_checkpoint()?;
    let now = || -> Result<String, String> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string())
    };
    let started = now()?;
    let mut rows = Vec::new();
    let mut failed_execution = false;
    for case in &pack.cases {
        let (actual, error, receipt_path, receipt, started_case, finished_case) =
            if failed_execution {
                (
                    None,
                    Some("not run after earlier worker failure".to_string()),
                    None,
                    None,
                    None,
                    None,
                )
            } else {
                let case_start = now()?;
                let outcome = executor.execute(&artifact, &case.window, &case.event, timeout);
                let case_end = now()?;
                match outcome {
                    Ok(result) => (
                        Some(result.verdict),
                        None,
                        Some(result.receipt_path.to_string_lossy().into_owned()),
                        Some(result.receipt_json),
                        Some(case_start),
                        Some(case_end),
                    ),
                    Err(e) => {
                        failed_execution = true;
                        (None, Some(e), None, None, Some(case_start), Some(case_end))
                    }
                }
            };
        let passed = actual.is_some_and(|v| {
            v.decision == case.expected.decision
                && v.score.to_bits() == case.expected.score.to_bits()
                && v.reason == case.expected.reason
        });
        let w = case.window;
        let e = case.event;
        rows.push(serde_json::json!({"id":case.id,"window":{"count":w.count,"mean":w.mean,"mean_bits":format!("{:016x}",w.mean.to_bits()),"p99_us":w.p99_us,"last_error":w.last_error},
            "event":{"timestamp_ns":e.timestamp_ns,"entity_id":e.entity_id,"metric":e.metric,"metric_bits":format!("{:016x}",e.metric.to_bits()),"flags":e.flags},
            "expected":serde_json::from_str::<serde_json::Value>(&verdict_json(case.expected)).map_err(|e|e.to_string())?,
            "actual":actual.map(|v| serde_json::from_str::<serde_json::Value>(&verdict_json(v)).unwrap()),
            "error":error,"passed":passed,"receipt_path":receipt_path,"receipt":receipt,
            "started_unix_ns":started_case,"finished_unix_ns":finished_case}));
    }
    executor.verify_helpers()?;
    let passed = rows.iter().all(|row| row["passed"] == true);
    let finished = now()?;
    let payload = serde_json::json!({"schema":"sese-container-replay-experiment-v1", "candidate_id":candidate,
        "fixture_version":pack.version,"fixture_digest":pack.digest,"state_semantics":pack.state_semantics,
        "image":executor.image(),"docker_host":executor.host(),"cases":rows,"passed":passed,
        "started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,
        "execution_authenticated":false,"promotion_evidence":false});
    let seq = db.save_container_replay_experiment(
        &payload,
        candidate,
        (checkpoint.0, &checkpoint.1),
        &finished,
    )?;
    Ok(ContainerReplayReport { seq, passed })
}

pub(crate) fn code(d: Decision) -> u32 {
    match d {
        Decision::Allow => 0,
        Decision::Throttle => 1,
        Decision::Quarantine => 2,
        Decision::Escalate => 3,
    }
}
pub(crate) fn verdict_json(v: Verdict) -> String {
    serde_json::json!({"decision":code(v.decision),"score":v.score,"score_bits":format!("{:016x}",v.score.to_bits()),"reason":v.reason}).to_string()
}

pub(crate) fn pack_digest(cases: &[Case], semantics: &str) -> Result<String, String> {
    if cases.is_empty() || cases.len() > 1000 {
        return Err("invalid replay pack".into());
    }
    let mut ids = HashSet::new();
    let mut hash = Sha256::new();
    match semantics {
        "legacy-v1" => hash.update(b"sese-event-pack-v1\0"),
        "measured-v2" => hash.update(b"sese-event-pack-v2-measured\0"),
        _ => return Err("unknown state semantics".into()),
    };
    for c in cases {
        if c.id.trim().is_empty()
            || c.id.len() > 256
            || !ids.insert(&c.id)
            || !c.window.mean.is_finite()
            || !c.event.metric.is_finite()
            || !c.expected.score.is_finite()
        {
            return Err("invalid replay case".into());
        }
        hash.update((c.id.len() as u64).to_be_bytes());
        hash.update(c.id.as_bytes());
        hash.update(c.window.count.to_be_bytes());
        hash.update(c.window.mean.to_bits().to_be_bytes());
        hash.update(c.window.p99_us.to_be_bytes());
        hash.update(c.window.last_error.to_be_bytes());
        hash.update(c.event.timestamp_ns.to_be_bytes());
        hash.update(c.event.entity_id.to_be_bytes());
        hash.update(c.event.metric.to_bits().to_be_bytes());
        hash.update(c.event.flags.to_be_bytes());
        hash.update(code(c.expected.decision).to_be_bytes());
        hash.update(c.expected.score.to_bits().to_be_bytes());
        hash.update(c.expected.reason.to_be_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn replay(
    db: &mut Storage,
    worker: &str,
    candidate: &str,
    version: &str,
    cases: &[Case],
    timeout: Duration,
) -> Result<Report, String> {
    replay_with_semantics(db, worker, candidate, version, cases, timeout, "legacy-v1")
}
/// Replay fixed synthetic cases under measured-v2 state semantics.
pub fn replay_measured(
    db: &mut Storage,
    worker: &str,
    candidate: &str,
    version: &str,
    cases: &[Case],
    timeout: Duration,
) -> Result<Report, String> {
    replay_with_semantics(
        db,
        worker,
        candidate,
        version,
        cases,
        timeout,
        "measured-v2",
    )
}
fn replay_with_semantics(
    db: &mut Storage,
    worker: &str,
    candidate: &str,
    version: &str,
    cases: &[Case],
    timeout: Duration,
    semantics: &str,
) -> Result<Report, String> {
    if version.trim().is_empty() {
        return Err("invalid replay version".into());
    }
    let digest = pack_digest(cases, semantics)?;
    let artifact = db.artifact_for_abi(candidate, 3)?;
    let executor = crate::event_executor::LocalEventExecutor::capture(worker)?;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string();
    let results: Vec<_> = cases
        .iter()
        .map(|case| {
            let (actual, error) =
                match executor.execute(&artifact, &case.window, &case.event, timeout) {
                    Ok(v) => (Some(v), None),
                    Err(e) => (None, Some(e)),
                };
            let passed = actual.is_some_and(|v| {
                v.decision == case.expected.decision
                    && v.score.to_bits() == case.expected.score.to_bits()
                    && v.reason == case.expected.reason
            });
            CaseResult {
                case: case.clone(),
                actual,
                error,
                passed,
            }
        })
        .collect();
    executor.verify_unchanged()?;
    let passed = results.iter().all(|c| c.passed);
    let run_id = db.save_event_replay(
        candidate,
        version,
        &digest,
        &started,
        &results,
        semantics,
        &executor.identity,
    )?;
    Ok(Report {
        run_id,
        passed,
        cases: results,
    })
}

/// Internal consistency only: the digest is stored alongside the evidence and is not an external anchor.
#[derive(Debug)]
pub struct VerifiedReplay {
    pub candidate_id: String,
    pub pack_version: String,
    pub pack_digest: String,
    pub state_semantics: String,
    pub passed: bool,
    pub cases: usize,
}

fn parse_json(s: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(s).map_err(|_| "invalid replay JSON".into())
}
pub(crate) fn u64_field(v: &serde_json::Value, key: &str) -> Result<u64, String> {
    v.get(key)
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "invalid replay integer".into())
}
pub(crate) fn u32_field(v: &serde_json::Value, key: &str) -> Result<u32, String> {
    u32::try_from(u64_field(v, key)?).map_err(|_| "invalid replay integer".into())
}
pub(crate) fn float_field(v: &serde_json::Value, key: &str, bits_key: &str) -> Result<f64, String> {
    let bits = v
        .get(bits_key)
        .and_then(|x| x.as_str())
        .ok_or("missing float bits")?;
    if bits.len() != 16
        || !bits
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("invalid float bits".into());
    }
    let value = f64::from_bits(u64::from_str_radix(bits, 16).map_err(|_| "invalid float bits")?);
    let shown = v.get(key).and_then(|x| x.as_f64()).ok_or("invalid float")?;
    if !value.is_finite() || value.to_bits() != shown.to_bits() {
        return Err("float representation mismatch".into());
    }
    Ok(value)
}
pub(crate) fn verdict(s: &str) -> Result<Verdict, String> {
    let v = parse_json(s)?;
    if v.as_object().map(|o| o.len()) != Some(4) {
        return Err("invalid verdict".into());
    }
    let decision = match u32_field(&v, "decision")? {
        0 => Decision::Allow,
        1 => Decision::Throttle,
        2 => Decision::Quarantine,
        3 => Decision::Escalate,
        _ => return Err("invalid decision".into()),
    };
    Ok(Verdict {
        decision,
        score: float_field(&v, "score", "score_bits")?,
        reason: u32_field(&v, "reason")?,
    })
}
fn same(a: Verdict, b: Verdict) -> bool {
    a.decision == b.decision && a.score.to_bits() == b.score.to_bits() && a.reason == b.reason
}

pub(crate) type StoredCase = (
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
);
pub(crate) fn verify_cases(
    rows: Vec<StoredCase>,
    expected_digest: &str,
    run_passed: i64,
    semantics: &str,
) -> Result<usize, String> {
    if !matches!(run_passed, 0 | 1) {
        return Err("invalid run result".into());
    }
    let mut cases = Vec::new();
    let mut all_passed = true;
    for (ordinal, id, w, e, expected, actual, error, passed) in rows {
        if ordinal != cases.len() as i64 || !matches!(passed, 0 | 1) {
            return Err("invalid case order or result".into());
        }
        let w = parse_json(&w)?;
        let e = parse_json(&e)?;
        if w.as_object().map(|o| o.len()) != Some(5) || e.as_object().map(|o| o.len()) != Some(5) {
            return Err("invalid replay fields".into());
        }
        let window = Window {
            count: u64_field(&w, "count")?,
            mean: float_field(&w, "mean", "mean_bits")?,
            p99_us: u64_field(&w, "p99_us")?,
            last_error: u32_field(&w, "last_error")?,
        };
        let event = Event {
            timestamp_ns: u64_field(&e, "timestamp_ns")?,
            entity_id: u64_field(&e, "entity_id")?,
            metric: float_field(&e, "metric", "metric_bits")?,
            flags: u32_field(&e, "flags")?,
        };
        let expected = verdict(&expected)?;
        let actual = actual.as_deref().map(verdict).transpose()?;
        if actual.is_some() == error.is_some() || error.as_ref().is_some_and(|e| e.is_empty()) {
            return Err("invalid case outcome".into());
        }
        let calculated = actual.is_some_and(|a| same(a, expected));
        if calculated != (passed == 1) {
            return Err("case result mismatch".into());
        }
        all_passed &= calculated;
        cases.push(Case {
            id,
            window,
            event,
            expected,
        });
    }
    if pack_digest(&cases, semantics)? != expected_digest || all_passed != (run_passed == 1) {
        return Err("replay digest or result mismatch".into());
    }
    Ok(cases.len())
}
