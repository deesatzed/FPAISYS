use sese_runtime_spike::storage::Storage;
use std::process::Command;
#[test]
fn file_database_uses_wal_and_discards_uncommitted_crash() {
    let path = std::env::temp_dir().join(format!(
        "sese-crash-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let id=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
    db.pin_window_fixture("domain", "v1", &"a".repeat(64), "operator")
        .unwrap();
    db.bootstrap_champion("domain", &id, "operator").unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    drop(conn);
    let code="import sqlite3,sys,os\nc=sqlite3.connect(sys.argv[1]); c.execute('BEGIN IMMEDIATE'); c.execute(\"UPDATE champion_domains SET generation=99 WHERE domain='domain'\"); os._exit(42)";
    let status = Command::new("python3")
        .args(["-c", code, path.to_str().unwrap()])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(42));
    let db = Storage::open(&path).unwrap();
    assert_eq!(db.current_champion("domain").unwrap(), id);
    assert!(db.verify_ledger().unwrap());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
