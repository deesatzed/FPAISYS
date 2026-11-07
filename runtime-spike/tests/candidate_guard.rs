use sese_runtime_spike::storage::Storage;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sese-origin-{label}-{}-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}
fn register_synthetic(db: &Storage, kind: &str, bytes: &[u8]) -> Result<String, String> {
    match kind {
        "scalar" => db.register_synthetic_scalar(bytes),
        "window" => db.register_synthetic_window(bytes),
        "event" => db.register_synthetic_event(bytes),
        _ => unreachable!(),
    }
}
fn register_untrusted(db: &Storage, kind: &str, bytes: &[u8]) -> Result<String, String> {
    match kind {
        "scalar" => db.register(bytes),
        "window" => db.register_window(bytes),
        "event" => db.register_event(bytes),
        _ => unreachable!(),
    }
}
#[test]
fn candidate_record_and_ledger_event_commit_atomically_for_each_abi() {
    for kind in ["scalar", "window", "event"] {
        let p = path(kind);
        let db = Storage::open(&p).unwrap();
        let conn = rusqlite::Connection::open(&p).unwrap();
        let trigger=format!("CREATE TRIGGER fail_origin BEFORE INSERT ON ledger_events WHEN NEW.kind='synthetic_{kind}_candidate' BEGIN SELECT RAISE(ABORT,'injected'); END;");
        conn.execute_batch(&trigger).unwrap();
        assert!(register_synthetic(&db, kind, b"fixed synthetic bytes").is_err());
        let candidates: i64 = conn
            .query_row("SELECT count(*) FROM candidates", [], |r| r.get(0))
            .unwrap();
        let table = format!("synthetic_{kind}_candidates");
        let origins: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        let events: i64 = conn
            .query_row("SELECT count(*) FROM ledger_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!((candidates, origins, events), (0, 0, 0), "{kind}");
        conn.execute_batch("DROP TRIGGER fail_origin").unwrap();
        assert!(register_synthetic(&db, kind, b"fixed synthetic bytes").is_ok());
        assert!(db.verify_ledger().unwrap());
        drop(conn);
        drop(db);
        std::fs::remove_file(p).unwrap();
    }
}
#[test]
fn forged_or_deleted_synthetic_record_fails_ledger_verification() {
    for kind in ["scalar", "window", "event"] {
        let p = path(kind);
        let db = Storage::open(&p).unwrap();
        let id = register_untrusted(&db, kind, b"not authorized").unwrap();
        let conn = rusqlite::Connection::open(&p).unwrap();
        let table = format!("synthetic_{kind}_candidates");
        conn.execute(
            &format!("INSERT INTO {table}(candidate_id,created_unix_ns) VALUES (?1,'0')"),
            [&id],
        )
        .unwrap();
        assert!(!db.verify_ledger().unwrap(), "{kind}");
        assert!(register_synthetic(&db, kind, b"not authorized").is_err());
        conn.execute(&format!("DELETE FROM {table}"), []).unwrap();
        let legit = register_synthetic(&db, kind, b"fixed synthetic").unwrap();
        conn.execute(
            &format!("DELETE FROM {table} WHERE candidate_id=?1"),
            [legit],
        )
        .unwrap();
        assert!(!db.verify_ledger().unwrap(), "{kind}");
        drop(conn);
        drop(db);
        std::fs::remove_file(p).unwrap();
    }
}

#[test]
fn intake_does_not_change_an_existing_synthetic_identity() {
    for kind in ["scalar", "window", "event"] {
        let p = path(kind);
        let db = Storage::open(&p).unwrap();
        let id = register_synthetic(&db, kind, b"fixed synthetic").unwrap();
        assert_eq!(
            register_untrusted(&db, kind, b"fixed synthetic").unwrap(),
            id
        );
        let conn = rusqlite::Connection::open(&p).unwrap();
        let count: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM synthetic_{kind}_candidates WHERE candidate_id=?1"),
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(db.verify_ledger().unwrap());
        drop(conn);
        drop(db);
        std::fs::remove_file(p).unwrap();
    }
}
