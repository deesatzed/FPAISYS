use sese_runtime_spike::{host::DurableHost, storage::Storage};
use std::time::Duration;
#[test]
fn restart_restores_window_and_stale_writer_is_rejected() {
    let path = std::env::temp_dir().join(format!(
        "sese-durable-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let code=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 1 i32.wrap_i64 local.get 2 i32.add))").unwrap();
    let id = db.register_synthetic_window(&code).unwrap();
    let mut host = DurableHost::open(&mut db, "stream", 2).unwrap();
    let mut stale = DurableHost::open(&mut db, "stream", 2).unwrap();
    let worker = env!("CARGO_BIN_EXE_window-worker");
    host.process(&mut db, worker, &id, &id, 3, Duration::from_secs(2))
        .unwrap();
    assert!(stale
        .process(&mut db, worker, &id, &id, 8, Duration::from_secs(2))
        .is_err());
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let mut restored = DurableHost::open(&mut db, "stream", 2).unwrap();
    assert_eq!(restored.window(), vec![3]);
    assert!(DurableHost::open(&mut db, "stream", 3).is_err());
    assert_eq!(
        restored
            .process(&mut db, worker, &id, &id, 5, Duration::from_secs(2))
            .unwrap()
            .champion,
        Some(8)
    );
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let observations: i64 = conn
        .query_row("SELECT COUNT(*) FROM window_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(observations, 2);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
