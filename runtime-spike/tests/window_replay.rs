use sese_runtime_spike::{
    replay::{replay_window, WindowCase},
    storage::Storage,
};
use std::time::Duration;
#[test]
fn window_replay_runs_cases_and_retains_input() {
    let path = std::env::temp_dir().join(format!(
        "sese-window-replay-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let code=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 1 i32.wrap_i64 local.get 2 i32.add))").unwrap();
    let id = db.register_synthetic_window(&code).unwrap();
    let cases = [
        WindowCase {
            id: "a".into(),
            window: vec![1, 2],
            input: 4,
            expected: 7,
        },
        WindowCase {
            id: "b".into(),
            window: vec![],
            input: 3,
            expected: 4,
        },
    ];
    let report = replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &id,
        "pack-v1",
        &cases,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!report.passed);
    assert_eq!(report.cases[0].actual, Some(7));
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (snapshot,actual,passed):(Vec<u8>,i32,bool)=conn.query_row("SELECT prior_window,actual,passed FROM window_replay_cases WHERE run_id=?1 AND case_id='a'",[report.run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(
        snapshot,
        [1_i32.to_le_bytes(), 2_i32.to_le_bytes()].concat()
    );
    assert_eq!((actual, passed), (7, true));
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn empty_pack_fails_closed() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    assert!(replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        "missing",
        "v1",
        &[],
        Duration::from_secs(2)
    )
    .is_err());
}
#[test]
fn failed_execution_never_passes_and_wrong_abi_is_rejected() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let trapped=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) unreachable))").unwrap()).unwrap();
    let scalar = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let case = WindowCase {
        id: "one".into(),
        window: vec![1],
        input: 2,
        expected: 3,
    };
    let report = replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &trapped,
        "v1",
        &[case.clone()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!report.passed);
    assert!(report.cases[0].error.is_some());
    let obs = sese_runtime_spike::host::Host::new(4)
        .unwrap()
        .process_window(
            &mut db,
            env!("CARGO_BIN_EXE_window-worker"),
            &trapped,
            &trapped,
            2,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(db
        .record_window_review(&trapped, report.run_id, obs.id, "operator", "approve")
        .is_err());
    assert!(db
        .record_window_review(&trapped, report.run_id, obs.id, "operator", "reject")
        .is_ok());
    assert!(replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &scalar,
        "v1",
        &[case.clone()],
        Duration::from_secs(2)
    )
    .is_err());
    assert!(replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &trapped,
        "v1",
        &[case.clone(), case],
        Duration::from_secs(2)
    )
    .is_err());
}
#[test]
fn window_review_requires_matching_passed_evidence() {
    use sese_runtime_spike::host::Host;
    let path = std::env::temp_dir().join(format!(
        "sese-window-review-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let champion=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
    let candidate=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) i32.const 7))").unwrap()).unwrap();
    let worker = env!("CARGO_BIN_EXE_window-worker");
    let case = WindowCase {
        id: "a".into(),
        window: vec![1],
        input: 2,
        expected: 7,
    };
    let report = replay_window(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    let obs = Host::new(4)
        .unwrap()
        .process_window(
            &mut db,
            worker,
            &champion,
            &candidate,
            2,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(db
        .record_window_review(&champion, report.run_id, obs.id, "operator", "approve")
        .is_err());
    let review = db
        .record_window_review(&candidate, report.run_id, obs.id, "operator", "approve")
        .unwrap();
    assert_eq!(db.window_outcome_status(review).unwrap(), "pending");
    assert!(db.export_window_rlvr_jsonl().unwrap().is_empty());
    let resolved = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "window-resolve",
            path.to_str().unwrap(),
            &review.to_string(),
            "validated",
            "auditor",
            "synthetic observation",
        ])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    assert_eq!(db.window_outcome_status(review).unwrap(), "validated");
    let data = db.export_window_rlvr_jsonl().unwrap();
    let row: serde_json::Value = serde_json::from_str(data.trim()).unwrap();
    assert_eq!(row["review_id"], review);
    assert_eq!(row["schema"], "sese-synthetic-window-outcome-v1");
    assert_eq!(row["fixture_version"], "v1");
    assert_eq!(row["runtime_provenance_verified"], false);
    assert_eq!(row["fixture_digest"].as_str().unwrap().len(), 64);
    assert_eq!(
        row["replay_environment"],
        "runtime unverified; window ABI v1; synthetic"
    );
    assert_eq!(row["reward"], 1);
    assert_eq!(row["outcome_evidence"], "caller_asserted");
    assert_eq!(row["reward_derivation"], "validated=1;falsified=-1");
    assert_eq!(row["training_ready"], false);
    assert!(db
        .resolve_window_review(review, "falsified", "auditor", "duplicate")
        .is_err());
    assert!(db.verify_ledger().unwrap());
    drop(db);
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let status = std::process::Command::new(cli)
        .args(["window-status", path.to_str().unwrap(), &review.to_string()])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(status.stdout, b"validated\n");
    let export = std::process::Command::new(cli)
        .args(["window-export", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(export.status.success());
    let row: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(row["schema"], "sese-synthetic-window-outcome-v1");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let fixture_digest: String = conn
        .query_row(
            "SELECT pack_digest FROM window_replay_runs WHERE id=?1",
            [report.run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row["fixture_digest"], fixture_digest);
    conn.execute(
        "UPDATE window_replay_runs SET pack_digest='altered' WHERE id=?1",
        [report.run_id],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(db.export_window_rlvr_jsonl().is_err());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE window_replay_runs SET pack_digest=?1 WHERE id=?2",
        rusqlite::params![fixture_digest, report.run_id],
    )
    .unwrap();
    conn.execute("UPDATE window_review_outcomes SET source='altered'", [])
        .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE window_review_outcomes SET source='synthetic observation'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE window_replay_cases SET prior_window=x'00000000'",
        [],
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(!db.verify_ledger().unwrap());
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn untrusted_window_intake_cannot_be_replayed_or_reclassified() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap();
    let candidate = db.register_window(&wasm).unwrap();
    let case = WindowCase {
        id: "one".into(),
        window: vec![],
        input: 3,
        expected: 3,
    };
    assert!(replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2)
    )
    .is_err());
    assert!(db.register_synthetic_window(&wasm).is_err());
    assert_eq!(db.artifact(&candidate).unwrap(), wasm);
}
