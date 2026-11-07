use sese_runtime_spike::{
    replay::{replay, Case},
    storage::Storage,
};
use std::time::Duration;

#[test]
fn caller_supplied_worker_is_not_mislabeled_as_a_verified_runtime() {
    use sese_runtime_spike::replay::{replay_window, WindowCase};
    let path = std::env::temp_dir().join(format!(
        "sese-unverified-worker-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let scalar = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let window = db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
    let a = replay(
        &mut db,
        "/usr/bin/false",
        &scalar,
        "v1",
        &[Case {
            id: "a".into(),
            input: 0,
            expected: 0,
        }],
        Duration::from_secs(1),
    )
    .unwrap();
    let b = replay_window(
        &mut db,
        "/usr/bin/false",
        &window,
        "v1",
        &[WindowCase {
            id: "b".into(),
            window: vec![],
            input: 0,
            expected: 0,
        }],
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(!a.passed && !b.passed);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    for table in ["replay_runs", "window_replay_runs"] {
        let environment: String = conn
            .query_row(&format!("SELECT environment FROM {table}"), [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            environment.contains("runtime unverified"),
            "{table}: {environment}"
        );
        assert!(
            !environment.contains("wasmi-0.40.0"),
            "{table}: {environment}"
        );
    }
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replay_runs_real_cases_and_persists_failure() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let bytes = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
    )
    .unwrap();
    let id = db.register_synthetic_scalar(&bytes).unwrap();
    let cases = vec![
        Case {
            id: "one".into(),
            input: 3,
            expected: 3,
        },
        Case {
            id: "two".into(),
            input: 4,
            expected: 5,
        },
    ];
    let report = replay(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &id,
        "fixture-v1",
        &cases,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!report.passed);
    assert_eq!(report.cases.len(), 2);
    assert_eq!(db.replay_case_count(report.run_id).unwrap(), 2);
}

#[test]
fn empty_or_duplicate_cases_fail_closed() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let id = db.register_synthetic_scalar(b"fixture").unwrap();
    assert!(replay(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &id,
        "v1",
        &[],
        Duration::from_secs(1)
    )
    .is_err());
    let c = Case {
        id: "same".into(),
        input: 1,
        expected: 1,
    };
    assert!(replay(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &id,
        "v1",
        &[c.clone(), c],
        Duration::from_secs(1)
    )
    .is_err());
}

#[test]
fn passing_fixture_and_worker_error_are_distinguished() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let bytes = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
    )
    .unwrap();
    let id = db.register_synthetic_scalar(&bytes).unwrap();
    let cases = [Case {
        id: "ok".into(),
        input: 7,
        expected: 7,
    }];
    let good = replay(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &id,
        "v1",
        &cases,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(good.passed);
    let failed = replay(
        &mut db,
        "/usr/bin/false",
        &id,
        "v1",
        &cases,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!failed.passed);
    assert!(failed.cases[0].error.is_some());
}

#[test]
fn replay_evidence_survives_reopening_database() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let path = std::env::temp_dir().join(format!(
        "sese-replay-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let run_id;
    {
        let mut db = Storage::open(&path).unwrap();
        let bytes = wat::parse_str(
            "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
        )
        .unwrap();
        let id = db.register_synthetic_scalar(&bytes).unwrap();
        let cases = [Case {
            id: "case-1".into(),
            input: 7,
            expected: 8,
        }];
        run_id = replay(
            &mut db,
            env!("CARGO_BIN_EXE_worker"),
            &id,
            "fixture-v1",
            &cases,
            Duration::from_secs(2),
        )
        .unwrap()
        .run_id;
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (version, passed, input, expected, actual): (String, bool, i32, i32, i32) = conn.query_row("SELECT r.pack_version, r.passed, c.input, c.expected, c.actual FROM replay_runs r JOIN replay_cases c ON c.run_id=r.id WHERE r.id=?1", [run_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
    assert_eq!(
        (version.as_str(), passed, input, expected, actual),
        ("fixture-v1", false, 7, 8, 7)
    );
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn scalar_intake_is_not_authorized_for_synthetic_replay() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let wasm = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
    )
    .unwrap();
    let id = db.register(&wasm).unwrap();
    let cases = [Case {
        id: "one".into(),
        input: 1,
        expected: 1,
    }];
    assert!(replay(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &id,
        "v1",
        &cases,
        Duration::from_secs(2)
    )
    .is_err());
    assert!(db.register_synthetic_scalar(&wasm).is_err());
    assert_eq!(db.artifact(&id).unwrap(), wasm);
}
