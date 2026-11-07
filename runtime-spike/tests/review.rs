use sese_runtime_spike::{
    replay::{replay, Case},
    shadow::compare,
    storage::Storage,
};
use std::time::Duration;
#[test]
fn approval_requires_matching_passed_replay_and_complete_shadow() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let a = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let b = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) i32.const 4))",
            )
            .unwrap(),
        )
        .unwrap();
    let worker = env!("CARGO_BIN_EXE_worker");
    let report = replay(
        &mut db,
        worker,
        &b,
        "v1",
        &[Case {
            id: "one".into(),
            input: 1,
            expected: 4,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let obs = compare(&mut db, worker, &a, &b, 1, Duration::from_secs(2)).unwrap();
    assert!(db
        .record_review(&a, report.run_id, obs.id, "operator", "approve")
        .is_err());
    assert!(db
        .record_review(&b, 999, obs.id, "operator", "approve")
        .is_err());
    let review = db
        .record_review(&b, report.run_id, obs.id, "operator", "approve")
        .unwrap();
    assert_eq!(db.outcome_status(review).unwrap(), "pending");
    assert!(db.export_rlvr_jsonl().unwrap().is_empty());
    db.resolve_review(review, "validated", "auditor", "synthetic audit")
        .unwrap();
    assert_eq!(db.outcome_status(review).unwrap(), "validated");
    let exported = db.export_rlvr_jsonl().unwrap();
    let row: serde_json::Value = serde_json::from_str(exported.trim()).unwrap();
    assert_eq!(row["review_id"], review);
    assert_eq!(row["reward"], 1);
    assert_eq!(row["outcome_source"], "synthetic audit");
    assert_eq!(row["synthetic"], true);
    assert_eq!(row["runtime_provenance_verified"], false);
    assert_eq!(row["fixture_version"], "v1");
    assert_eq!(row["fixture_digest"].as_str().unwrap().len(), 64);
    assert_eq!(
        row["replay_environment"],
        "runtime unverified; scalar ABI v1; synthetic"
    );
    assert!(db
        .resolve_review(review, "falsified", "auditor", "duplicate")
        .is_err());
    assert!(db.verify_ledger().unwrap());
}
#[test]
fn failed_replay_cannot_be_approved() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let a = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let b = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) i32.const 4))",
            )
            .unwrap(),
        )
        .unwrap();
    let worker = env!("CARGO_BIN_EXE_worker");
    let report = replay(
        &mut db,
        worker,
        &b,
        "v1",
        &[Case {
            id: "one".into(),
            input: 1,
            expected: 5,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let obs = compare(&mut db, worker, &a, &b, 1, Duration::from_secs(2)).unwrap();
    assert!(db
        .record_review(&b, report.run_id, obs.id, "operator", "approve")
        .is_err());
    assert!(db
        .record_review(&b, report.run_id, obs.id, "operator", "reject")
        .is_ok());
}

#[test]
fn review_and_ledger_event_are_committed_together() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let path = std::env::temp_dir().join(format!(
        "sese-review-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let a = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let b = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) i32.const 4))",
            )
            .unwrap(),
        )
        .unwrap();
    let worker = env!("CARGO_BIN_EXE_worker");
    let r = replay(
        &mut db,
        worker,
        &b,
        "v1",
        &[Case {
            id: "one".into(),
            input: 1,
            expected: 4,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let s = compare(&mut db, worker, &a, &b, 1, Duration::from_secs(2)).unwrap();
    let review = db
        .record_review(&b, r.run_id, s.id, "operator", "approve")
        .unwrap();
    db.resolve_review(review, "falsified", "auditor", "synthetic check")
        .unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE review_outcomes SET source='changed'", [])
        .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    assert!(db.export_rlvr_jsonl().is_err());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE review_outcomes SET source='synthetic check'", [])
        .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    let output = db.export_rlvr_jsonl().unwrap();
    let row: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(row["reward"], -1);
    assert_eq!(row["outcome"], "falsified");
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let fixture_digest: String = conn
        .query_row(
            "SELECT pack_digest FROM replay_runs WHERE id=?1",
            [r.run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row["fixture_digest"], fixture_digest);
    conn.execute(
        "UPDATE replay_runs SET pack_digest='altered' WHERE id=?1",
        [r.run_id],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(db.export_rlvr_jsonl().is_err());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE replay_runs SET pack_digest=?1 WHERE id=?2",
        rusqlite::params![fixture_digest, r.run_id],
    )
    .unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ledger_events WHERE kind='review' AND reference=?1",
            [review],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    conn.execute("UPDATE review_records SET actor='altered'", [])
        .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE review_records SET actor='operator'", [])
        .unwrap();
    conn.execute("UPDATE replay_cases SET expected=99", [])
        .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE replay_cases SET expected=4", [])
        .unwrap();
    conn.execute("UPDATE shadow_observations SET shadow_result=42", [])
        .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE shadow_observations SET shadow_result=4", [])
        .unwrap();
    let original: Vec<u8> = conn
        .query_row("SELECT artifact FROM candidates WHERE id=?1", [&a], |row| {
            row.get(0)
        })
        .unwrap();
    conn.execute(
        "UPDATE candidates SET artifact=?1 WHERE id=?2",
        rusqlite::params![b"tamper".as_slice(), a],
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE candidates SET artifact=?1 WHERE id=?2",
        rusqlite::params![original, a],
    )
    .unwrap();
    conn.execute("UPDATE ledger_events SET payload='corrupt'", [])
        .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(db
        .record_review(&b, r.run_id, s.id, "operator", "reject")
        .is_err());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let reviews: i64 = conn
        .query_row("SELECT COUNT(*) FROM review_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reviews, 1);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
