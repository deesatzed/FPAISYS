#![cfg(unix)]
use sese_runtime_spike::{
    builtin_event,
    event_abi::{Decision, Event, Verdict, Window},
    event_executor::SyntheticContainerExecutor,
    event_host::ContainerShadowHost,
    event_replay::{self, Case},
    storage::Storage,
};
use std::time::Duration;

fn setup(db: &mut Storage) -> (String, String) {
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let shadow = db
        .register_synthetic_event(&builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let case = Case {
        id: "case".into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 3.0,
            reason: 4,
        },
    };
    let replay = event_replay::replay_measured(
        db,
        env!("CARGO_BIN_EXE_event-worker"),
        &champion,
        "shadow-fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    assert!(replay.passed);
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    (champion, shadow)
}

#[test]
fn container_shadow_rejects_nonexistent_domain_before_execution() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    assert!(ContainerShadowHost::open(&mut db, "missing", "stream", 4).is_err());
}

#[test]
fn champion_failure_records_measured_state_and_survives_restart() {
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-failed-champion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("db");
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let (champion, shadow) = setup(&mut db);
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "failed", 2).unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    let error = stream
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::from_secs(2),
        )
        .err()
        .unwrap();
    let seq: i64 = error
        .split("container_event_seq=")
        .nth(1)
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let record = db.inspect_container_shadow_experiment(seq).unwrap();
    assert_eq!(record["status"], "failed");
    assert_eq!(record["failure_stage"], "champion");
    assert_eq!(record["promotion_evidence"], false);
    assert!(record["error"].as_str().unwrap().contains("receipt_path="));
    assert!(record["shadow"].is_null());
    assert_eq!(stream.snapshot().count, 1);
    assert_eq!(stream.snapshot().mean, 7.0);
    assert_eq!(stream.snapshot().last_error, 500);
    drop(stream);
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let reopened = ContainerShadowHost::open(&mut db, "domain", "failed", 2).unwrap();
    assert_eq!(reopened.snapshot().count, 1);
    assert_eq!(reopened.snapshot().last_error, 500);
    assert_eq!(
        db.inspect_container_shadow_experiment(seq).unwrap()["failure_stage"],
        "champion"
    );
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    payload["next_state"][19] = serde_json::json!(0);
    let payload = payload.to_string();
    use sha2::{Digest, Sha256};
    let encoded = [
        "sese-ledger-v1",
        prev.as_str(),
        "container_shadow_experiment",
        "0",
        payload.as_str(),
        time.as_str(),
    ]
    .iter()
    .map(|s| format!("{}:{}", s.len(), s))
    .collect::<String>();
    let digest = format!("{:x}", Sha256::digest(encoded.as_bytes()));
    conn.execute(
        "UPDATE ledger_events SET payload=?1,hash=?2 WHERE seq=?3",
        rusqlite::params![payload, digest, seq],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(db.inspect_container_shadow_experiment(seq).is_err());
    assert!(ContainerShadowHost::open(&mut db, "domain", "failed", 2).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_deadline_is_rejected_without_recording_an_attempt() {
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-invalid-deadline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        dir.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let (champion, shadow) = setup(&mut db);
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "invalid", 2).unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    assert!(stream
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::ZERO
        )
        .is_err());
    assert_eq!(stream.snapshot().count, 0);
    assert_eq!(db.ledger_checkpoint().unwrap(), checkpoint);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn pending_journal_prevents_application_from_contacting_docker() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-pending-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let pending = dir.join(format!("sese-synthetic-{}.json", "a".repeat(32)));
    std::fs::write(&pending, b"{}").unwrap();
    std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o600)).unwrap();
    let marker = dir.join("docker-contacted");
    let wrapper = dir.join("docker-wrapper");
    let marker_quoted = format!("'{}'", marker.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\ntouch {marker_quoted}\nexit 1\n"),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        wrapper.to_str().unwrap(),
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        dir.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let (champion, shadow) = setup(&mut db);
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "pending", 2).unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    assert!(stream
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::from_secs(10)
        )
        .is_err());
    assert!(!marker.exists());
    assert_eq!(
        std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("sese-synthetic-"))
            .count(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_attempt_with_changed_ledger_does_not_advance_or_record() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-failed-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let path = dir.join("db");
    let marker = dir.join("started");
    let release = dir.join("release");
    let wrapper = dir.join("runner");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n: > {}\nwhile [ ! -f {} ]; do /bin/sleep 0.05; done\nexit 1\n",
            quote(&marker),
            quote(&release)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let executor = SyntheticContainerExecutor::new(
        wrapper.to_str().unwrap(),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let (champion, shadow) = setup(&mut db);
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let mut db = Storage::open(&worker_path).unwrap();
        let mut stream = ContainerShadowHost::open(&mut db, "domain", "race-failure", 2).unwrap();
        let event = Event {
            timestamp_ns: 9,
            entity_id: 42,
            metric: 7.0,
            flags: 4,
        };
        let error = stream
            .process(
                &mut db,
                &executor,
                &champion,
                &shadow,
                &event,
                Duration::from_secs(20),
            )
            .err()
            .unwrap();
        (error, stream.snapshot().count)
    });
    let start = std::time::Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = marker.exists();
    if started {
        db.register_synthetic_event(&builtin_event::artifact("quarantine").unwrap())
            .unwrap();
    }
    std::fs::write(&release, b"release").unwrap();
    let (error, count) = worker.join().unwrap();
    assert!(started);
    assert!(
        error.contains("ledger changed during container shadow")
            && error.contains("champion_error="),
        "{error}"
    );
    assert_eq!(count, 0);
    assert_eq!(
        ContainerShadowHost::open(&mut db, "domain", "race-failure", 2)
            .unwrap()
            .snapshot()
            .count,
        0
    );
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM ledger_events WHERE kind='container_shadow_experiment'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn container_shadow_rejects_unknown_candidate_before_launch() {
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-reject-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        dir.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let (champion, _) = setup(&mut db);
    let unknown = db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 1 i32.const 0))").unwrap()).unwrap();
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "stream", 2).unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    assert!(stream
        .process(
            &mut db,
            &executor,
            &champion,
            &unknown,
            &event,
            Duration::from_secs(2)
        )
        .err()
        .unwrap()
        .contains("unknown synthetic artifact"));
    assert_eq!(db.ledger_checkpoint().unwrap(), checkpoint);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn container_shadow_rejects_changed_ledger_during_execution() {
    use std::os::unix::fs::PermissionsExt;
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db_path = dir.join("db");
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let marker = dir.join("started");
    let release = dir.join("release");
    let wrapper = dir.join("docker-wrapper");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$3\" = run ]; then : > {}; while [ ! -f {} ]; do /bin/sleep 0.05; done; fi\nexec /usr/local/bin/docker \"$@\"\n",quote(&marker),quote(&release))).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut db = Storage::open(&db_path).unwrap();
    let (champion, shadow) = setup(&mut db);
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        wrapper.to_str().unwrap(),
        &image,
        &host,
        journal.to_str().unwrap(),
    )
    .unwrap();
    let worker_path = db_path.clone();
    let task = std::thread::spawn(move || {
        let mut db = Storage::open(&worker_path).unwrap();
        let mut stream = ContainerShadowHost::open(&mut db, "domain", "race", 2).unwrap();
        let event = Event {
            timestamp_ns: 9,
            entity_id: 42,
            metric: 7.0,
            flags: 4,
        };
        let err = stream
            .process(
                &mut db,
                &executor,
                &champion,
                &shadow,
                &event,
                Duration::from_secs(40),
            )
            .err();
        (err, stream.snapshot().count)
    });
    let start = std::time::Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = marker.exists();
    if started {
        db.register_synthetic_event(&builtin_event::artifact("quarantine").unwrap())
            .unwrap();
    }
    std::fs::write(&release, b"release").unwrap();
    let (error, count) = task.join().unwrap();
    assert!(started);
    assert!(error
        .unwrap_or_default()
        .contains("ledger changed during container shadow"));
    assert_eq!(count, 0);
    assert_eq!(
        ContainerShadowHost::open(&mut db, "domain", "race", 2)
            .unwrap()
            .snapshot()
            .count,
        0
    );
    drop(db);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM ledger_events WHERE kind='container_shadow_experiment'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn container_shadow_failure_records_champion_and_both_attempt_paths() {
    use std::os::unix::fs::PermissionsExt;
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-shadow-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let marker = dir.join("first-run");
    let wrapper = dir.join("docker-wrapper");
    let marker = format!("'{}'", marker.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&wrapper,format!("#!/bin/sh\nif [ \"$3\" = run ]; then if [ -e {marker} ]; then exit 1; fi; : > {marker}; fi\nexec /usr/local/bin/docker \"$@\"\n")).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        wrapper.to_str().unwrap(),
        &image,
        &host,
        dir.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let (champion, shadow) = setup(&mut db);
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "failure", 2).unwrap();
    let before = db.ledger_checkpoint().unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    let error = stream
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::from_secs(40),
        )
        .err()
        .unwrap();
    assert!(error.contains("champion_receipt_path="), "{error}");
    assert!(
        error.contains("shadow_error=") && error.contains("; receipt_path="),
        "{error}"
    );
    let seq: i64 = error
        .split("container_event_seq=")
        .nth(1)
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let failure = db.inspect_container_shadow_experiment(seq).unwrap();
    assert_eq!(failure["failure_stage"], "shadow");
    assert_eq!(failure["status"], "failed");
    assert!(failure["champion"]["receipt"].as_str().is_some());
    assert!(failure["shadow"].is_null());
    assert_eq!(failure["promotion_evidence"], false);
    assert_eq!(stream.snapshot().count, 1);
    assert_eq!(stream.snapshot().mean, 7.0);
    assert_eq!(stream.snapshot().last_error, 0);
    assert_eq!(db.ledger_checkpoint().unwrap().0, before.0 + 1);
    assert_eq!(
        ContainerShadowHost::open(&mut db, "domain", "failure", 2)
            .unwrap()
            .snapshot()
            .count,
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn container_shadow_uses_prior_state_and_survives_restart() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let directory = std::env::temp_dir().join(format!(
        "sese-container-host-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("db");
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/local/bin/docker",
        &image,
        &host,
        directory.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let (champion, shadow) = setup(&mut db);
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "stream", 2).unwrap();
    let mut stale = ContainerShadowHost::open(&mut db, "domain", "stream", 2).unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    let first = stream
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::from_secs(40),
        )
        .unwrap();
    assert_eq!(first.prior.count, 0);
    assert_eq!(first.champion.decision, Decision::Allow);
    assert_eq!(first.shadow.decision, Decision::Throttle);
    let checkpoint = db.ledger_checkpoint().unwrap();
    assert!(stale
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::from_secs(40)
        )
        .err()
        .unwrap()
        .contains("stale"));
    assert_eq!(db.ledger_checkpoint().unwrap(), checkpoint);
    let evidence = db.inspect_container_shadow_experiment(first.seq).unwrap();
    assert_eq!(evidence["promotion_evidence"], false);
    assert_eq!(evidence["execution_authenticated"], false);
    assert!(evidence["shadow_us"].as_u64().is_some());
    assert_ne!(
        evidence["champion"]["receipt_path"],
        evidence["shadow"]["receipt_path"]
    );
    drop(stream);
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let mut stream = ContainerShadowHost::open(&mut db, "domain", "stream", 2).unwrap();
    assert_eq!(stream.snapshot().count, 1);
    assert_eq!(stream.snapshot().mean, 7.0);
    let second = stream
        .process(
            &mut db,
            &executor,
            &champion,
            &shadow,
            &event,
            Duration::from_secs(40),
        )
        .unwrap();
    assert_eq!(second.prior.count, 1);
    assert_eq!(second.prior.mean, 7.0);
    assert_eq!(stream.snapshot().count, 2);
    assert!(db.inspect_event_review(second.seq).is_err());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-container-shadow",
            path.to_str().unwrap(),
            "domain",
            "cli-stream",
            "2",
            "allow",
            "throttle",
            "9",
            "42",
            "7.0",
            "4",
            &image,
            &host,
            directory.to_str().unwrap(),
            "/opt/homebrew/bin/python3",
            "/usr/local/bin/docker",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/scripts/synthetic_container_worker.py"
            ),
            "40",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["promotion_evidence"], false);
    assert_eq!(result["prior"]["count"], 0);
    let cli_seq = result["container_event_seq"].as_i64().unwrap();
    assert_eq!(
        db.inspect_container_shadow_experiment(cli_seq).unwrap()["stream"],
        "cli-stream"
    );
    let inspected = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-container-shadow-inspect",
            path.to_str().unwrap(),
            &cli_seq.to_string(),
        ])
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let inspected: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected["stream"], "cli-stream");
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [cli_seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    payload["next_state"][0] = serde_json::json!(0);
    let payload = payload.to_string();
    use sha2::{Digest, Sha256};
    let encoded = [
        "sese-ledger-v1",
        prev.as_str(),
        "container_shadow_experiment",
        "0",
        payload.as_str(),
        time.as_str(),
    ]
    .iter()
    .map(|s| format!("{}:{}", s.len(), s))
    .collect::<String>();
    let digest = format!("{:x}", Sha256::digest(encoded.as_bytes()));
    conn.execute(
        "UPDATE ledger_events SET payload=?1,hash=?2 WHERE seq=?3",
        rusqlite::params![payload, digest, cli_seq],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(db.inspect_container_shadow_experiment(cli_seq).is_err());
    assert!(ContainerShadowHost::open(&mut db, "domain", "cli-stream", 2).is_err());
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}
