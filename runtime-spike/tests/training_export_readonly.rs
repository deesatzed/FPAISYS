use std::process::Command;

#[test]
fn rejected_training_export_does_not_initialize_existing_empty_database() {
    let path = std::env::temp_dir().join(format!(
        "sese-empty-export-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-training-export",
            path.to_str().unwrap(),
            "sft",
            "1",
            "1",
            "missing-authorities",
            "0",
            "missing-policy",
            "0",
            "-",
            "-",
            "1",
            "0",
            "decline",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(report["status"], "not_exported");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let objects: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, 0);
    assert_eq!(objects, 0);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn read_only_open_rejects_old_schema_and_writes() {
    use sese_runtime_spike::storage::Storage;
    let path = std::env::temp_dir().join(format!(
        "sese-readonly-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE preserved(value TEXT); INSERT INTO preserved VALUES ('unchanged'); PRAGMA user_version=36;").unwrap();
    drop(conn);
    assert!(Storage::open_read_only(&path).is_err());
    let rejected = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-training-export",
            path.to_str().unwrap(),
            "sft",
            "1",
            "1",
            "missing",
            "0",
            "missing",
            "0",
            "-",
            "-",
            "1",
            "0",
            "accept-synthetic-trust-v1",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&rejected.stderr).unwrap();
    assert_eq!(report["status"], "not_exported");
    assert!(report["reason"]
        .as_str()
        .unwrap()
        .contains("schema version 37"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let value: String = conn
        .query_row("SELECT value FROM preserved", [], |r| r.get(0))
        .unwrap();
    assert_eq!((version, value), (36, "unchanged".to_string()));
    drop(conn);
    std::fs::remove_file(&path).unwrap();
    let db = Storage::open(&path).unwrap();
    drop(db);
    let db = Storage::open_read_only(&path).unwrap();
    assert!(db.register(b"synthetic test bytes").is_err());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
