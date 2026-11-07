use std::{
    io::Write,
    process::{Command, Stdio},
};
#[test]
fn event_history_shows_verified_domain_decisions_and_transitions() {
    let path = std::env::temp_dir().join(format!(
        "sese-history-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    assert!(demo.wait().unwrap().success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let history = |domain: &str, seq: &str| {
        Command::new(cli)
            .args(["event-history", path.to_str().unwrap(), domain, seq])
            .output()
            .unwrap()
    };
    let pending = history("typed-cycle", "0");
    assert!(
        pending.status.success(),
        "{}",
        String::from_utf8_lossy(&pending.stderr)
    );
    let pending: serde_json::Value = serde_json::from_slice(&pending.stdout).unwrap();
    let entries = pending["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0]["detail"]["action"], "bootstrap");
    assert_eq!(entries[1]["detail"]["action"], "approve");
    assert_eq!(entries[2]["detail"]["action"], "promote");
    assert_eq!(entries[3]["detail"]["action"], "rollback");
    assert!(pending["next_after_seq"].is_null());
    let small = Command::new(cli)
        .args([
            "event-history",
            path.to_str().unwrap(),
            "typed-cycle",
            "0",
            "2",
        ])
        .output()
        .unwrap();
    assert!(small.status.success());
    let small: serde_json::Value = serde_json::from_slice(&small.stdout).unwrap();
    assert_eq!(small["entries"].as_array().unwrap().len(), 2);
    assert_eq!(small["next_after_seq"], entries[1]["seq"]);
    assert!(!Command::new(cli)
        .args([
            "event-history",
            path.to_str().unwrap(),
            "typed-cycle",
            "0",
            "101"
        ])
        .status()
        .unwrap()
        .success());
    let after = entries[1]["seq"].as_i64().unwrap().to_string();
    let later: serde_json::Value =
        serde_json::from_slice(&history("typed-cycle", &after).stdout).unwrap();
    assert_eq!(later["entries"].as_array().unwrap().len(), 2);
    let other: serde_json::Value = serde_json::from_slice(&history("other", "0").stdout).unwrap();
    assert!(other["entries"].as_array().unwrap().is_empty());
    assert!(!history("typed-cycle", "-1").status.success());
    assert!(Command::new(cli)
        .args([
            "event-resolve",
            path.to_str().unwrap(),
            "1",
            "validated",
            "operator",
            "synthetic assertion"
        ])
        .status()
        .unwrap()
        .success());
    let resolved: serde_json::Value =
        serde_json::from_slice(&history("typed-cycle", "0").stdout).unwrap();
    assert_eq!(resolved["entries"].as_array().unwrap().len(), 5);
    assert_eq!(
        resolved["entries"][4]["detail"]["evidence"],
        "caller_asserted"
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE event_review_outcomes SET source='tampered'", [])
        .unwrap();
    let invalid = history("typed-cycle", "0");
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn independent_verifier_checks_both_stream_receipt_bindings() {
    for measured in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "sese-stream-anchor-{}-{}-{}.db",
            std::process::id(),
            measured,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut command = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"));
        command.args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()]);
        if measured {
            command.arg("--measured");
        }
        let mut demo = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        demo.stdin
            .take()
            .unwrap()
            .write_all(b"approve\nrollback\n")
            .unwrap();
        assert!(demo.wait().unwrap().success());
        let anchor = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args(["checkpoint", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(anchor.status.success());
        let anchor: serde_json::Value = serde_json::from_slice(&anchor.stdout).unwrap();
        let seq = anchor["seq"].as_i64().unwrap().to_string();
        let hash = anchor["hash"].as_str().unwrap();
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
        let check = || {
            Command::new("python3")
                .args([
                    script.to_str().unwrap(),
                    path.to_str().unwrap(),
                    "--seq",
                    &seq,
                    "--hash",
                    hash,
                    "--verify-event-stream-receipts",
                ])
                .output()
                .unwrap()
        };
        let output = check();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["event_streams_without_receipts"], 0);
        let key = if measured {
            "measured_stream_receipt_bindings_checked"
        } else {
            "event_stream_receipt_bindings_checked"
        };
        assert!(value[key].as_u64().unwrap() > 1);
        let table = if measured {
            "measured_stream_receipts"
        } else {
            "event_stream_receipts"
        };
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO event_streams VALUES ('unreceipted',1,0,x'')",
            [],
        )
        .unwrap();
        let with_legacy = check();
        assert!(with_legacy.status.success());
        let legacy: serde_json::Value = serde_json::from_slice(&with_legacy.stdout).unwrap();
        assert_eq!(legacy["event_streams_without_receipts"], 1);
        conn.execute("DELETE FROM event_streams WHERE name='unreceipted'", [])
            .unwrap();
        let stream = if measured {
            "measured_streams"
        } else {
            "event_streams"
        };
        conn.execute(&format!("UPDATE {stream} SET generation=generation+1"), [])
            .unwrap();
        assert!(
            !check().status.success(),
            "changed current stream generation was accepted"
        );
        conn.execute(&format!("UPDATE {stream} SET generation=generation-1"), [])
            .unwrap();
        let original: Vec<u8> = conn
            .query_row(
                &format!("SELECT next FROM {table} WHERE generation=1"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            &format!("UPDATE {table} SET next=x'00' WHERE generation=1"),
            [],
        )
        .unwrap();
        assert!(!check().status.success());
        conn.execute(
            &format!("UPDATE {table} SET next=?1 WHERE generation=1"),
            [&original],
        )
        .unwrap();
        assert!(check().status.success());
        conn.execute(&format!("DELETE FROM {table} WHERE generation=1"), [])
            .unwrap();
        assert!(!check().status.success());
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn independent_verifier_checks_event_champion_history() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-champion-anchor-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    assert!(demo.wait().unwrap().success());
    let receipt = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["checkpoint", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(receipt.status.success());
    let anchor: serde_json::Value = serde_json::from_slice(&receipt.stdout).unwrap();
    let seq = anchor["seq"].as_i64().unwrap().to_string();
    let hash = anchor["hash"].as_str().unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let check = || {
        Command::new("python3")
            .args([
                script.to_str().unwrap(),
                path.to_str().unwrap(),
                "--seq",
                &seq,
                "--hash",
                hash,
                "--verify-event-champions",
            ])
            .output()
            .unwrap()
    };
    let result = check();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["event_champion_transitions_checked"], 3);
    assert_eq!(value["event_champions_checked"], 1);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE event_champions SET generation=generation+1", [])
        .unwrap();
    assert!(!check().status.success());
    conn.execute("UPDATE event_champions SET generation=generation-1", [])
        .unwrap();
    conn.execute(
        "UPDATE event_champion_transitions SET actor='tampered' WHERE kind='promote'",
        [],
    )
    .unwrap();
    assert!(!check().status.success());
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn external_checkpoint_detects_complete_truncation() {
    let path = std::env::temp_dir().join(format!(
        "sese-anchor-{}-{}.db",
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
    let receipt = Command::new(cli)
        .args(["checkpoint", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(receipt.status.success());
    let anchor: serde_json::Value = serde_json::from_slice(&receipt.stdout).unwrap();
    let seq = anchor["seq"].as_i64().unwrap().to_string();
    let hash = anchor["hash"].as_str().unwrap();
    let verify = Command::new(cli)
        .args(["verify-checkpoint", path.to_str().unwrap(), &seq, hash])
        .output()
        .unwrap();
    assert!(verify.status.success());
    let resolved = Command::new(cli)
        .args([
            "window-resolve",
            path.to_str().unwrap(),
            "1",
            "validated",
            "auditor",
            "synthetic outcome",
        ])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    let verify = Command::new(cli)
        .args(["verify-checkpoint", path.to_str().unwrap(), &seq, hash])
        .output()
        .unwrap();
    assert!(verify.status.success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DELETE FROM window_review_outcomes; DELETE FROM window_review_cards; DELETE FROM window_review_records; DELETE FROM ledger_events;").unwrap();
    drop(conn);
    let verify = Command::new(cli)
        .args(["verify-checkpoint", path.to_str().unwrap(), &seq, hash])
        .output()
        .unwrap();
    assert!(!verify.status.success());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_chain_verifier_checks_checkpoint_and_tampering() {
    let path = std::env::temp_dir().join(format!(
        "sese-independent-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    db.register_synthetic_event(b"fixed synthetic artifact")
        .unwrap();
    let (seq, hash) = db.ledger_checkpoint().unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let check = |sequence: String, digest: &str| {
        Command::new("python3")
            .args([
                script.to_str().unwrap(),
                path.to_str().unwrap(),
                "--seq",
                &sequence,
                "--hash",
                digest,
            ])
            .output()
            .unwrap()
    };
    let output = check(seq.to_string(), &hash);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["checkpoint_match"], true);
    assert!(!check((seq + 1).to_string(), &hash).status.success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let created: String = conn
        .query_row(
            "SELECT created_unix_ns FROM synthetic_event_candidates LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE synthetic_event_candidates SET created_unix_ns='0'",
        [],
    )
    .unwrap();
    assert!(check(seq.to_string(), &hash).status.success());
    assert!(!db.verify_ledger().unwrap());
    conn.execute(
        "UPDATE synthetic_event_candidates SET created_unix_ns=?1",
        [created],
    )
    .unwrap();
    conn.execute("UPDATE ledger_events SET payload='altered' WHERE seq=1", [])
        .unwrap();
    assert!(!check(seq.to_string(), &hash).status.success());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_verifier_uses_utf8_byte_lengths_and_accepts_extension() {
    let path = std::env::temp_dir().join(format!(
        "sese-unicode-chain-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut ledger =
        sese_runtime_spike::ledger::Ledger::new(rusqlite::Connection::open(&path).unwrap())
            .unwrap();
    let first = ledger
        .decision("候補", "opérateur", "reject", "人間", "bridge", "proposal")
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE seq=?1",
            [first],
            |r| r.get(0),
        )
        .unwrap();
    ledger
        .decision(
            "second", "operator", "reject", "human", "bridge", "proposal",
        )
        .unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let output = Command::new("python3")
        .args([
            script.to_str().unwrap(),
            path.to_str().unwrap(),
            "--seq",
            &first.to_string(),
            "--hash",
            &hash,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["head_seq"], 2);
    assert_eq!(value["evidence_bindings_checked"], false);
    drop(conn);
    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_verifier_checks_synthetic_candidate_bindings_when_requested() {
    let path = std::env::temp_dir().join(format!(
        "sese-independent-origin-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    db.register_synthetic_scalar(b"scalar fixture").unwrap();
    db.register_synthetic_window(b"window fixture").unwrap();
    db.register_synthetic_event(b"event fixture").unwrap();
    let (seq, hash) = db.ledger_checkpoint().unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let check = || {
        Command::new("python3")
            .args([
                script.to_str().unwrap(),
                path.to_str().unwrap(),
                "--seq",
                &seq.to_string(),
                "--hash",
                &hash,
                "--verify-synthetic-candidates",
            ])
            .output()
            .unwrap()
    };
    let output = check();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["synthetic_candidates_checked"], 3);
    assert_eq!(value["evidence_bindings_checked"], false);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let original_time: String = conn
        .query_row(
            "SELECT created_unix_ns FROM synthetic_event_candidates",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE synthetic_event_candidates SET created_unix_ns='0'",
        [],
    )
    .unwrap();
    assert!(!check().status.success());
    conn.execute(
        "UPDATE synthetic_event_candidates SET created_unix_ns=?1",
        [original_time],
    )
    .unwrap();
    conn.execute("UPDATE candidates SET artifact=x'00' WHERE id=(SELECT candidate_id FROM synthetic_event_candidates)",[]).unwrap();
    assert!(!check().status.success());

    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_verifier_detects_typed_replay_evidence_changes() {
    use sese_runtime_spike::event_abi::{Decision, Event, Verdict, Window};
    use sese_runtime_spike::event_replay::{self, Case};
    let path = std::env::temp_dir().join(format!(
        "sese-independent-replay-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let case = Case {
        id: "case".into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event: Event {
            timestamp_ns: 1,
            entity_id: 2,
            metric: 3.0,
            flags: 4,
        },
        expected: Verdict {
            decision: Decision::Allow,
            score: 3.0,
            reason: 4,
        },
    };
    let report = event_replay::replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v1",
        &[case.clone()],
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    let measured = event_replay::replay_measured(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v2",
        &[case],
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    assert!(measured.passed);
    let zero_case = Case {
        id: "signed-zero".into(),
        window: Window {
            count: 0,
            mean: -0.0,
            p99_us: 0,
            last_error: 0,
        },
        event: Event {
            timestamp_ns: 2,
            entity_id: u64::MAX,
            metric: -0.0,
            flags: 0,
        },
        expected: Verdict {
            decision: Decision::Allow,
            score: 0.0,
            reason: 0,
        },
    };
    let zero = event_replay::replay_measured(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v2",
        &[zero_case],
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    assert!(!zero.passed);
    let failed_case = Case {
        id: "worker-failure".into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event: Event {
            timestamp_ns: 3,
            entity_id: 1,
            metric: 1.0,
            flags: 0,
        },
        expected: Verdict {
            decision: Decision::Allow,
            score: 1.0,
            reason: 0,
        },
    };
    let failed = event_replay::replay(
        &mut db,
        "/usr/bin/false",
        &candidate,
        "v1",
        &[failed_case],
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    assert!(failed.cases[0].error.is_some());
    let failed_inspection = db.inspect_event_replay(failed.run_id).unwrap();
    assert_eq!(failed_inspection["passed"], false);
    assert!(failed_inspection["cases"][0]["error"].is_string());
    assert_eq!(
        failed_inspection["cases"][0]["actual"],
        serde_json::Value::Null
    );
    let passed_inspection = db.inspect_event_replay(report.run_id).unwrap();
    assert_eq!(passed_inspection["passed"], true);
    assert_eq!(passed_inspection["cases"][0]["passed"], true);
    let mut host = sese_runtime_spike::event_host::MeasuredEventHost::new(1).unwrap();
    let observed = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            &candidate,
            &Event {
                timestamp_ns: 4,
                entity_id: 1,
                metric: 1.0,
                flags: 0,
            },
            std::time::Duration::from_secs(2),
        )
        .unwrap();

    let review = db
        .record_event_review(
            &candidate,
            measured.run_id,
            observed.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    db.resolve_event_review(review, "validated", "operator", "synthetic follow-up")
        .unwrap();
    db.record_event_review(
        &candidate,
        measured.run_id,
        observed.id,
        "operator",
        "reject",
        "human",
        "bridge",
        "proposal",
    )
    .unwrap();

    let (seq, hash) = db.ledger_checkpoint().unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let check = || {
        Command::new("python3")
            .args([
                script.to_str().unwrap(),
                path.to_str().unwrap(),
                "--seq",
                &seq.to_string(),
                "--hash",
                &hash,
                "--verify-event-replays",
                "--verify-event-fixtures",
                "--verify-event-workers",
                "--verify-event-observations",
                "--verify-event-reviews",
            ])
            .output()
            .unwrap()
    };
    let output = check();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["event_replay_bindings_checked"], 4);
    assert_eq!(value["event_fixture_cases_checked"], 4);
    assert_eq!(value["event_workers_checked"], 5);
    assert_eq!(value["event_workers_unrecorded"], 0);
    assert_eq!(value["event_observation_bindings_checked"], 1);
    assert_eq!(value["event_review_bindings_checked"], 2);
    assert_eq!(value["event_outcome_bindings_checked"], 1);
    assert_eq!(value["evidence_bindings_checked"], false);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE event_reviews SET proposal='altered' WHERE id=?1",
        [review],
    )
    .unwrap();
    assert!(!check().status.success());
    conn.execute(
        "UPDATE event_reviews SET proposal='proposal' WHERE id=?1",
        [review],
    )
    .unwrap();
    conn.execute(
        "UPDATE event_review_outcomes SET source='altered' WHERE review_id=?1",
        [review],
    )
    .unwrap();
    assert!(!check().status.success());
    conn.execute(
        "UPDATE event_review_outcomes SET source='synthetic follow-up' WHERE review_id=?1",
        [review],
    )
    .unwrap();
    conn.execute(
        "UPDATE event_replay_cases SET passed=0 WHERE run_id=?1",
        [report.run_id],
    )
    .unwrap();
    assert!(!check().status.success());
    let direct=Command::new("python3").args(["-c","import sys,sqlite3;sys.path.insert(0,sys.argv[1]);import verify_ledger;verify_ledger.verify_event_fixture(sqlite3.connect(sys.argv[2]),int(sys.argv[3]))",script.parent().unwrap().to_str().unwrap(),path.to_str().unwrap(),&report.run_id.to_string()]).output().unwrap();
    assert!(!direct.status.success());
    conn.execute(
        "UPDATE event_replay_cases SET passed=1 WHERE run_id=?1",
        [report.run_id],
    )
    .unwrap();
    let worker_sha: String = conn
        .query_row(
            "SELECT worker_sha256 FROM event_observations WHERE id=?1",
            [observed.id],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE event_observations SET worker_sha256=?1 WHERE id=?2",
        rusqlite::params!["0".repeat(64), observed.id],
    )
    .unwrap();
    assert!(!check().status.success());
    conn.execute(
        "UPDATE event_observations SET worker_sha256=?1 WHERE id=?2",
        rusqlite::params![worker_sha, observed.id],
    )
    .unwrap();
    let prior: String = conn
        .query_row(
            "SELECT prior_json FROM event_observations WHERE id=?1",
            [observed.id],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE event_observations SET prior_json='{}' WHERE id=?1",
        [observed.id],
    )
    .unwrap();
    assert!(!check().status.success());
    conn.execute(
        "UPDATE event_observations SET prior_json=?1 WHERE id=?2",
        rusqlite::params![prior, observed.id],
    )
    .unwrap();
    conn.execute(
        "UPDATE event_observations SET domain='tampered' WHERE id=?1",
        [observed.id],
    )
    .unwrap();
    assert!(!check().status.success());

    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_worker_check_reports_legacy_missing_provenance() {
    let path = std::env::temp_dir().join(format!(
        "sese-legacy-worker-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let candidate = db
        .register_synthetic_event(b"fixed synthetic bytes")
        .unwrap();
    let (seq, hash) = db.ledger_checkpoint().unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("INSERT INTO event_replay_runs(candidate_id,pack_version,pack_digest,passed,started_unix_ns,environment,state_semantics) VALUES (?1,'v1','legacy',0,'0','synthetic','legacy-v1')",[candidate]).unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let output = Command::new("python3")
        .args([
            script.to_str().unwrap(),
            path.to_str().unwrap(),
            "--seq",
            &seq.to_string(),
            "--hash",
            &hash,
            "--verify-event-workers",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["event_workers_unrecorded"], 1);
    assert_eq!(value["evidence_bindings_checked"], false);
    let output = Command::new("python3")
        .args([
            script.to_str().unwrap(),
            path.to_str().unwrap(),
            "--seq",
            &seq.to_string(),
            "--hash",
            &hash,
            "--verify-event-replays",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
