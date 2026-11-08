use std::process::Command;

#[test]
fn fixture_driven_proposal_infers_a_threshold_without_executing_candidate() {
    let dir = std::env::temp_dir().join(format!(
        "sese-infer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("db");
    drop(sese_runtime_spike::storage::Storage::open(&db).unwrap());
    let fixture = dir.join("training.json");
    let source = serde_json::json!({"schema":"sese-event-fixture-file-v1", "version":"training-v1", "state_semantics":"measured-v2", "cases":[
        {"id":"allow","window":{"count":0,"mean":0.0,"p99_us":0,"last_error":0},"event":{"timestamp_ns":1,"entity_id":1,"metric":2.0,"flags":4},"expected":{"decision":0,"score":2.0,"reason":4}},
        {"id":"throttle","window":{"count":0,"mean":0.0,"p99_us":0,"last_error":0},"event":{"timestamp_ns":2,"entity_id":2,"metric":3.0,"flags":5},"expected":{"decision":1,"score":3.0,"reason":5}}
    ]});
    std::fs::write(&fixture, serde_json::to_vec(&source).unwrap()).unwrap();
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .arg("event-propose-from-fixture")
            .arg(&db)
            .arg(&fixture)
            .output()
            .unwrap()
    };
    let result = call();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let proposal: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(proposal["threshold"], 2.0);
    assert_eq!(proposal["training_cases"], 2);
    assert_eq!(proposal["static_rule_fits_training"], true);
    assert_eq!(proposal["executed"], false);
    assert_eq!(proposal["validated"], false);
    let proposal_seq = proposal["proposal_seq"].as_i64().unwrap();
    assert_eq!(
        proposal["training_fixture_digest"],
        sese_runtime_spike::fixture_file::load(&fixture)
            .unwrap()
            .digest
    );
    let id = proposal["candidate_id"].as_str().unwrap();
    let storage = sese_runtime_spike::storage::Storage::open_read_only(&db).unwrap();
    assert_eq!(
        storage.artifact(id).unwrap(),
        sese_runtime_spike::proposal::threshold_artifact(2.0).unwrap()
    );
    drop(storage);
    let inspected = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .arg("event-proposal-inspect")
        .arg(&db)
        .arg(proposal_seq.to_string())
        .output()
        .unwrap();
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inspected: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected["candidate_id"], id);
    assert_eq!(
        inspected["training_fixture_digest"],
        proposal["training_fixture_digest"]
    );
    assert_eq!(inspected["executed"], false);
    assert_eq!(
        inspected["training_fixture_json"]
            .as_str()
            .unwrap()
            .as_bytes(),
        serde_json::to_vec(&source).unwrap()
    );
    let mut overlapping = source.clone();
    overlapping["cases"][1]["event"]["metric"] = 2.0.into();
    overlapping["cases"][1]["expected"]["score"] = 2.0.into();
    std::fs::write(&fixture, serde_json::to_vec(&overlapping).unwrap()).unwrap();
    assert!(!call().status.success());
    let mut one_class = source.clone();
    one_class["cases"].as_array_mut().unwrap().pop();
    std::fs::write(&fixture, serde_json::to_vec(&one_class).unwrap()).unwrap();
    assert!(!call().status.success());
    for (field, value) in [
        ("score", serde_json::json!(99.0)),
        ("reason", serde_json::json!(99)),
        ("decision", serde_json::json!(2)),
    ] {
        let mut invalid = source.clone();
        invalid["cases"][0]["expected"][field] = value;
        std::fs::write(&fixture, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let result = call();
        assert!(!result.status.success(), "unexpectedly accepted {field}");
        assert!(result.stdout.is_empty());
    }
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (candidates, synthetic, replays): (i64, i64, i64) = conn.query_row("SELECT (SELECT count(*) FROM candidates), (SELECT count(*) FROM synthetic_event_candidates), (SELECT count(*) FROM event_replay_runs)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((candidates, synthetic, replays), (1, 0, 0));
    conn.execute(
        "UPDATE candidates SET artifact=?1 WHERE id=?2",
        rusqlite::params![b"tampered".as_slice(), id],
    )
    .unwrap();
    drop(conn);
    let altered = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .arg("event-proposal-inspect")
        .arg(&db)
        .arg(proposal_seq.to_string())
        .output()
        .unwrap();
    assert!(
        !altered.status.success(),
        "inspection accepted altered candidate bytes"
    );
    assert!(altered.stdout.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fixture_proposal_rolls_back_candidate_if_ledger_write_fails() {
    let dir = std::env::temp_dir().join(format!(
        "sese-proposal-rollback-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("db");
    drop(sese_runtime_spike::storage::Storage::open(&db).unwrap());
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_proposal BEFORE INSERT ON ledger_events WHEN NEW.kind='event_proposal' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    drop(conn);
    let result = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .arg("event-propose-from-fixture")
        .arg(&db)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/threshold-training-synthetic-v1.json"
        ))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let counts: (i64, i64) = conn
        .query_row(
            "SELECT (SELECT count(*) FROM candidates), (SELECT count(*) FROM ledger_events)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 0));
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn local_threshold_proposal_registers_deterministic_unexecuted_wasm() {
    let dir = std::env::temp_dir().join(format!(
        "sese-proposal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("db");
    drop(sese_runtime_spike::storage::Storage::open(&path).unwrap());
    let propose = |threshold: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .arg("event-propose-threshold")
            .arg(&path)
            .arg(threshold)
            .output()
            .unwrap()
    };
    let first = propose("3.5");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["executed"], false);
    assert_eq!(first["validated"], false);
    assert_eq!(first["threshold"], 3.5);
    let id = first["candidate_id"].as_str().unwrap();
    let second: serde_json::Value = serde_json::from_slice(&propose("3.5").stdout).unwrap();
    assert_eq!(second["candidate_id"], id);
    let different: serde_json::Value = serde_json::from_slice(&propose("4.5").stdout).unwrap();
    assert_ne!(different["candidate_id"], id);
    for input in ["NaN", "inf", "1000000000001", "(module)"] {
        let invalid = propose(input);
        assert!(!invalid.status.success());
        assert!(invalid.stdout.is_empty());
    }
    let mut db = sese_runtime_spike::storage::Storage::open_read_only(&path).unwrap();
    let bytes = db.artifact(id).unwrap();
    let pack = sese_runtime_spike::fixture_file::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-measured-synthetic-v1.json"
    ))
    .unwrap();
    let error = pack
        .replay_synthetic(
            &mut db,
            "/nonexistent/worker",
            id,
            std::time::Duration::from_secs(1),
        )
        .unwrap_err();
    assert!(error.contains("not authorized"), "{error}");
    drop(db);
    let engine = wasmi::Engine::default();
    let module = wasmi::Module::new(&engine, &bytes[..]).unwrap();
    assert_eq!(module.imports().count(), 0);
    assert!(module.exports().any(|e| e.name() == "evaluate_event_v1"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (candidates, synthetic, replay): (i64, i64, i64) = conn.query_row("SELECT (SELECT count(*) FROM candidates), (SELECT count(*) FROM synthetic_event_candidates), (SELECT count(*) FROM event_replay_runs)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((candidates, synthetic, replay), (2, 0, 0));
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn candidate_intake_rejects_named_pipe_without_blocking_or_writing() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::time::{Duration, Instant};
    let dir = std::env::temp_dir().join(format!(
        "sese-intake-fifo-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("store.db");
    let fifo = dir.join("candidate.fifo");
    let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    drop(sese_runtime_spike::storage::Storage::open(&path).unwrap());
    for command in ["event-register", "window-register"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .arg(command)
            .arg(&path)
            .arg(&fifo)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let timed_out = loop {
            if child.try_wait().unwrap().is_some() {
                break false;
            }
            if started.elapsed() >= Duration::from_secs(3) {
                child.kill().unwrap();
                break true;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let output = child.wait_with_output().unwrap();
        assert!(!timed_out, "{command} blocked on a named pipe");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("regular file"));
        let conn = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM candidates", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM ledger_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn nonexistent_database_is_not_created() {
    let path = std::env::temp_dir().join(format!(
        "sese-missing-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let status = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["export", path.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!path.exists());
}
#[test]
fn export_empty_database_succeeds_without_fabricating_rows() {
    let path = std::env::temp_dir().join(format!(
        "sese-empty-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    drop(sese_runtime_spike::storage::Storage::open(&path).unwrap());
    let result = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["export", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    std::fs::remove_file(path).unwrap();
}
#[test]
fn resolve_and_export_synthetic_review_across_processes() {
    use std::io::Write;
    use std::process::Stdio;
    let path = std::env::temp_dir().join(format!(
        "sese-outcome-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-demo"))
        .args([env!("CARGO_BIN_EXE_worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let pending = Command::new(cli)
        .args(["status", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert_eq!(pending.stdout, b"pending\n");
    let resolved = Command::new(cli)
        .args([
            "resolve",
            path.to_str().unwrap(),
            "1",
            "falsified",
            "auditor",
            "synthetic observation",
        ])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    let exported = Command::new(cli)
        .args(["export", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(exported.status.success());
    let row: serde_json::Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(row["outcome_source"], "synthetic observation");
    assert_eq!(row["reward"], -1);
    assert_eq!(row["outcome_evidence"], "caller_asserted");
    assert_eq!(row["reward_derivation"], "validated=1;falsified=-1");
    assert_eq!(row["training_ready"], false);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn inspect_window_review_shows_replay_and_prior_window() {
    use std::io::Write;
    use std::process::Stdio;
    let path = std::env::temp_dir().join(format!(
        "sese-inspect-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let result = Command::new(cli)
        .args(["window-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let row: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(row["decision"], "reject");
    assert_eq!(row["outcome"], "pending");
    assert!(row["outcome_detail"].is_null());
    let resolved = Command::new(cli)
        .args([
            "window-resolve",
            path.to_str().unwrap(),
            "1",
            "validated",
            "reviewer",
            "synthetic audit",
        ])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    let inspected = Command::new(cli)
        .args(["window-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    let resolved_row: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(resolved_row["outcome"], "validated");
    assert_eq!(resolved_row["outcome_detail"]["actor"], "reviewer");
    assert_eq!(resolved_row["outcome_detail"]["source"], "synthetic audit");
    assert!(resolved_row["outcome_detail"]["created_unix_ns"]
        .as_str()
        .is_some());
    assert_eq!(row["shadow"]["prior_window"], serde_json::json!([3]));
    assert_eq!(row["cases"][0]["expected"], 8);
    assert_eq!(row["card"]["human"], "Review prior-window influence.");
    assert_eq!(
        row["card"]["proposal"],
        "Add prior-window sum to current input."
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE window_review_cards SET proposal='altered'", [])
        .unwrap();
    drop(conn);
    let invalid = Command::new(cli)
        .args(["window-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE window_review_cards SET proposal='Add prior-window sum to current input.'",
        [],
    )
    .unwrap();
    conn.execute("UPDATE window_replay_cases SET expected=999", [])
        .unwrap();
    drop(conn);
    let invalid = Command::new(cli)
        .args(["window-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    std::fs::remove_file(path).unwrap();
}
#[test]
fn removing_card_and_its_last_event_is_detected() {
    use std::io::Write;
    use std::process::Stdio;
    let path = std::env::temp_dir().join(format!(
        "sese-card-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("DELETE FROM window_review_cards", []).unwrap();
    conn.execute("DELETE FROM ledger_events WHERE kind='window_card'", [])
        .unwrap();
    drop(conn);
    let db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn explicit_fixture_pin_and_match_work_across_processes() {
    use std::io::Write;
    use std::process::Stdio;
    let path = std::env::temp_dir().join(format!(
        "sese-pin-cli-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let inspection = Command::new(cli)
        .args(["window-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    let row: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    let digest = row["replay"]["pack_digest"].as_str().unwrap();
    let version = row["replay"]["pack_version"].as_str().unwrap();
    let pin = Command::new(cli)
        .args([
            "window-pin",
            path.to_str().unwrap(),
            "demo-domain",
            version,
            digest,
            "operator",
        ])
        .output()
        .unwrap();
    assert!(pin.status.success());
    let matched = Command::new(cli)
        .args([
            "window-pin-match",
            path.to_str().unwrap(),
            "demo-domain",
            "1",
        ])
        .output()
        .unwrap();
    assert!(matched.status.success());
    assert_eq!(matched.stdout, b"match\n");
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let code=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap();
    let id = db.register_synthetic_window(&code).unwrap();
    let case = sese_runtime_spike::replay::WindowCase {
        id: "different".into(),
        window: vec![],
        input: 4,
        expected: 4,
    };
    let run = sese_runtime_spike::replay::replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &id,
        version,
        &[case],
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    drop(db);
    let mismatch = Command::new(cli)
        .args([
            "window-pin-match",
            path.to_str().unwrap(),
            "demo-domain",
            &run.run_id.to_string(),
        ])
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    let missing = Command::new(cli)
        .args([
            "window-pin-match",
            path.to_str().unwrap(),
            "other-domain",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    std::fs::remove_file(path).unwrap();
}
#[test]
fn synthetic_champion_commands_require_explicit_evidence_and_support_rollback() {
    use std::io::Write;
    use std::process::Stdio;
    let path = std::env::temp_dir().join(format!(
        "sese-promotion-cli-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"approve\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let p = path.to_str().unwrap();
    let inspection = Command::new(cli)
        .args(["window-inspect", p, "1"])
        .output()
        .unwrap();
    let row: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    let original = row["shadow"]["champion_id"].as_str().unwrap();
    let candidate = row["candidate_id"].as_str().unwrap();
    let version = row["replay"]["pack_version"].as_str().unwrap();
    let digest = row["replay"]["pack_digest"].as_str().unwrap();
    assert!(Command::new(cli)
        .args(["window-pin", p, "domain", version, digest, "operator"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new(cli)
        .args(["window-bootstrap", p, "domain", original, "operator"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new(cli)
        .args(["window-promote", p, "domain", original, "1", "operator"])
        .status()
        .unwrap()
        .success());
    let stale = Command::new(cli)
        .args(["window-promote", p, "domain", original, "1", "operator"])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    let current = Command::new(cli)
        .args(["window-champion", p, "domain"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(current.stdout).unwrap().trim(), candidate);
    assert!(Command::new(cli)
        .args(["window-rollback", p, "domain", candidate, "operator"])
        .status()
        .unwrap()
        .success());
    let current = Command::new(cli)
        .args(["window-champion", p, "domain"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(current.stdout).unwrap().trim(), original);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn modified_review_evidence_blocks_synthetic_promotion() {
    use std::io::Write;
    use std::process::Stdio;
    let path = std::env::temp_dir().join(format!(
        "sese-tamper-promote-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"approve\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let p = path.to_str().unwrap();
    let inspection = Command::new(cli)
        .args(["window-inspect", p, "1"])
        .output()
        .unwrap();
    let row: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    let original = row["shadow"]["champion_id"].as_str().unwrap();
    let v = row["replay"]["pack_version"].as_str().unwrap();
    let d = row["replay"]["pack_digest"].as_str().unwrap();
    assert!(Command::new(cli)
        .args(["window-pin", p, "domain", v, d, "operator"])
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new(cli)
        .args(["window-bootstrap", p, "domain", original, "operator"])
        .output()
        .unwrap()
        .status
        .success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE window_replay_cases SET expected=999", [])
        .unwrap();
    drop(conn);
    let promotion = Command::new(cli)
        .args(["window-promote", p, "domain", original, "1", "operator"])
        .output()
        .unwrap();
    assert!(!promotion.status.success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let current: String = conn
        .query_row(
            "SELECT champion_id FROM champion_domains WHERE domain='domain'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, original);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn window_registration_stores_bytes_without_executing_them() {
    let base = std::env::temp_dir().join(format!(
        "sese-intake-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let dbpath = base.join("store.db");
    let artifact = base.join("sample.wasm");
    drop(sese_runtime_spike::storage::Storage::open(&dbpath).unwrap());
    let bytes=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) (loop br 0) i32.const 0))").unwrap();
    std::fs::write(&artifact, &bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "window-register",
            dbpath.to_str().unwrap(),
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["executed"], false);
    assert_eq!(row["validated"], false);
    let id = row["candidate_id"].as_str().unwrap();
    let mut db = sese_runtime_spike::storage::Storage::open(&dbpath).unwrap();
    assert_eq!(db.artifact(id).unwrap(), bytes);
    let case = sese_runtime_spike::replay::WindowCase {
        id: "intake".into(),
        window: vec![],
        input: 1,
        expected: 1,
    };
    assert!(sese_runtime_spike::replay::replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        id,
        "v1",
        &[case],
        std::time::Duration::from_secs(2)
    )
    .is_err());
    drop(db);
    std::fs::write(&artifact, vec![0_u8; 65_537]).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "window-register",
            dbpath.to_str().unwrap(),
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    std::fs::remove_file(artifact).unwrap();
    std::fs::remove_file(dbpath).unwrap();
    std::fs::remove_dir(base).unwrap();
}

#[test]
fn event_registration_records_bytes_without_execution_or_validation() {
    let base = std::env::temp_dir().join(format!(
        "sese-event-intake-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let dbpath = base.join("store.db");
    let artifact = base.join("candidate.wasm");
    drop(sese_runtime_spike::storage::Storage::open(&dbpath).unwrap());
    let bytes=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) (loop br 0) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    std::fs::write(&artifact, &bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-register",
            dbpath.to_str().unwrap(),
            artifact.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["executed"], false);
    assert_eq!(result["validated"], false);
    assert_eq!(result["abi"], "event-v1");
    let db = sese_runtime_spike::storage::Storage::open(&dbpath).unwrap();
    assert_eq!(
        db.artifact(result["candidate_id"].as_str().unwrap())
            .unwrap(),
        bytes
    );
    drop(db);
    std::fs::write(&artifact, vec![0u8; 65_537]).unwrap();
    assert!(!Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-register",
            dbpath.to_str().unwrap(),
            artifact.to_str().unwrap()
        ])
        .output()
        .unwrap()
        .status
        .success());
    std::fs::remove_file(artifact).unwrap();
    std::fs::remove_file(dbpath).unwrap();
    std::fs::remove_dir(base).unwrap();
}
