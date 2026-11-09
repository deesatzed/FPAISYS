use sese_runtime_spike::{
    event_abi::{Decision, Event, Verdict, Window},
    event_replay::{replay, Case},
    storage::Storage,
};
use std::time::Duration;

fn case(id: &str, score: f64) -> Case {
    Case {
        id: id.into(),
        window: Window {
            count: 1,
            mean: 2.0,
            p99_us: 3,
            last_error: 4,
        },
        event: Event {
            timestamp_ns: 5,
            entity_id: 6,
            metric: 7.0,
            flags: 8,
        },
        expected: Verdict {
            decision: Decision::Allow,
            score,
            reason: 8,
        },
    }
}

#[cfg(unix)]
#[test]
fn replay_stops_launching_after_worker_failure_and_records_skipped_cases() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sese-fail-fast-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let marker = dir.join("launches");
    let worker = dir.join("fail-worker.sh");
    let quoted = marker.to_str().unwrap().replace('\'', "'\\''");
    std::fs::write(
        &worker,
        format!("#!/bin/sh\nprintf 'launch\\n' >> '{quoted}'\nexit 7\n"),
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join("db");
    let mut db = Storage::open(&path).unwrap();
    let candidate = db
        .register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("allow").unwrap())
        .unwrap();
    let run = replay(
        &mut db,
        worker.to_str().unwrap(),
        &candidate,
        "v1",
        &[case("first", 7.0), case("second", 7.0), case("third", 7.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!run.passed);
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1);
    let result = db.inspect_event_replay(run.run_id).unwrap();
    assert!(result["cases"][0]["error"].is_string());
    for index in 1..3 {
        assert_eq!(
            result["cases"][index]["error"],
            "not run after earlier worker failure"
        );
        assert!(result["cases"][index]["actual"].is_null());
        assert!(result["case_timings"][index]["started_unix_ns"].is_null());
        assert!(result["case_timings"][index]["finished_unix_ns"].is_null());
        assert!(result["case_timings"][index]["elapsed_ns"].is_null());
    }
    assert!(!db.verify_event_replay(run.run_id).unwrap().passed);
    let checkpoint = db.ledger_checkpoint().unwrap();
    let verification = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/verify_ledger.py"
        ))
        .args([
            path.to_str().unwrap(),
            "--seq",
            &checkpoint.0.to_string(),
            "--hash",
            &checkpoint.1,
            "--verify-event-replays",
        ])
        .output()
        .unwrap();
    assert!(
        verification.status.success(),
        "{}",
        String::from_utf8_lossy(&verification.stderr)
    );
    let verification: serde_json::Value = serde_json::from_slice(&verification.stdout).unwrap();
    assert_eq!(verification["event_replay_timing_bindings_checked"], 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replay_records_case_timestamps_and_durations_for_success_and_failure() {
    let path = std::env::temp_dir().join(format!(
        "sese-case-timing-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let trap = db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) unreachable))").unwrap()).unwrap();
    let report = replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &trap,
        "v1",
        &[case("first", 7.0), case("second", 7.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!report.passed);
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let inspection = db.inspect_event_replay(report.run_id).unwrap();
    let timings = inspection["case_timings"].as_array().unwrap();
    assert_eq!(timings.len(), 2);
    for (index, timing) in timings.iter().enumerate() {
        assert_eq!(timing["case_id"], ["first", "second"][index]);
        if index == 1 {
            assert!(timing["started_unix_ns"].is_null());
            assert_eq!(
                inspection["cases"][index]["error"],
                "not run after earlier worker failure"
            );
            continue;
        }
        assert!(timing["started_unix_ns"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .is_ok());
        assert!(timing["finished_unix_ns"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .is_ok());
        assert!(
            timing["elapsed_ns"]
                .as_str()
                .unwrap()
                .parse::<u128>()
                .unwrap()
                > 0
        );
        assert!(inspection["cases"][index]["error"].is_string());
    }
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replay_timing_preserves_legacy_runs_and_rejects_rehashed_case_mismatch() {
    use sha2::{Digest, Sha256};
    let path = std::env::temp_dir().join(format!(
        "sese-case-timing-compat-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let artifact = sese_runtime_spike::builtin_event::artifact("allow").unwrap();
    let id = db.register_synthetic_event(&artifact).unwrap();
    let run = replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &id,
        "v1",
        &[case("one", 7.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(run.passed);
    let current = db.inspect_event_replay(run.run_id).unwrap();
    assert!(
        current["case_timings"][0]["elapsed_ns"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .unwrap()
            > 0
    );
    assert_eq!(current["cases"][0]["passed"], true);
    let checkpoint = db.ledger_checkpoint().unwrap();
    let verification = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/verify_ledger.py"
        ))
        .args([
            path.to_str().unwrap(),
            "--seq",
            &checkpoint.0.to_string(),
            "--hash",
            &checkpoint.1,
            "--verify-event-replays",
        ])
        .output()
        .unwrap();
    assert!(
        verification.status.success(),
        "{}",
        String::from_utf8_lossy(&verification.stderr)
    );
    let verification: serde_json::Value = serde_json::from_slice(&verification.stdout).unwrap();
    assert_eq!(verification["event_replay_timing_bindings_checked"], 1);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (seq,payload,time,prev,hash): (i64,String,String,String,String) = conn.query_row("SELECT seq,payload,created_ns,prev,hash FROM ledger_events WHERE kind='event_replay_timing' AND reference=?1",[run.run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
    conn.execute("DELETE FROM ledger_events WHERE seq=?1", [seq])
        .unwrap();
    let mut legacy = Storage::open(&path).unwrap();
    assert!(legacy.inspect_event_replay(run.run_id).unwrap()["case_timings"].is_null());
    drop(legacy);
    let legacy_check = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/verify_ledger.py"
        ))
        .args([
            path.to_str().unwrap(),
            "--seq",
            &(seq - 1).to_string(),
            "--hash",
            &prev,
            "--verify-event-replays",
        ])
        .output()
        .unwrap();
    assert!(
        legacy_check.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy_check.stderr)
    );
    let legacy_check: serde_json::Value = serde_json::from_slice(&legacy_check.stdout).unwrap();
    assert_eq!(legacy_check["event_replay_timing_bindings_checked"], 0);
    conn.execute("INSERT INTO ledger_events(seq,kind,reference,payload,created_ns,prev,hash) VALUES (?1,'event_replay_timing',?2,?3,?4,?5,?6)",rusqlite::params![seq,run.run_id,payload,time,prev,hash]).unwrap();
    let mut old_timing: serde_json::Value = serde_json::from_str(&payload).unwrap();
    old_timing["schema"] = serde_json::json!("sese-event-replay-timing-v1");
    let old_timing = old_timing.to_string();
    let reference = run.run_id.to_string();
    let old_encoded = [
        "sese-ledger-v1",
        &prev,
        "event_replay_timing",
        &reference,
        &old_timing,
        &time,
    ]
    .iter()
    .map(|field| format!("{}:{}", field.len(), field))
    .collect::<String>();
    let old_hash = format!("{:x}", Sha256::digest(old_encoded.as_bytes()));
    conn.execute(
        "UPDATE ledger_events SET payload=?1,hash=?2 WHERE seq=?3",
        rusqlite::params![old_timing, old_hash, seq],
    )
    .unwrap();
    let mut old = Storage::open(&path).unwrap();
    assert!(old.inspect_event_replay(run.run_id).unwrap()["case_timings"].is_array());
    drop(old);
    let old_check = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/verify_ledger.py"
        ))
        .args([
            path.to_str().unwrap(),
            "--seq",
            &seq.to_string(),
            "--hash",
            &old_hash,
            "--verify-event-replays",
        ])
        .output()
        .unwrap();
    assert!(
        old_check.status.success(),
        "{}",
        String::from_utf8_lossy(&old_check.stderr)
    );
    let mut bad: serde_json::Value = serde_json::from_str(&payload).unwrap();
    bad["cases"][0]["case_id"] = serde_json::json!("wrong-case");
    let bad = bad.to_string();
    let encoded = [
        "sese-ledger-v1",
        &prev,
        "event_replay_timing",
        &reference,
        &bad,
        &time,
    ]
    .iter()
    .map(|field| format!("{}:{}", field.len(), field))
    .collect::<String>();
    let rewritten_hash = format!("{:x}", Sha256::digest(encoded.as_bytes()));
    conn.execute(
        "UPDATE ledger_events SET payload=?1,hash=?2 WHERE seq=?3",
        rusqlite::params![bad, rewritten_hash, seq],
    )
    .unwrap();
    drop(conn);
    let mut altered = Storage::open(&path).unwrap();
    assert!(altered.inspect_event_replay(run.run_id).is_err());
    drop(altered);
    let result = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/verify_ledger.py"
        ))
        .args([
            path.to_str().unwrap(),
            "--seq",
            &seq.to_string(),
            "--hash",
            &rewritten_hash,
            "--verify-event-replays",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("replay timing case mismatch"));
    std::fs::remove_file(path).unwrap();
}
#[test]
fn persists_ordered_results_and_exact_verdicts() {
    let path = std::env::temp_dir().join(format!(
        "event-replay-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm = wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let report = replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &id,
        "v1",
        &[case("first", 7.0), case("second", 8.0), case("third", 7.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!report.passed);
    assert!(report.cases[0].passed);
    assert!(!report.cases[1].passed);
    assert!(
        report.cases[2].passed,
        "a verdict mismatch must not skip later cases"
    );
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (version, digest, passed): (String, String, bool) = conn
        .query_row(
            "SELECT pack_version,pack_digest,passed FROM event_replay_runs WHERE id=?1",
            [report.run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(version, "v1");
    assert_eq!(digest.len(), 64);
    assert!(!passed);
    let (name, actual): (String, String) = conn
        .query_row(
            "SELECT case_id,actual_json FROM event_replay_cases WHERE run_id=?1 AND ordinal=1",
            [report.run_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name, "second");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap()["score"],
        7.0
    );
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn rejects_invalid_packs_and_wrong_abi() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let scalar = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    assert!(replay(
        &mut db,
        worker,
        &scalar,
        "v1",
        &[case("a", 7.0)],
        Duration::from_secs(2)
    )
    .is_err());
    assert!(replay(
        &mut db,
        worker,
        &scalar,
        "v1",
        &[case("a", 7.0), case("a", 7.0)],
        Duration::from_secs(2)
    )
    .is_err());
    assert!(replay(
        &mut db,
        worker,
        &scalar,
        "v1",
        &[case("a", f64::NAN)],
        Duration::from_secs(2)
    )
    .is_err());
}

#[test]
fn signed_zero_is_distinct_and_traps_are_retained() {
    let path = std::env::temp_dir().join(format!(
        "event-replay-errors-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let zero=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 8))").unwrap()).unwrap();
    let trap=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) unreachable))").unwrap()).unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let neg = replay(
        &mut db,
        worker,
        &zero,
        "v1",
        &[case("zero", -0.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!neg.passed);
    let positive = replay(
        &mut db,
        worker,
        &zero,
        "v1",
        &[case("zero", 0.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(positive.passed);
    let failed = replay(
        &mut db,
        worker,
        &trap,
        "v1",
        &[case("zero", 0.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(failed.cases[0].error.is_some());
    assert!(!db.verify_event_replay(neg.run_id).unwrap().passed);
    assert!(db.verify_event_replay(positive.run_id).unwrap().passed);
    assert!(!db.verify_event_replay(failed.run_id).unwrap().passed);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let digest = |id| {
        conn.query_row(
            "SELECT pack_digest FROM event_replay_runs WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    assert_ne!(digest(neg.run_id), digest(positive.run_id));
    assert_eq!(digest(positive.run_id), digest(failed.run_id));
    let error: Option<String> = conn
        .query_row(
            "SELECT error FROM event_replay_cases WHERE run_id=?1",
            [failed.run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(error.is_some());
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn persisted_replay_verification_detects_fixture_and_result_tampering() {
    let path = std::env::temp_dir().join(format!(
        "event-replay-verify-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let report = replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v1",
        &[case("one", 7.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    let verified = db.verify_event_replay(report.run_id).unwrap();
    assert!(verified.passed);
    assert_eq!(verified.cases, 1);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE event_replay_cases SET actual_json=json_set(actual_json,'$.reason',9) WHERE run_id=?1", [report.run_id]).unwrap();
    assert!(db.verify_event_replay(report.run_id).is_err());
    conn.execute("UPDATE event_replay_cases SET actual_json=json_set(actual_json,'$.reason',8) WHERE run_id=?1", [report.run_id]).unwrap();
    conn.execute(
        "UPDATE event_replay_cases SET passed=0 WHERE run_id=?1",
        [report.run_id],
    )
    .unwrap();
    assert!(db.verify_event_replay(report.run_id).is_err());
    conn.execute(
        "UPDATE event_replay_cases SET passed=1, case_id='changed' WHERE run_id=?1",
        [report.run_id],
    )
    .unwrap();
    assert!(db.verify_event_replay(report.run_id).is_err());
    conn.execute("UPDATE event_replay_cases SET case_id='one', window_json=json_set(window_json,'$.mean',9) WHERE run_id=?1",[report.run_id]).unwrap();
    assert!(db.verify_event_replay(report.run_id).is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ledger_binding_detects_coherent_result_rewrite_and_missing_link() {
    let path = std::env::temp_dir().join(format!(
        "event-ledger-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let report = replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v1",
        &[case("one", 7.0)],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(db.verify_event_replay(report.run_id).unwrap().passed);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE event_replay_cases SET actual_json=json_set(actual_json,'$.reason',9),passed=0 WHERE run_id=?1",[report.run_id]).unwrap();
    conn.execute(
        "UPDATE event_replay_runs SET passed=0 WHERE id=?1",
        [report.run_id],
    )
    .unwrap();
    assert!(db.verify_event_replay(report.run_id).is_err());
    conn.execute("UPDATE event_replay_cases SET actual_json=json_set(actual_json,'$.reason',8),passed=1 WHERE run_id=?1",[report.run_id]).unwrap();
    conn.execute(
        "UPDATE event_replay_runs SET passed=1 WHERE id=?1",
        [report.run_id],
    )
    .unwrap();
    assert!(db.verify_event_replay(report.run_id).is_ok());
    conn.execute(
        "DELETE FROM ledger_events WHERE kind='event_replay' AND reference=?1",
        [report.run_id],
    )
    .unwrap();
    assert!(db.verify_event_replay(report.run_id).is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ledger_write_failure_rolls_back_entire_replay() {
    let path = std::env::temp_dir().join(format!(
        "event-ledger-fail-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    for kind in ["event_replay", "event_replay_timing"] {
        conn.execute_batch(&format!("CREATE TRIGGER reject_event_replay BEFORE INSERT ON ledger_events WHEN NEW.kind='{kind}' BEGIN SELECT RAISE(ABORT,'injected failure'); END;")).unwrap();
        assert!(replay(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            "v1",
            &[case("one", 7.0)],
            Duration::from_secs(2)
        )
        .is_err());
        for table in ["event_replay_runs", "event_replay_cases", "ledger_events"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                count,
                if table == "ledger_events" { 1 } else { 0 },
                "{kind}"
            );
        }
        conn.execute_batch("DROP TRIGGER reject_event_replay")
            .unwrap();
    }
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn untrusted_registration_cannot_be_replayed_or_reclassified_as_synthetic() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_event(&wasm).unwrap();
    assert!(replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v1",
        &[case("one", 7.0)],
        Duration::from_secs(2)
    )
    .is_err());
    assert!(db.register_synthetic_event(&wasm).is_err());
    assert_eq!(db.artifact(&candidate).unwrap(), wasm);
}

#[cfg(unix)]
#[test]
fn changed_worker_file_rejects_replay_without_recording_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!(
        "sese-worker-change-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let path = base.join("store.db");
    let worker = base.join("worker.sh");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&worker,format!("#!/bin/sh\nprintf changed > {}\nprintf '\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000'\n",quote(&worker))).unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    assert!(replay(
        &mut db,
        worker.to_str().unwrap(),
        &id,
        "v1",
        &[case("one", 0.0)],
        Duration::from_secs(2)
    )
    .is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_replay_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    drop(db);
    std::fs::remove_dir_all(base).unwrap();
}
