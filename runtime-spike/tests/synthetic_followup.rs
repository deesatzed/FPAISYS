use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn followup_outcome_check_reports_a_contradictory_assertion() {
    let db = std::env::temp_dir().join(format!(
        "sese-followup-contradiction-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db.to_str().unwrap(),
            "--followup-fixture",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/event-followup-synthetic-v1.json"
            ),
            "--followup-case",
            "post-promotion",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = demo.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    for args in [
        vec!["event-followup-record", db.to_str().unwrap(), "1", "2"],
        vec![
            "event-resolve",
            db.to_str().unwrap(),
            "1",
            "falsified",
            "operator",
            "assertion",
        ],
    ] {
        let out = Command::new(cli).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let anchor = Command::new(cli)
        .args(["checkpoint", db.to_str().unwrap()])
        .output()
        .unwrap();
    let anchor: serde_json::Value = serde_json::from_slice(&anchor.stdout).unwrap();
    let result = Command::new(cli)
        .args([
            "event-followup-outcome-check",
            db.to_str().unwrap(),
            "1",
            &anchor["seq"].as_i64().unwrap().to_string(),
            anchor["hash"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["comparison"], "match");
    assert_eq!(result["outcome"], "falsified");
    assert_eq!(result["outcome_matches_comparison"], false);
    assert_eq!(result["training_ready"], false);
    std::fs::remove_file(db).unwrap();
}

#[test]
fn post_promotion_observation_is_checked_against_predeclared_fixture() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let db = std::env::temp_dir().join(format!("sese-followup-{suffix}.db"));
    let fixture = std::env::temp_dir().join(format!("sese-followup-{suffix}.json"));
    let mut pack: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/event-synthetic-v1.json")).unwrap();
    let mut second = pack["cases"][0].clone();
    second["id"] = "followup".into();
    second["window"]["count"] = 1.into();
    second["window"]["mean"] = 3.0.into();
    second["window"]["p99_us"] = 3.into();
    pack["cases"].as_array_mut().unwrap().push(second);
    std::fs::write(&fixture, pack.to_string()).unwrap();
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db.to_str().unwrap(),
            "--fixture",
            fixture.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = demo.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let check = |observation: &str, case: &str| {
        Command::new(cli)
            .args([
                "event-followup-check",
                db.to_str().unwrap(),
                "1",
                observation,
                case,
            ])
            .output()
            .unwrap()
    };
    let output = check("2", "followup");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "match");
    assert_eq!(
        value["evidence_kind"],
        "synthetic_post_promotion_fixture_comparison"
    );
    assert_eq!(value["training_ready"], false);
    let other = check("2", "throttle-example");
    assert!(other.status.success());
    let other: serde_json::Value = serde_json::from_slice(&other.stdout).unwrap();
    assert_eq!(other["status"], "input_mismatch");
    assert!(!check("1", "throttle-example").status.success());
    assert!(!check("3", "followup").status.success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let outcomes: i64 = conn
        .query_row("SELECT count(*) FROM event_review_outcomes", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(outcomes, 0);
    conn.execute(
        "UPDATE event_observations SET champion_json='{}' WHERE id=2",
        [],
    )
    .unwrap();
    drop(conn);
    assert!(!check("2", "followup").status.success());
    std::fs::remove_file(db).unwrap();
    std::fs::remove_file(fixture).unwrap();
}

#[test]
fn separate_followup_case_is_selected_before_review() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let db = std::env::temp_dir().join(format!("sese-followup-pin-{suffix}.db"));
    let fixture = std::env::temp_dir().join(format!("sese-followup-pin-{suffix}.json"));
    let mut pack: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/event-synthetic-v1.json")).unwrap();
    pack["version"] = "followup-v1".into();
    pack["cases"][0]["id"] = "later-case".into();
    pack["cases"][0]["window"]["count"] = 1.into();
    pack["cases"][0]["window"]["mean"] = 3.0.into();
    pack["cases"][0]["window"]["p99_us"] = 3.into();
    std::fs::write(&fixture, pack.to_string()).unwrap();
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db.to_str().unwrap(),
            "--followup-fixture",
            fixture.to_str().unwrap(),
            "--followup-case",
            "later-case",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = demo.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let check = || {
        Command::new(cli)
            .args([
                "event-followup-selected-check",
                db.to_str().unwrap(),
                "1",
                "2",
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
    assert_eq!(value["status"], "match");
    assert_eq!(value["case_id"], "later-case");
    assert_eq!(value["fixture_version"], "followup-v1");
    assert_eq!(
        value["evidence_kind"],
        "synthetic_preselected_separate_fixture_comparison"
    );
    assert_eq!(value["training_ready"], false);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts; DROP TABLE event_followup_receipts; PRAGMA user_version=34;")
        .unwrap();
    drop(conn);
    assert!(check().status.success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let selections: i64 = conn
        .query_row("SELECT count(*) FROM event_followup_pins", [], |r| r.get(0))
        .unwrap();
    assert_eq!((version, selections), (37, 1));
    conn.execute_batch("CREATE TRIGGER reject_followup_receipt BEFORE INSERT ON ledger_events WHEN NEW.kind='event_followup_receipt' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    drop(conn);
    let failed = Command::new(cli)
        .args(["event-followup-record", db.to_str().unwrap(), "1", "2"])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_followup_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    conn.execute_batch("DROP TRIGGER reject_followup_receipt")
        .unwrap();
    drop(conn);
    let recorded = Command::new(cli)
        .args(["event-followup-record", db.to_str().unwrap(), "1", "2"])
        .output()
        .unwrap();
    assert!(
        recorded.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    let recorded: serde_json::Value = serde_json::from_slice(&recorded.stdout).unwrap();
    let receipt = recorded["receipt_id"].as_i64().unwrap().to_string();
    let inspect = || {
        Command::new(cli)
            .args(["event-followup-inspect", db.to_str().unwrap(), &receipt])
            .output()
            .unwrap()
    };
    let inspected = inspect();
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inspected: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected["comparison"]["status"], "match");
    assert_eq!(inspected["comparison"]["records_outcome"], false);
    assert_eq!(inspected["comparison"]["training_ready"], false);
    let duplicate = Command::new(cli)
        .args(["event-followup-record", db.to_str().unwrap(), "1", "2"])
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    let export = || {
        Command::new(cli)
            .args(["event-export", db.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let pending = export();
    assert!(pending.status.success());
    assert!(pending.stdout.is_empty());
    let pending_anchor = Command::new(cli)
        .args(["checkpoint", db.to_str().unwrap()])
        .output()
        .unwrap();
    let pending_anchor: serde_json::Value = serde_json::from_slice(&pending_anchor.stdout).unwrap();
    let pending_check = Command::new(cli)
        .args([
            "event-followup-outcome-check",
            db.to_str().unwrap(),
            &receipt,
            &pending_anchor["seq"].as_i64().unwrap().to_string(),
            pending_anchor["hash"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!pending_check.status.success());
    assert!(pending_check.stdout.is_empty());
    let resolved = Command::new(cli)
        .args([
            "event-resolve",
            db.to_str().unwrap(),
            "1",
            "validated",
            "operator",
            "synthetic assertion",
        ])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    let exported = export();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let exported: serde_json::Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(exported["outcome_evidence"], "caller_asserted");
    assert_eq!(exported["training_ready"], false);
    assert_eq!(
        exported["synthetic_followup_receipts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        exported["synthetic_followup_receipts"][0]["id"],
        receipt.parse::<i64>().unwrap()
    );
    assert_eq!(
        exported["synthetic_followup_receipts"][0]["comparison"]["status"],
        "match"
    );
    assert_eq!(
        exported["synthetic_followup_receipts"][0]["comparison"]["records_outcome"],
        false
    );
    assert_eq!(
        exported["synthetic_followup_receipts"][0]["comparison"]["fixture_version"],
        "followup-v1"
    );
    let anchor = Command::new(cli)
        .args(["checkpoint", db.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(anchor.status.success());
    let anchor: serde_json::Value = serde_json::from_slice(&anchor.stdout).unwrap();
    let seq = anchor["seq"].as_i64().unwrap().to_string();
    let hash = anchor["hash"].as_str().unwrap();
    let outcome_check = |checkpoint_seq: &str, checkpoint_hash: &str| {
        Command::new(cli)
            .args([
                "event-followup-outcome-check",
                db.to_str().unwrap(),
                &receipt,
                checkpoint_seq,
                checkpoint_hash,
            ])
            .output()
            .unwrap()
    };
    let consistency = outcome_check(&seq, hash);
    assert!(
        consistency.status.success(),
        "{}",
        String::from_utf8_lossy(&consistency.stderr)
    );
    let consistency: serde_json::Value = serde_json::from_slice(&consistency.stdout).unwrap();
    assert_eq!(consistency["outcome"], "validated");
    assert_eq!(consistency["comparison"], "match");
    assert_eq!(consistency["outcome_matches_comparison"], true);
    assert_eq!(consistency["real_world_outcome_verified"], false);
    assert_eq!(consistency["training_ready"], false);
    let bad_checkpoint = outcome_check(&seq, &"0".repeat(64));
    assert!(!bad_checkpoint.status.success());
    assert!(bad_checkpoint.stdout.is_empty());
    let old_checkpoint = outcome_check(
        &pending_anchor["seq"].as_i64().unwrap().to_string(),
        pending_anchor["hash"].as_str().unwrap(),
    );
    assert!(!old_checkpoint.status.success());
    assert!(old_checkpoint.stdout.is_empty());
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let independent = || {
        Command::new("python3")
            .args([
                script.to_str().unwrap(),
                db.to_str().unwrap(),
                "--seq",
                &seq,
                "--hash",
                hash,
                "--verify-event-followups",
            ])
            .output()
            .unwrap()
    };
    let result = independent();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let verified: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(verified["event_followup_pins_checked"], 1);
    assert_eq!(verified["event_followup_promotion_gates_checked"], 1);
    assert_eq!(verified["event_followup_receipts_checked"], 1);
    assert_eq!(verified["event_fixture_pins_checked"], 1);
    assert_eq!(verified["evidence_bindings_checked"], false);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let review_replay: i64 = conn
        .query_row("SELECT replay_id FROM event_reviews WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let preview = Command::new(cli)
        .args([
            "event-review-preview",
            db.to_str().unwrap(),
            "typed-cycle",
            &review_replay.to_string(),
        ])
        .output()
        .unwrap();
    assert!(preview.status.success());
    let preview: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["followup"]["case_id"], "later-case");
    assert_eq!(
        preview["followup"]["replay"]["fixture_version"],
        "followup-v1"
    );
    let seqs:(i64,i64)=conn.query_row("SELECT (SELECT seq FROM ledger_events WHERE kind='event_followup_pin'),(SELECT seq FROM ledger_events WHERE kind='event_review' AND reference=1)",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert!(seqs.0 < seqs.1);
    let outcomes: i64 = conn
        .query_row("SELECT count(*) FROM event_review_outcomes", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(outcomes, 1);
    conn.execute(
        "UPDATE event_followup_receipts SET status='verdict_mismatch'",
        [],
    )
    .unwrap();
    assert!(!inspect().status.success());
    assert!(!check().status.success());
    let blocked_record = Command::new(cli)
        .args(["event-followup-record", db.to_str().unwrap(), "1", "2"])
        .output()
        .unwrap();
    assert!(!blocked_record.status.success());
    assert!(String::from_utf8_lossy(&blocked_record.stderr).contains("ledger integrity failure"));
    assert!(!independent().status.success());
    let blocked_export = export();
    assert!(!blocked_export.status.success());
    assert!(blocked_export.stdout.is_empty());
    conn.execute("UPDATE event_followup_receipts SET status='match'", [])
        .unwrap();
    assert!(inspect().status.success());
    assert!(independent().status.success());
    conn.execute("UPDATE event_followup_pins SET case_id='altered'", [])
        .unwrap();
    drop(conn);
    assert!(!check().status.success());
    assert!(!independent().status.success());
    std::fs::remove_file(db).unwrap();
    std::fs::remove_file(fixture).unwrap();
}

#[test]
fn duplicate_inputs_and_late_followup_selection_are_rejected() {
    use sese_runtime_spike::{fixture_file, storage::Storage};
    use std::time::Duration;
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let db_path = std::env::temp_dir().join(format!("sese-followup-late-{suffix}.db"));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nkeep\n")
        .unwrap();
    assert!(demo.wait().unwrap().success());
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let candidate: String = conn
        .query_row(
            "SELECT candidate_id FROM event_reviews WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);
    let mut db = Storage::open(&db_path).unwrap();
    let mut pack: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/event-synthetic-v1.json")).unwrap();
    pack["version"] = "different-version".into();
    pack["cases"][0]["id"] = "renamed".into();
    let duplicate = fixture_file::parse(pack.to_string().as_bytes()).unwrap();
    let run = duplicate
        .replay_synthetic(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            Duration::from_secs(2),
        )
        .unwrap();
    let error = db
        .pin_synthetic_event_followup("typed-cycle", run.run_id, "renamed", "operator")
        .unwrap_err();
    assert!(error.contains("duplicates"), "{error}");
    pack["cases"][0]["id"] = "later-case".into();
    pack["cases"][0]["window"]["count"] = 1.into();
    pack["cases"][0]["window"]["mean"] = 3.0.into();
    pack["cases"][0]["window"]["p99_us"] = 3.into();
    let later = fixture_file::parse(pack.to_string().as_bytes()).unwrap();
    let run = later
        .replay_synthetic(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            Duration::from_secs(2),
        )
        .unwrap();
    db.pin_synthetic_event_followup("typed-cycle", run.run_id, "later-case", "operator")
        .unwrap();
    let error = db
        .check_selected_synthetic_event_followup(1, 2)
        .unwrap_err();
    assert!(
        error.contains("not recorded before the decision"),
        "{error}"
    );
    drop(db);
    std::fs::remove_file(db_path).unwrap();
}

#[test]
fn independent_comparator_distinguishes_synthetic_results_and_signed_zero() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts");
    let script = r#"
import json,sys
sys.path.insert(0,sys.argv[1])
from verify_ledger import _followup_result
j=lambda v:json.dumps(v,separators=(',',':'))
w={'count':0,'mean':0.0,'mean_bits':'0000000000000000','p99_us':0,'last_error':0}
e={'timestamp_ns':1,'entity_id':2,'metric':0.0,'metric_bits':'0000000000000000','flags':0}
x={'decision':1,'score':0.0,'score_bits':'0000000000000000','reason':2}
p={'count':0,'mean':0.0,'p99_us':0,'last_error':0}
o={'timestamp_ns':1,'entity_id':2,'metric':0.0,'flags':0}
a={'decision':'throttle','score':0.0,'reason':2}
check=lambda obs,actual,error:_followup_result(j(w),j(e),j(x),j(p),j(obs),None if actual is None else j(actual),error,'legacy-v1')
assert check(o,a,None)=='match'
assert check(o,{'decision':'allow','score':0.0,'reason':2},None)=='verdict_mismatch'
assert check(o,None,'worker failed')=='worker_error'
assert check(dict(o,metric=-0.0),a,None)=='input_mismatch'
assert _followup_result(j(w),j(e),j(x),j(dict(p,state_semantics='measured-v2')),j(o),j(a),None,'measured-v2')=='match'
"#;
    let result = Command::new("python3")
        .args(["-c", script, directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
