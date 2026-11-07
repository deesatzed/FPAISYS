use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn synthetic_trace_preserves_asserted_prompt_response_and_explicit_pair() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let db = std::env::temp_dir().join(format!("sese-trace-{suffix}.db"));
    let trace_file = std::env::temp_dir().join(format!("sese-trace-{suffix}.json"));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), db.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    std::fs::write(&trace_file, r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic test","prompt":"Input α","response":"Model reply","preference":{"preferred":"Human choice","rejected":"Model reply","source":"explicit synthetic comparison"}}"#).unwrap();
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let valid = std::fs::read(&trace_file).unwrap();
    for invalid in [
        r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic","prompt":"","response":"reply"}"#,
        r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic","prompt":"prompt","response":"reply","preference":{"preferred":"same","rejected":"same","source":"test"}}"#,
        r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic","prompt":"prompt","response":"reply","preference":{"preferred":"one","source":"test"}}"#,
    ] {
        std::fs::write(&trace_file, invalid).unwrap();
        assert!(!Command::new(cli)
            .args([
                "event-trace-record",
                db.to_str().unwrap(),
                "1",
                trace_file.to_str().unwrap()
            ])
            .output()
            .unwrap()
            .status
            .success());
    }
    std::fs::write(&trace_file, &valid).unwrap();
    assert!(!Command::new(cli)
        .args([
            "event-trace-record",
            db.to_str().unwrap(),
            "999",
            trace_file.to_str().unwrap()
        ])
        .output()
        .unwrap()
        .status
        .success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_trace_event BEFORE INSERT ON ledger_events WHEN NEW.kind='event_training_trace' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    drop(conn);
    assert!(!Command::new(cli)
        .args([
            "event-trace-record",
            db.to_str().unwrap(),
            "1",
            trace_file.to_str().unwrap()
        ])
        .output()
        .unwrap()
        .status
        .success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_training_traces", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    conn.execute_batch("DROP TRIGGER reject_trace_event")
        .unwrap();
    drop(conn);
    let record = Command::new(cli)
        .args([
            "event-trace-record",
            db.to_str().unwrap(),
            "1",
            trace_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        record.status.success(),
        "{}",
        String::from_utf8_lossy(&record.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&record.stdout).unwrap();
    assert_eq!(value["training_ready"], false);
    let id = value["trace_id"].as_i64().unwrap().to_string();
    let inspect = || {
        Command::new(cli)
            .args(["event-trace-inspect", db.to_str().unwrap(), &id])
            .output()
            .unwrap()
    };
    let output = inspect();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["prompt"], "Input α");
    assert_eq!(value["response"], "Model reply");
    assert_eq!(value["preference"]["preferred"], "Human choice");
    assert_eq!(value["preference"]["rejected"], "Model reply");
    assert_eq!(value["provenance"], "caller_asserted_synthetic");
    assert_eq!(value["outcome"], "pending");
    assert_eq!(value["training_ready"], false);
    let eligibility = |trace: &str| {
        let output = Command::new(cli)
            .args(["event-trace-eligibility", db.to_str().unwrap(), trace])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let pending = eligibility(&id);
    assert_eq!(pending["outcome"], "pending");
    for format in ["sft", "dpo", "rlvr"] {
        assert_eq!(pending["formats"][format]["eligible"], false);
    }
    assert!(pending["formats"]["sft"]["blockers"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("outcome_pending")));
    std::fs::write(&trace_file, r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic test","prompt":"Another input","response":"Another response"}"#).unwrap();
    let no_pair = Command::new(cli)
        .args([
            "event-trace-record",
            db.to_str().unwrap(),
            "1",
            trace_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(no_pair.status.success());
    let no_pair: serde_json::Value = serde_json::from_slice(&no_pair.stdout).unwrap();
    let no_pair = Command::new(cli)
        .args([
            "event-trace-inspect",
            db.to_str().unwrap(),
            &no_pair["trace_id"].as_i64().unwrap().to_string(),
        ])
        .output()
        .unwrap();
    let no_pair: serde_json::Value = serde_json::from_slice(&no_pair.stdout).unwrap();
    assert!(no_pair["preference"].is_null());
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts; DROP TABLE event_followup_receipts; DROP TABLE event_followup_pins; PRAGMA user_version=33;")
        .unwrap();
    drop(conn);
    let migrated = inspect();
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let migrated: serde_json::Value = serde_json::from_slice(&migrated.stdout).unwrap();
    assert_eq!(migrated["prompt"], "Input α");
    assert_eq!(migrated["preference"]["preferred"], "Human choice");
    let conn = rusqlite::Connection::open(&db).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 37);
    drop(conn);
    let unpaired = eligibility(&no_pair["id"].as_i64().unwrap().to_string());
    assert!(unpaired["formats"]["dpo"]["blockers"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("preference_pair_missing")));
    let record_response = |response: &str| {
        let trace = serde_json::json!({"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic test","prompt":"Produce WAT","response":response});
        std::fs::write(&trace_file, trace.to_string()).unwrap();
        let output = Command::new(cli)
            .args([
                "event-trace-record",
                db.to_str().unwrap(),
                "1",
                trace_file.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        value["trace_id"].as_i64().unwrap().to_string()
    };
    let matching=record_response("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))");
    let different = record_response("(module)");
    let compare = |trace: &str| {
        Command::new(cli)
            .args(["event-trace-compare-wat", db.to_str().unwrap(), trace])
            .output()
            .unwrap()
    };
    let output = compare(&matching);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let compared: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(compared["status"], "match");
    assert_eq!(compared["executed"], false);
    assert_eq!(compared["training_ready"], false);
    let output = compare(&different);
    assert!(output.status.success());
    let compared: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(compared["status"], "mismatch");
    let output = compare(&id);
    assert!(output.status.success());
    let compared: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(compared["status"], "invalid_wat");
    let conn = rusqlite::Connection::open(&db).unwrap();
    let runs: i64 = conn
        .query_row("SELECT count(*) FROM event_replay_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 1);
    drop(conn);
    assert!(Command::new(cli)
        .args([
            "event-resolve",
            db.to_str().unwrap(),
            "1",
            "validated",
            "operator",
            "synthetic assertion"
        ])
        .output()
        .unwrap()
        .status
        .success());
    let output = inspect();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["outcome"], "validated");
    assert_eq!(value["training_ready"], false);
    let validated = eligibility(&id);
    assert_eq!(validated["outcome"], "validated");
    assert!(validated["formats"]["sft"]["blockers"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("outcome_is_caller_asserted")));
    assert!(validated["formats"]["dpo"]["blockers"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("preference_comparability_unverified")));
    assert!(validated["formats"]["rlvr"]["blockers"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("reward_derivation_missing")));
    let receipt = Command::new(cli)
        .args(["checkpoint", db.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(receipt.status.success());
    let anchor: serde_json::Value = serde_json::from_slice(&receipt.stdout).unwrap();
    let seq = anchor["seq"].as_i64().unwrap().to_string();
    let hash = anchor["hash"].as_str().unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let independently_verify = || {
        Command::new("python3")
            .args([
                script.to_str().unwrap(),
                db.to_str().unwrap(),
                "--seq",
                &seq,
                "--hash",
                hash,
                "--verify-event-traces",
            ])
            .output()
            .unwrap()
    };
    let checked = independently_verify();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let checked: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["event_trace_bindings_checked"], 4);
    assert_eq!(checked["event_review_bindings_checked"], 1);
    assert_eq!(checked["evidence_bindings_checked"], false);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE event_training_traces SET preferred='forged' WHERE id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    assert!(!independently_verify().status.success());
    conn.execute(
        "UPDATE event_training_traces SET preferred='Human choice' WHERE id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    assert!(independently_verify().status.success());
    conn.execute(
        "UPDATE event_training_traces SET response='forged' WHERE id IN (1,2)",
        [],
    )
    .unwrap();
    drop(conn);
    assert!(!inspect().status.success());
    assert!(!independently_verify().status.success());
    assert!(!compare(&matching).status.success());
    assert!(!Command::new(cli)
        .args(["event-trace-eligibility", db.to_str().unwrap(), &id])
        .output()
        .unwrap()
        .status
        .success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE event_training_traces SET response='Model reply' WHERE id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    conn.execute(
        "UPDATE event_training_traces SET response='Another response' WHERE id=2",
        [],
    )
    .unwrap();
    assert!(independently_verify().status.success());
    conn.execute(
        "DELETE FROM event_training_traces WHERE id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    drop(conn);
    assert!(!independently_verify().status.success());
    std::fs::remove_file(db).unwrap();
    std::fs::remove_file(trace_file).unwrap();
}

#[test]
fn falsified_outcome_does_not_satisfy_validated_format_gates() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let db = std::env::temp_dir().join(format!("sese-eligibility-{suffix}.db"));
    let trace = std::env::temp_dir().join(format!("sese-eligibility-{suffix}.json"));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), db.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    std::fs::write(&trace,r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic","prompt":"input","response":"reply"}"#).unwrap();
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    assert!(Command::new(cli)
        .args([
            "event-trace-record",
            db.to_str().unwrap(),
            "1",
            trace.to_str().unwrap()
        ])
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new(cli)
        .args([
            "event-resolve",
            db.to_str().unwrap(),
            "1",
            "falsified",
            "operator",
            "synthetic assertion"
        ])
        .output()
        .unwrap()
        .status
        .success());
    let output = Command::new(cli)
        .args(["event-trace-eligibility", db.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["outcome"], "falsified");
    for format in ["sft", "dpo"] {
        assert!(value["formats"][format]["blockers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("validated_outcome_required")));
    }
    assert_eq!(value["formats"]["rlvr"]["eligible"], false);
    std::fs::remove_file(db).unwrap();
    std::fs::remove_file(trace).unwrap();
}
