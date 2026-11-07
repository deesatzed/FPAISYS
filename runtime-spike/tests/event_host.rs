use sese_runtime_spike::{
    event_abi::{Decision, Event},
    event_host::EventHost,
    storage::Storage,
};
use std::time::Duration;
#[test]
fn event_host_supplies_prior_window_and_retains_observation() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-host-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 1 local.get 3))").unwrap()).unwrap();
    let shadow=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))").unwrap()).unwrap();
    let mut host = EventHost::new(2).unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let first = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let second = Event {
        timestamp_ns: 2,
        entity_id: 2,
        metric: 5.0,
        flags: 6,
    };
    host.process(
        &mut db,
        worker,
        &champion,
        &shadow,
        &first,
        0,
        Duration::from_secs(2),
    )
    .unwrap();
    let out = host
        .process(
            &mut db,
            worker,
            &champion,
            &shadow,
            &second,
            9,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(out.prior.count, 1);
    assert_eq!(out.prior.mean, 3.0);
    assert_eq!(out.prior.p99_us, 3);
    assert_eq!(out.prior.last_error, 9);
    assert_eq!(out.champion.unwrap().decision, Decision::Allow);
    assert_eq!(out.shadow.unwrap().decision, Decision::Throttle);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let snapshot: String = conn
        .query_row(
            "SELECT prior_json FROM event_observations WHERE id=?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(parsed["mean"], 3.0);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn invalid_event_and_wrong_abi_do_not_advance_window() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let valid=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap()).unwrap();
    let scalar = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let mut host = EventHost::new(2).unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let bad = Event {
        timestamp_ns: 1,
        entity_id: 0,
        metric: f64::NAN,
        flags: 0,
    };
    assert!(host
        .process(
            &mut db,
            worker,
            &valid,
            &valid,
            &bad,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    let good = Event { metric: 1.0, ..bad };
    assert!(host
        .process(
            &mut db,
            worker,
            &valid,
            &scalar,
            &good,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(host.snapshot(0).count, 0);
}
#[test]
fn shadow_trap_never_replaces_champion_and_is_recorded_as_error() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap()).unwrap();
    let shadow=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) unreachable))").unwrap()).unwrap();
    let mut host = EventHost::new(2).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 2.5,
        flags: 0,
    };
    let result = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &champion,
            &shadow,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.champion.unwrap().decision, Decision::Allow);
    assert!(result.shadow.is_none());
    assert!(result.shadow_error.is_some());
    assert_eq!(host.snapshot(0).p99_us, 3);
}

#[test]
fn durable_event_window_reopens_and_rejects_stale_writers() {
    let path = std::env::temp_dir().join(format!(
        "sese-durable-event-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 1 local.get 3))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut first = EventHost::open(&mut db, "latency", 2).unwrap();
    let mut stale = EventHost::open(&mut db, "latency", 2).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 3.5,
        flags: 0,
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    first
        .process(&mut db, worker, &id, &id, &event, 0, Duration::from_secs(2))
        .unwrap();
    assert!(stale
        .process(&mut db, worker, &id, &id, &event, 0, Duration::from_secs(2))
        .is_err());
    assert_eq!(stale.snapshot(0).count, 0);
    drop(first);
    drop(stale);
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let mut restarted = EventHost::open(&mut db, "latency", 2).unwrap();
    assert_eq!(restarted.snapshot(0).mean, 3.5);
    let second = Event {
        metric: 5.0,
        ..event
    };
    let observed = restarted
        .process(
            &mut db,
            worker,
            &id,
            &id,
            &second,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(observed.prior.count, 1);
    assert_eq!(observed.champion.unwrap().score, 3.5);
    assert!(EventHost::open(&mut db, "latency", 3).is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_observation_write_does_not_advance_durable_window() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-atomic-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut host = EventHost::open(&mut db, "stream", 1).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_event_observation BEFORE INSERT ON event_observations BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 1.0,
        flags: 0,
    };
    assert!(host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &id,
            &id,
            &event,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(host.snapshot(0).count, 0);
    let (generation, len): (i64, i64) = conn
        .query_row(
            "SELECT generation,length(latencies) FROM event_streams WHERE name='stream'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((generation, len), (0, 0));
    conn.execute_batch("DROP TRIGGER fail_event_observation")
        .unwrap();
    host.process(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &id,
        &id,
        &event,
        0,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(host.snapshot(0).count, 1);
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn observation_ledger_link_detects_tampering_and_missing_evidence() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-observation-ledger-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut host = EventHost::open(&mut db, "stream", 2).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 1.0,
        flags: 0,
    };
    let observed = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &id,
            &id,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(db.verify_event_observation(observed.id).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE event_observations SET shadow_error='tampered' WHERE id=?1",
        [observed.id],
    )
    .unwrap();
    assert!(!db.verify_event_observation(observed.id).unwrap());
    conn.execute(
        "UPDATE event_observations SET shadow_error=NULL WHERE id=?1",
        [observed.id],
    )
    .unwrap();
    assert!(db.verify_event_observation(observed.id).unwrap());
    conn.execute(
        "DELETE FROM ledger_events WHERE kind='event_observation' AND reference=?1",
        [observed.id],
    )
    .unwrap();
    assert!(!db.verify_event_observation(observed.id).unwrap());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn observation_ledger_failure_rolls_back_stream_and_observation() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-ledger-failure-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut host = EventHost::open(&mut db, "stream", 1).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_observation_event BEFORE INSERT ON ledger_events WHEN NEW.kind='event_observation' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 1.0,
        flags: 0,
    };
    assert!(host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &id,
            &id,
            &event,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    let (generation, len): (i64, i64) = conn
        .query_row(
            "SELECT generation,length(latencies) FROM event_streams WHERE name='stream'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((generation, len), (0, 0));
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(host.snapshot(0).count, 0);
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn durable_event_stream_tampering_is_detected_before_reopen() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-stream-integrity-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let mut host = EventHost::open(&mut db, "stream", 2).unwrap();
    host.process(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        &candidate,
        &Event {
            timestamp_ns: 1,
            entity_id: 2,
            metric: 3.0,
            flags: 0,
        },
        0,
        Duration::from_secs(2),
    )
    .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE event_streams SET latencies=?1 WHERE name='stream'",
        [4.0f64.to_bits().to_be_bytes().to_vec()],
    )
    .unwrap();
    assert!(!db.verify_ledger().unwrap());
    assert!(EventHost::open(&mut db, "stream", 2).is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn legacy_unlinked_event_stream_is_preserved_but_rejected() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-legacy-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let _host = EventHost::open(&mut db, "legacy", 2).unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts",
    )
    .unwrap();
    conn.execute_batch("DROP TABLE event_followup_receipts; DROP TABLE event_followup_pins; DELETE FROM ledger_events WHERE kind='event_stream_receipt'; DROP TABLE event_training_traces; DROP TABLE synthetic_scalar_candidates; DROP TABLE synthetic_window_candidates; DROP TABLE synthetic_event_candidates; DROP TABLE measured_stream_receipts; DROP TABLE measured_streams; DROP TABLE event_stream_receipts; ALTER TABLE event_replay_runs DROP COLUMN state_semantics; ALTER TABLE event_replay_runs DROP COLUMN worker_path; ALTER TABLE event_replay_runs DROP COLUMN worker_sha256; ALTER TABLE event_observations DROP COLUMN worker_path; ALTER TABLE event_observations DROP COLUMN worker_sha256; ALTER TABLE event_observations DROP COLUMN domain; PRAGMA user_version=24;").unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(EventHost::open(&mut db, "legacy", 2).is_err());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM event_streams WHERE name='legacy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn measured_host_supplies_metric_mean_and_host_error_from_prior_events() {
    use sese_runtime_spike::event_host::MeasuredEventHost;
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) local.get 7 i32.const 9 i32.eq if unreachable end i32.const 0 local.get 1 local.get 3))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut host = MeasuredEventHost::new(2).unwrap();
    let first = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: -10.0,
        flags: 0,
    };
    host.process(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &id,
        &id,
        &first,
        Duration::from_secs(2),
    )
    .unwrap();
    let failed = Event {
        metric: 20.0,
        flags: 9,
        ..first
    };
    let result = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &id,
            &id,
            &failed,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(result.champion_error.is_some());
    let snapshot = host.snapshot();
    assert_eq!(snapshot.count, 2);
    assert_eq!(snapshot.mean, 5.0);
    assert_eq!(snapshot.last_error, 500);
    let third = Event {
        metric: 30.0,
        flags: 0,
        ..first
    };
    let result = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &id,
            &id,
            &third,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.champion.unwrap().score, 5.0);
    assert_eq!(result.champion.unwrap().reason, 500);
    assert_eq!(host.snapshot().count, 3);
    assert_eq!(host.snapshot().mean, 25.0);
}

#[test]
fn measured_stream_restarts_rejects_stale_writers_and_detects_tampering() {
    use sese_runtime_spike::event_host::MeasuredEventHost;
    let path = std::env::temp_dir().join(format!(
        "sese-measured-stream-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 1 local.get 3))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut host = MeasuredEventHost::open(&mut db, "measured", 2).unwrap();
    let mut stale = MeasuredEventHost::open(&mut db, "measured", 2).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: -4.0,
        flags: 0,
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    host.process(&mut db, worker, &id, &id, &event, Duration::from_secs(2))
        .unwrap();
    assert!(stale
        .process(&mut db, worker, &id, &id, &event, Duration::from_secs(2))
        .is_err());
    assert_eq!(stale.snapshot().count, 0);
    drop(host);
    drop(stale);
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let host = MeasuredEventHost::open(&mut db, "measured", 2).unwrap();
    assert_eq!(host.snapshot().count, 1);
    assert_eq!(host.snapshot().mean, -4.0);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE measured_streams SET generation=generation+1 WHERE name='measured'",
        [],
    )
    .unwrap();
    assert!(!db.verify_ledger().unwrap());
    assert!(MeasuredEventHost::open(&mut db, "measured", 2).is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn measured_receipt_failure_rolls_back_observation_and_state() {
    use sese_runtime_spike::event_host::MeasuredEventHost;
    let path = std::env::temp_dir().join(format!(
        "sese-measured-atomic-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut db = Storage::open(&path).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let id = db.register_synthetic_event(&wasm).unwrap();
    let mut host = MeasuredEventHost::open(&mut db, "measured", 2).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_measured_receipt BEFORE INSERT ON ledger_events WHEN NEW.kind='measured_stream_receipt' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 0,
    };
    assert!(host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &id,
            &id,
            &event,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(host.snapshot().count, 0);
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM measured_streams WHERE name='measured'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let observations: i64 = conn
        .query_row("SELECT count(*) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!((generation, observations), (0, 0));
    assert!(db.verify_ledger().unwrap());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
