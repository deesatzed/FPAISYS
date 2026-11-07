use sese_runtime_spike::{
    replay::{replay_window, WindowCase},
    storage::Storage,
};
use std::time::Duration;
#[test]
fn explicit_fixture_pin_matches_exact_pack_and_cannot_be_replaced() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let module=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap();
    let id = db.register_synthetic_window(&module).unwrap();
    let case = WindowCase {
        id: "one".into(),
        window: vec![1],
        input: 2,
        expected: 2,
    };
    let run = replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &id,
        "frozen-v1",
        &[case.clone()],
        Duration::from_secs(2),
    )
    .unwrap();
    let changed = replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &id,
        "frozen-v1",
        &[WindowCase {
            expected: 3,
            ..case
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let (version, digest) = db.window_replay_identity(run.run_id).unwrap();
    db.pin_window_fixture("synthetic-domain", &version, &digest, "operator")
        .unwrap();
    assert!(db
        .window_replay_matches_pin("synthetic-domain", run.run_id)
        .unwrap());
    assert!(!db
        .window_replay_matches_pin("synthetic-domain", changed.run_id)
        .unwrap());
    assert!(db
        .pin_window_fixture("synthetic-domain", &version, &"0".repeat(64), "operator")
        .is_err());
}
#[test]
fn fixture_order_survives_vacuum_and_tampering_is_rejected() {
    let path = std::env::temp_dir().join(format!(
        "sese-pin-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let id=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
    let cases = [
        WindowCase {
            id: "z".into(),
            window: vec![1],
            input: 2,
            expected: 2,
        },
        WindowCase {
            id: "a".into(),
            window: vec![3],
            input: 4,
            expected: 4,
        },
    ];
    let run = replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &id,
        "v1",
        &cases,
        Duration::from_secs(2),
    )
    .unwrap();
    let before = db.window_replay_identity(run.run_id).unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("VACUUM").unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert_eq!(before, db.window_replay_identity(run.run_id).unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE window_replay_cases SET expected=99 WHERE case_id='z'",
        [],
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(db.window_replay_identity(run.run_id).is_err());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn altered_fixture_pin_fails_ledger_verification() {
    let path = std::env::temp_dir().join(format!(
        "sese-pin-ledger-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    db.pin_window_fixture("domain", "v1", &"a".repeat(64), "operator")
        .unwrap();
    assert!(db.verify_ledger().unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE window_fixture_pins SET pack_digest=?1",
        ["b".repeat(64)],
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn legacy_unanchored_pin_is_not_silently_trusted() {
    let path = std::env::temp_dir().join(format!(
        "sese-legacy-pin-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    drop(Storage::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts",
    )
    .unwrap();
    conn.execute_batch("DROP TABLE event_followup_receipts; DROP TABLE event_followup_pins; DROP TABLE event_training_traces; DROP TABLE synthetic_scalar_candidates; DROP TABLE synthetic_window_candidates; DROP TABLE synthetic_event_candidates; DROP TABLE measured_stream_receipts; DROP TABLE measured_streams; DROP TABLE event_stream_receipts; DROP TABLE event_champion_transitions; DROP TABLE event_champions; DROP TABLE event_fixture_pins; DROP TABLE event_review_outcomes; DROP TABLE event_reviews; DROP TABLE event_streams; DROP TABLE event_replay_cases; DROP TABLE event_replay_runs; DROP TABLE event_observations; DROP TABLE champion_transitions; DROP TABLE champion_domains; DROP TABLE window_fixture_pins; CREATE TABLE window_fixture_pins(domain TEXT PRIMARY KEY,pack_version TEXT NOT NULL,pack_digest TEXT NOT NULL,actor TEXT NOT NULL,created_unix_ns TEXT NOT NULL); INSERT INTO window_fixture_pins VALUES('domain','v1','legacy','operator','0'); PRAGMA user_version=14;").unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
