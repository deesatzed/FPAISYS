use sese_runtime_spike::{
    host::{DurableHost, Host},
    replay::{replay_window, WindowCase},
    storage::Storage,
};
use std::time::Duration;
#[test]
fn local_promotion_requires_exact_pin_and_approved_matching_review() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champion=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
    let candidate=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) i32.const 7))").unwrap()).unwrap();
    let worker = env!("CARGO_BIN_EXE_window-worker");
    let run = replay_window(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[WindowCase {
            id: "c".into(),
            window: vec![1],
            input: 2,
            expected: 7,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let obs = Host::new(2)
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
    let review = db
        .record_window_review_with_card(
            &candidate, run.run_id, obs.id, "operator", "approve", "map", "bridge", "proposal",
        )
        .unwrap();
    assert!(db
        .bootstrap_champion("domain", &champion, "operator")
        .is_err());
    let (v, d) = db.window_replay_identity(run.run_id).unwrap();
    db.pin_window_fixture("domain", &v, &d, "operator").unwrap();
    db.bootstrap_champion("domain", &champion, "operator")
        .unwrap();
    assert!(db
        .promote_window("domain", &champion, 999, "operator")
        .is_err());
    assert_eq!(db.current_champion("domain").unwrap(), champion);
    let rejected = db
        .record_window_review_with_card(
            &candidate, run.run_id, obs.id, "operator", "reject", "map", "bridge", "proposal",
        )
        .unwrap();
    assert!(db
        .promote_window("domain", &champion, rejected, "operator")
        .is_err());
    assert!(db
        .promote_window("domain", &champion, review, "operator")
        .is_err());
    let review = db
        .record_window_review_with_card(
            &candidate, run.run_id, obs.id, "operator", "approve", "map", "bridge", "proposal",
        )
        .unwrap();
    db.promote_window("domain", &champion, review, "operator")
        .unwrap();
    assert_eq!(db.current_champion("domain").unwrap(), candidate);
    let mut host = DurableHost::open(&mut db, "event-stream", 4).unwrap();
    let observed = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &champion,
            5,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(observed.champion, Some(7));
    assert!(db
        .promote_window("domain", &champion, review, "operator")
        .is_err());
    assert!(db.rollback_window("domain", &champion, "operator").is_err());
    db.rollback_window("domain", &candidate, "operator")
        .unwrap();
    assert_eq!(db.current_champion("domain").unwrap(), champion);
    let observed = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            5,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(observed.champion, Some(5));
    assert!(db.rollback_window("domain", &champion, "operator").is_err());
    assert!(db.verify_ledger().unwrap());
}
#[test]
fn champion_metadata_survives_reopen_and_tampering_fails_closed() {
    let path = std::env::temp_dir().join(format!(
        "sese-champion-{}-{}.db",
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
    let db = Storage::open(&path).unwrap();
    assert_eq!(db.current_champion("domain").unwrap(), id);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE champion_domains SET generation=99", [])
        .unwrap();
    drop(conn);
    let db = Storage::open(&path).unwrap();
    assert!(db.current_champion("domain").is_err());
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn rollback_during_worker_execution_rejects_stale_observation() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};
    let base = std::env::temp_dir().join(format!(
        "sese-race-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let path = base.join("data.db");
    let mut db = Storage::open(&path).unwrap();
    let original=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
    let candidate=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) i32.const 7))").unwrap()).unwrap();
    let worker = env!("CARGO_BIN_EXE_window-worker");
    let run = replay_window(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[WindowCase {
            id: "c".into(),
            window: vec![1],
            input: 2,
            expected: 7,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let obs = Host::new(2)
        .unwrap()
        .process_window(
            &mut db,
            worker,
            &original,
            &candidate,
            2,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_window_review_with_card(
            &candidate, run.run_id, obs.id, "operator", "approve", "map", "bridge", "proposal",
        )
        .unwrap();
    let (version, digest) = db.window_replay_identity(run.run_id).unwrap();
    db.pin_window_fixture("domain", &version, &digest, "operator")
        .unwrap();
    db.bootstrap_champion("domain", &original, "operator")
        .unwrap();
    db.promote_window("domain", &original, review, "operator")
        .unwrap();
    let marker = base.join("started");
    let release = base.join("release");
    let script = base.join("worker.sh");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    let text=format!("#!/bin/sh\n: > {}\nwhile [ ! -e {} ]; do /bin/sleep 0.01; done\nprintf '\\000\\000\\000\\000'\n",quote(&marker),quote(&release));
    std::fs::write(&script, text).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path2 = path.clone();
    let script2 = script.clone();
    let shadow = original.clone();
    let task = std::thread::spawn(move || {
        let mut db = Storage::open(&path2).unwrap();
        let mut host = DurableHost::open(&mut db, "raced", 4).unwrap();
        let result = host.process_domain(
            &mut db,
            script2.to_str().unwrap(),
            "domain",
            &shadow,
            5,
            Duration::from_secs(3),
        );
        (result.is_err(), host.window())
    });
    let start = Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let reached = marker.exists();
    let rollback = if reached {
        db.rollback_window("domain", &candidate, "operator")
    } else {
        Err("worker did not start".into())
    };
    std::fs::write(&release, b"go").unwrap();
    let (failed, window) = task.join().unwrap();
    assert!(reached);
    rollback.unwrap();
    assert!(failed);
    assert!(window.is_empty());
    assert_eq!(db.current_champion("domain").unwrap(), original);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM window_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(conn);
    std::fs::remove_file(script).unwrap();
    std::fs::remove_file(marker).unwrap();
    std::fs::remove_file(release).unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(base).unwrap();
}

#[test]
fn mismatch_and_newer_failed_replay_do_not_promote() {
    fn setup(pin_matches: bool) -> (Storage, String, String, i64) {
        let mut db =
            Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let champ=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))").unwrap()).unwrap();
        let candidate=db.register_synthetic_window(&wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) i32.const 7))").unwrap()).unwrap();
        let worker = env!("CARGO_BIN_EXE_window-worker");
        let run = replay_window(
            &mut db,
            worker,
            &candidate,
            "v1",
            &[WindowCase {
                id: "a".into(),
                window: vec![],
                input: 2,
                expected: 7,
            }],
            Duration::from_secs(2),
        )
        .unwrap();
        let obs = Host::new(2)
            .unwrap()
            .process_window(
                &mut db,
                worker,
                &champ,
                &candidate,
                2,
                Duration::from_secs(2),
            )
            .unwrap();
        let review = db
            .record_window_review_with_card(
                &candidate, run.run_id, obs.id, "operator", "approve", "human", "bridge",
                "proposal",
            )
            .unwrap();
        let (version, digest) = db.window_replay_identity(run.run_id).unwrap();
        db.pin_window_fixture(
            "domain",
            &version,
            if pin_matches {
                &digest
            } else {
                "0000000000000000000000000000000000000000000000000000000000000000"
            },
            "operator",
        )
        .unwrap();
        db.bootstrap_champion("domain", &champ, "operator").unwrap();
        (db, champ, candidate, review)
    }
    let (mut db, champ, _, review) = setup(false);
    assert!(db
        .promote_window("domain", &champ, review, "operator")
        .is_err());
    assert_eq!(db.current_champion("domain").unwrap(), champ);
    let (mut db, champ, candidate, review) = setup(true);
    let failed = replay_window(
        &mut db,
        env!("CARGO_BIN_EXE_window-worker"),
        &candidate,
        "v1",
        &[WindowCase {
            id: "a".into(),
            window: vec![],
            input: 2,
            expected: 8,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!failed.passed);
    assert!(db
        .promote_window("domain", &champ, review, "operator")
        .is_err());
    assert_eq!(db.current_champion("domain").unwrap(), champ);
}
#[test]
fn interrupted_transition_insertion_rolls_back_champion_and_history() {
    let path = std::env::temp_dir().join(format!(
        "sese-atomic-promotion-{}-{}.db",
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
    let run = replay_window(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[WindowCase {
            id: "c".into(),
            window: vec![],
            input: 1,
            expected: 7,
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    let obs = Host::new(2)
        .unwrap()
        .process_window(
            &mut db,
            worker,
            &champion,
            &candidate,
            1,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_window_review_with_card(
            &candidate, run.run_id, obs.id, "operator", "approve", "map", "bridge", "proposal",
        )
        .unwrap();
    let (version, digest) = db.window_replay_identity(run.run_id).unwrap();
    db.pin_window_fixture("domain", &version, &digest, "operator")
        .unwrap();
    db.bootstrap_champion("domain", &champion, "operator")
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_promotion BEFORE INSERT ON champion_transitions WHEN NEW.kind='promote' BEGIN SELECT RAISE(ABORT,'simulated write failure'); END;").unwrap();
    drop(conn);
    assert!(db
        .promote_window("domain", &champion, review, "operator")
        .is_err());
    assert_eq!(db.current_champion("domain").unwrap(), champion);
    assert!(db.verify_ledger().unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM champion_transitions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    conn.execute_batch("DROP TRIGGER fail_promotion; CREATE TRIGGER fail_ledger BEFORE INSERT ON ledger_events WHEN NEW.kind='champion_transition' BEGIN SELECT RAISE(ABORT,'simulated ledger failure'); END;").unwrap();
    drop(conn);
    assert!(db
        .promote_window("domain", &champion, review, "operator")
        .is_err());
    assert_eq!(db.current_champion("domain").unwrap(), champion);
    assert!(db.verify_ledger().unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM champion_transitions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    conn.execute_batch("DROP TRIGGER fail_ledger;").unwrap();
    drop(conn);
    db.promote_window("domain", &champion, review, "operator")
        .unwrap();
    assert_eq!(db.current_champion("domain").unwrap(), candidate);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
