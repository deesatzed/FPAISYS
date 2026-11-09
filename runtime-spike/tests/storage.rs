use sese_runtime_spike::storage::Storage;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn candidate_survives_reopen_and_identity_matches_bytes() {
    let path = std::env::temp_dir().join(format!(
        "sese-storage-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let id = {
        let db = Storage::open(&path).unwrap();
        db.register(b"candidate").unwrap()
    };
    let db = Storage::open(&path).unwrap();
    assert_eq!(db.artifact(&id).unwrap(), b"candidate");
    assert_eq!(id.len(), 64);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn unknown_future_schema_is_rejected() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    assert!(Storage::from_connection(conn).is_err());
}

#[test]
fn different_artifacts_have_different_ids() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let db = Storage::from_connection(conn).unwrap();
    let id = db.register(b"original").unwrap();
    assert_ne!(id, db.register(b"different").unwrap());
}

#[test]
fn unversioned_nonempty_database_is_not_modified() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute("CREATE TABLE unrelated (value TEXT)", [])
        .unwrap();
    assert!(Storage::from_connection(conn).is_err());
}

#[test]
fn migrates_real_v1_database_without_losing_candidate() {
    let path = std::env::temp_dir().join(format!(
        "sese-v1-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bytes = b"prior candidate";
    use sha2::{Digest, Sha256};
    let id = format!("{:x}", Sha256::digest(bytes));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE candidates (id TEXT PRIMARY KEY, artifact BLOB NOT NULL, runtime TEXT NOT NULL, abi_version INTEGER NOT NULL); PRAGMA user_version=1;").unwrap();
    conn.execute(
        "INSERT INTO candidates VALUES (?1,?2,'wasmi-experimental',1)",
        rusqlite::params![id, bytes],
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert_eq!(db.artifact(&id).unwrap(), bytes);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 37);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn corrupt_artifact_is_not_returned() {
    let path = std::env::temp_dir().join(format!(
        "sese-tamper-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = Storage::open(&path).unwrap();
    let id = db.register(b"original").unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE candidates SET artifact=?1 WHERE id=?2",
        rusqlite::params![b"changed".as_slice(), id],
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(db.artifact(&id).is_err());
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn migrates_v32_preserving_candidates_and_creates_trace_table() {
    let path = std::env::temp_dir().join(format!(
        "sese-v32-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = Storage::open(&path).unwrap();
    let candidate = db.register(b"synthetic identity only").unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts; DROP TABLE event_followup_receipts; DROP TABLE event_followup_pins; DROP TABLE event_training_traces; PRAGMA user_version=32;",
    )
    .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert_eq!(db.artifact(&candidate).unwrap(), b"synthetic identity only");
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_training_traces", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!((version, count), (37, 0));
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_migration_rolls_back_version_and_schema() {
    let path = std::env::temp_dir().join(format!(
        "sese-rollback-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE candidates (id TEXT PRIMARY KEY, artifact BLOB NOT NULL, runtime TEXT NOT NULL, abi_version INTEGER NOT NULL); CREATE TABLE replay_cases (x INTEGER); PRAGMA user_version=1;").unwrap();
    drop(conn);
    assert!(Storage::open(&path).is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let created: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='replay_runs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((version, created), (1, 0));
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn existing_v37_database_gets_nonunique_ledger_lookup_index_without_rewriting_records() {
    let path = std::env::temp_dir().join(format!(
        "sese-ledger-index-{}-{}.db",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = Storage::open(&path).unwrap();
    let candidate = db.register(b"preserved candidate").unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DROP INDEX IF EXISTS ledger_kind_reference_lookup;")
        .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert_eq!(db.artifact(&candidate).unwrap(), b"preserved candidate");
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let plan: String = conn.query_row("EXPLAIN QUERY PLAN SELECT payload FROM ledger_events WHERE kind='event_observation' AND reference=?1", [1], |r| r.get(3)).unwrap();
    assert!(plan.contains("ledger_kind_reference_lookup"), "{plan}");
    let unique: i64 = conn.query_row("SELECT [unique] FROM pragma_index_list('ledger_events') WHERE name='ledger_kind_reference_lookup'", [], |r| r.get(0)).unwrap();
    assert_eq!(unique, 0);
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 37);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
