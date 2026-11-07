use sese_runtime_spike::{host::Host, storage::Storage};
use std::time::Duration;
#[test]
fn bounded_window_is_host_owned_and_shadow_never_changes_result() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champion = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let shadow = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) i32.const 99))",
            )
            .unwrap(),
        )
        .unwrap();
    let mut host = Host::new(2).unwrap();
    for value in [1, 2, 3] {
        let result = host
            .process(
                &mut db,
                env!("CARGO_BIN_EXE_worker"),
                &champion,
                &shadow,
                value,
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(result.champion, Some(value));
    }
    assert_eq!(host.window(), vec![2, 3]);
}
#[test]
fn invalid_capacity_is_rejected() {
    assert!(Host::new(0).is_err());
}
#[test]
fn missing_candidate_does_not_advance_window() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let id = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let mut host = Host::new(2).unwrap();
    assert!(host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_worker"),
            &id,
            "missing",
            1,
            Duration::from_secs(2)
        )
        .is_err());
    assert!(host.window().is_empty());
}
#[test]
fn window_worker_uses_prior_state_and_persists_snapshot() {
    let path = std::env::temp_dir().join(format!(
        "sese-window-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let champion=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 1 i32.wrap_i64 local.get 2 i32.add))").unwrap()).unwrap();
    let shadow=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) i32.const 99))").unwrap()).unwrap();
    let mut host = Host::new(2).unwrap();
    let worker = env!("CARGO_BIN_EXE_window-worker");
    assert_eq!(
        host.process_window(
            &mut db,
            worker,
            &champion,
            &shadow,
            3,
            Duration::from_secs(2)
        )
        .unwrap()
        .champion,
        Some(3)
    );
    let result = host
        .process_window(
            &mut db,
            worker,
            &champion,
            &shadow,
            5,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.champion, Some(8));
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let snapshot: Vec<u8> = conn
        .query_row(
            "SELECT prior_window FROM window_observations WHERE id=?1",
            [result.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(snapshot, 3_i32.to_le_bytes());
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn window_and_scalar_artifacts_cannot_be_relabelled() {
    let db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let module = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
    )
    .unwrap();
    db.register_synthetic_scalar(&module).unwrap();
    assert!(db.register_synthetic_window(&module).is_err());
    let window=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap();
    db.register_synthetic_window(&window).unwrap();
    assert!(db.register_synthetic_scalar(&window).is_err());
}
#[test]
fn window_registered_dual_entry_artifact_is_rejected_by_scalar_paths() {
    use sese_runtime_spike::{
        replay::{replay, Case},
        shadow::compare,
    };
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let dual=wat::parse_str("(module (func (export \"evaluate\") (param i32) (result i32) local.get 0) (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap();
    let window = db.register_synthetic_window(&dual).unwrap();
    let scalar = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) i32.const 1))",
            )
            .unwrap(),
        )
        .unwrap();
    let worker = env!("CARGO_BIN_EXE_worker");
    let case = Case {
        id: "one".into(),
        input: 1,
        expected: 1,
    };
    assert!(replay(
        &mut db,
        worker,
        &window,
        "v1",
        &[case],
        Duration::from_secs(2)
    )
    .is_err());
    assert!(compare(&mut db, worker, &scalar, &window, 1, Duration::from_secs(2)).is_err());
    assert!(compare(&mut db, worker, &window, &scalar, 1, Duration::from_secs(2)).is_err());
}
