//! Synthetic scalar replay experiment; not an authorization or promotion gate.
use crate::{storage::Storage, supervise};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub input: i32,
    pub expected: i32,
}
#[derive(Debug)]
pub struct CaseResult {
    pub case: Case,
    pub actual: Option<i32>,
    pub error: Option<String>,
    pub passed: bool,
}
#[derive(Debug)]
pub struct Report {
    pub run_id: i64,
    pub passed: bool,
    pub cases: Vec<CaseResult>,
}

pub fn replay(
    db: &mut Storage,
    worker: &str,
    candidate: &str,
    version: &str,
    cases: &[Case],
    timeout: Duration,
) -> Result<Report, String> {
    if version.trim().is_empty() || cases.is_empty() || cases.len() > 1000 {
        return Err("invalid replay pack".into());
    }
    let mut ids = HashSet::new();
    let mut hash = Sha256::new();
    hash.update(b"sese-scalar-pack-v1\0");
    for c in cases {
        if c.id.trim().is_empty() || c.id.len() > 256 || !ids.insert(&c.id) {
            return Err("invalid or duplicate case id".into());
        }
        hash.update((c.id.len() as u64).to_be_bytes());
        hash.update(c.id.as_bytes());
        hash.update(c.input.to_be_bytes());
        hash.update(c.expected.to_be_bytes());
    }
    let pack_digest = format!("{:x}", hash.finalize());
    let artifact = db.artifact_for_abi(candidate, 1)?;
    let started_at = format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let results: Vec<_> = cases
        .iter()
        .map(|case| {
            let outcome = supervise(worker, &artifact, case.input, timeout);
            let (actual, error) = match outcome {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(e)),
            };
            CaseResult {
                case: case.clone(),
                actual,
                error,
                passed: actual == Some(case.expected),
            }
        })
        .collect();
    let passed = results.iter().all(|c| c.passed);
    let run_id = db.save_replay(candidate, version, &pack_digest, &started_at, &results)?;
    Ok(Report {
        run_id,
        passed,
        cases: results,
    })
}

#[derive(Clone, Debug)]
pub struct WindowCase {
    pub id: String,
    pub window: Vec<i32>,
    pub input: i32,
    pub expected: i32,
}
#[derive(Debug)]
pub struct WindowCaseResult {
    pub case: WindowCase,
    pub actual: Option<i32>,
    pub error: Option<String>,
    pub passed: bool,
}
#[derive(Debug)]
pub struct WindowReport {
    pub run_id: i64,
    pub passed: bool,
    pub cases: Vec<WindowCaseResult>,
}

/// Synthetic window replay; not an approval gate or containment boundary.
pub fn replay_window(
    db: &mut Storage,
    worker: &str,
    candidate: &str,
    version: &str,
    cases: &[WindowCase],
    timeout: Duration,
) -> Result<WindowReport, String> {
    if version.trim().is_empty() || cases.is_empty() || cases.len() > 1000 {
        return Err("invalid replay pack".into());
    }
    let mut ids = HashSet::new();
    let mut hash = Sha256::new();
    hash.update(b"sese-window-pack-v1\0");
    for case in cases {
        if case.id.trim().is_empty()
            || case.id.len() > 256
            || !ids.insert(&case.id)
            || case.window.len() > 4096
        {
            return Err("invalid replay case".into());
        }
        hash.update((case.id.len() as u64).to_be_bytes());
        hash.update(case.id.as_bytes());
        hash.update((case.window.len() as u64).to_be_bytes());
        for value in &case.window {
            hash.update(value.to_be_bytes());
        }
        hash.update(case.input.to_be_bytes());
        hash.update(case.expected.to_be_bytes());
    }
    let digest = format!("{:x}", hash.finalize());
    let artifact = db.artifact_for_abi(candidate, 2)?;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string();
    let results: Vec<_> = cases
        .iter()
        .map(|case| {
            let (actual, error) =
                match crate::supervise_window(worker, &artifact, &case.window, case.input, timeout)
                {
                    Ok(v) => (Some(v), None),
                    Err(e) => (None, Some(e)),
                };
            WindowCaseResult {
                case: case.clone(),
                actual,
                error,
                passed: actual == Some(case.expected),
            }
        })
        .collect();
    let passed = results.iter().all(|c| c.passed);
    let run_id = db.save_window_replay(candidate, version, &digest, &started, &results)?;
    Ok(WindowReport {
        run_id,
        passed,
        cases: results,
    })
}
