#![cfg(unix)]
use sese_runtime_spike::{
    builtin_event,
    event_abi::{Decision, Event, Verdict, Window},
    event_executor::SyntheticContainerExecutor,
    event_host::{ContainerShadowHost, MeasuredEventHost},
    event_replay::{self, Case},
    storage::Storage,
};
use std::time::Duration;

#[test]
fn window_sensitive_candidate_matches_replay_and_captured_observation() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-window-review-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/local/bin/docker",
        &image,
        &host,
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::open(&dir.join("db")).unwrap();
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let candidate = db
        .register_synthetic_event(&builtin_event::artifact("window-probe").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
        flags: 4,
    };
    let case = Case {
        id: "case".into(),
        window: Window {
            count: 2,
            mean: 3.5,
            p99_us: 9,
            last_error: 4,
        },
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 14.5,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "window-fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let mut local = MeasuredEventHost::open(&mut db, "local-stream", 2).unwrap();
    local
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    let observation = local
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(observation.prior.count, 1);
    let verdict = observation.shadow.unwrap();
    assert_eq!(
        verdict.score,
        observation.prior.mean + observation.prior.count as f64 + observation.prior.p99_us as f64
    );
    assert_eq!(verdict.reason, observation.prior.last_error);
    let fixture = serde_json::json!({"schema":"sese-event-fixture-file-v1","version":"window-fixture","state_semantics":"measured-v2","cases":[{"id":"case","window":{"count":2,"mean":3.5,"p99_us":9,"last_error":4},"event":{"timestamp_ns":9,"entity_id":42,"metric":7.0,"flags":4},"expected":{"decision":0,"score":14.5,"reason":4}}]});
    let pack =
        sese_runtime_spike::fixture_file::parse(&serde_json::to_vec(&fixture).unwrap()).unwrap();
    let replay_container = event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &pack,
        Duration::from_secs(40),
    )
    .unwrap();
    let comparison = sese_runtime_spike::event_host::compare_container_observation(
        &mut db,
        &executor,
        "domain",
        observation.id,
        Duration::from_secs(40),
    )
    .unwrap();
    let preview = db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            replay_container.seq,
            comparison.seq,
        )
        .unwrap();
    assert_eq!(preview["matched"], true);
    assert_eq!(preview["promotion_evidence"], false);
    assert_eq!(preview["execution_authenticated"], false);
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_champion_attempt_is_durable_and_never_a_successful_comparison() {
    let dir = std::env::temp_dir().join(format!(
        "sese-observation-failure-{}-{}",
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
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let candidate = db
        .register_synthetic_event(&builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
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
            decision: Decision::Throttle,
            score: 7.0,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let observation = MeasuredEventHost::new(2)
        .unwrap()
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    let before = db.ledger_checkpoint().unwrap();
    let original_observations = db.inspect_event_observations("domain", 0).unwrap();
    assert!(
        sese_runtime_spike::event_host::compare_container_observation(
            &mut db,
            &executor,
            "domain",
            observation.id,
            Duration::ZERO
        )
        .is_err()
    );
    assert_eq!(db.ledger_checkpoint().unwrap(), before);
    let error = sese_runtime_spike::event_host::compare_container_observation(
        &mut db,
        &executor,
        "domain",
        observation.id,
        Duration::from_secs(3),
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
    let failure = db.inspect_container_observation_experiment(seq).unwrap();
    assert_eq!(failure["status"], "failed");
    assert_eq!(failure["failure_stage"], "champion");
    assert_eq!(failure["observation_id"], observation.id);
    assert_eq!(failure["promotion_evidence"], false);
    assert!(failure["error"].as_str().unwrap().contains("receipt_path="));
    assert!(failure["champion"].is_null() && failure["shadow"].is_null());
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    assert_eq!(db.ledger_checkpoint().unwrap().0, before.0 + 1);
    assert_eq!(
        db.inspect_event_observations("domain", 0).unwrap(),
        original_observations
    );
    drop(db);
    let mut db = Storage::open_read_only(&path).unwrap();
    assert_eq!(
        db.inspect_container_observation_experiment(seq).unwrap()["failure_stage"],
        "champion"
    );
    assert!(db
        .event_container_review_preview("domain", replay.run_id, observation.id, seq, seq)
        .is_err());
    drop(db);
    use sha2::{Digest, Sha256};
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    payload["failure_stage"] = serde_json::json!("shadow");
    let payload = payload.to_string();
    let encoded = [
        "sese-ledger-v1",
        prev.as_str(),
        "container_observation_experiment",
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
    let mut db = Storage::open_read_only(&path).unwrap();
    assert!(db.inspect_container_observation_experiment(seq).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn historical_observation_before_rollback_is_rejected_before_launch() {
    let directory = std::env::temp_dir().join(format!(
        "sese-stale-observation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        directory.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let candidate = db
        .register_synthetic_event(&builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
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
            decision: Decision::Throttle,
            score: 7.0,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let observation = MeasuredEventHost::new(2)
        .unwrap()
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate,
            replay.run_id,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    db.promote_event("domain", &champion, review, "operator")
        .unwrap();
    db.rollback_event("domain", &candidate, "operator").unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let error = sese_runtime_spike::event_host::compare_container_observation(
        &mut db,
        &executor,
        "domain",
        observation.id,
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert!(
        error.contains("source observation predates current champion transition"),
        "{error}"
    );
    assert_eq!(db.ledger_checkpoint().unwrap(), checkpoint);
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn failed_attempt_does_not_record_over_changed_ledger() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sese-observation-failure-race-{}-{}",
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
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let candidate = db
        .register_synthetic_event(&builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
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
            decision: Decision::Throttle,
            score: 7.0,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let observation = MeasuredEventHost::new(2)
        .unwrap()
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    let worker_path = path.clone();
    let task = std::thread::spawn(move || {
        let mut db = Storage::open(&worker_path).unwrap();
        sese_runtime_spike::event_host::compare_container_observation(
            &mut db,
            &executor,
            "domain",
            observation.id,
            Duration::from_secs(20),
        )
        .err()
    });
    let start = std::time::Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = marker.exists();
    let changed = if started {
        db.register_synthetic_event(&builtin_event::artifact("quarantine").unwrap())
            .is_ok()
    } else {
        false
    };
    std::fs::write(&release, b"release").unwrap();
    let error = task.join().unwrap().unwrap_or_default();
    assert!(started && changed);
    assert!(
        error.contains("ledger changed during container observation")
            && error.contains("champion_error=")
            && error.contains("receipt_path="),
        "{error}"
    );
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ledger_events WHERE kind='container_observation_experiment'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn changed_ledger_rejects_historical_container_experiment() {
    use std::os::unix::fs::PermissionsExt;
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-observation-race-{}-{}",
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
    let wrapper = dir.join("docker-wrapper");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&wrapper,format!("#!/bin/sh\nif [ \"$3\" = run ]; then : > {}; while [ ! -f {} ]; do /bin/sleep 0.05; done; fi\nexec /usr/local/bin/docker \"$@\"\n",quote(&marker),quote(&release))).unwrap();
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
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let candidate = db
        .register_synthetic_event(&builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
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
            decision: Decision::Throttle,
            score: 7.0,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let observation = MeasuredEventHost::new(2)
        .unwrap()
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    let worker_path = path.clone();
    let task = std::thread::spawn(move || {
        let mut db = Storage::open(&worker_path).unwrap();
        sese_runtime_spike::event_host::compare_container_observation(
            &mut db,
            &executor,
            "domain",
            observation.id,
            Duration::from_secs(40),
        )
        .err()
    });
    let start = std::time::Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = marker.exists();
    let changed = if started {
        db.register_synthetic_event(&builtin_event::artifact("quarantine").unwrap())
            .is_ok()
    } else {
        false
    };
    std::fs::write(&release, b"release").unwrap();
    let error = task.join().unwrap().unwrap_or_default();
    assert!(started && changed);
    assert!(
        error.contains("ledger changed during container observation")
            && error.contains("champion_receipt_path=")
            && error.contains("shadow_receipt_path="),
        "{error}"
    );
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ledger_events WHERE kind='container_observation_experiment'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn operator_can_compare_stored_container_evidence_in_a_read_only_snapshot() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-container-review-preview-{}-{}",
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
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        "/usr/local/bin/docker",
        &image,
        &host,
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let champion = db
        .register_synthetic_event(&builtin_event::artifact("allow").unwrap())
        .unwrap();
    let candidate = db
        .register_synthetic_event(&builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let event = Event {
        timestamp_ns: 9,
        entity_id: 42,
        metric: 7.0,
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
            decision: Decision::Throttle,
            score: 7.0,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "preview-fixture",
        &[case.clone()],
        Duration::from_secs(3),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let mut local_host = MeasuredEventHost::open(&mut db, "local-stream", 2).unwrap();
    let observation = local_host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(3),
        )
        .unwrap();
    let fixture = serde_json::json!({"schema":"sese-event-fixture-file-v1","version":"preview-fixture","state_semantics":"measured-v2","cases":[{"id":"case","window":{"count":0,"mean":0.0,"p99_us":0,"last_error":0},"event":{"timestamp_ns":9,"entity_id":42,"metric":7.0,"flags":4},"expected":{"decision":1,"score":7.0,"reason":4}}]});
    let pack =
        sese_runtime_spike::fixture_file::parse(&serde_json::to_vec(&fixture).unwrap()).unwrap();
    let container_replay = event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &pack,
        Duration::from_secs(40),
    )
    .unwrap();
    let mut container_host = ContainerShadowHost::open(&mut db, "domain", "container", 2).unwrap();
    let container_shadow = container_host
        .process(
            &mut db,
            &executor,
            &champion,
            &candidate,
            &event,
            Duration::from_secs(40),
        )
        .unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let preview = db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            container_replay.seq,
            container_shadow.seq,
        )
        .unwrap();
    assert_eq!(preview["matched"], true);
    assert_eq!(preview["promotion_evidence"], false);
    assert_eq!(preview["execution_authenticated"], false);
    assert_eq!(preview["checkpoint"]["seq"], checkpoint.0);
    assert_eq!(preview["checkpoint"]["hash"], checkpoint.1);
    assert_eq!(db.ledger_checkpoint().unwrap(), checkpoint);
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id + 1000,
            container_replay.seq,
            container_shadow.seq
        )
        .is_err());
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            container_shadow.seq,
            container_replay.seq
        )
        .is_err());
    let other_event = Event {
        metric: 8.0,
        ..event
    };
    let different_observation = local_host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &other_event,
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(different_observation.prior.count, 1);
    let historical = sese_runtime_spike::event_host::compare_container_observation(
        &mut db,
        &executor,
        "domain",
        different_observation.id,
        Duration::from_secs(40),
    )
    .unwrap();
    let inspected = db
        .inspect_container_observation_experiment(historical.seq)
        .unwrap();
    assert_eq!(inspected["observation_id"], different_observation.id);
    assert_eq!(inspected["prior"]["count"], 1);
    assert_eq!(inspected["promotion_evidence"], false);
    let historical_preview = db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            different_observation.id,
            container_replay.seq,
            historical.seq,
        )
        .unwrap();
    assert_eq!(historical_preview["matched"], true);
    assert_eq!(
        historical_preview["container_comparison_kind"],
        "container_observation_experiment"
    );
    assert_eq!(
        historical_preview["container_comparison_seq"],
        historical.seq
    );
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            container_replay.seq,
            historical.seq
        )
        .is_err());
    let cli = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-container-observation",
            path.to_str().unwrap(),
            "domain",
            &different_observation.id.to_string(),
            &image,
            &host,
            journal.to_str().unwrap(),
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
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli["promotion_evidence"], false);
    let inspection = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-container-observation-inspect",
            path.to_str().unwrap(),
            &cli["container_event_seq"].as_i64().unwrap().to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        inspection.status.success(),
        "{}",
        String::from_utf8_lossy(&inspection.stderr)
    );
    let inspected_cli: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    assert_eq!(inspected_cli["observation_id"], different_observation.id);
    assert_eq!(
        db.event_container_review_preview(
            "domain",
            replay.run_id,
            different_observation.id,
            container_replay.seq,
            cli["container_event_seq"].as_i64().unwrap()
        )
        .unwrap()["matched"],
        true
    );
    use std::os::unix::fs::PermissionsExt;
    let failure_journal = dir.join("failure-journal");
    std::fs::create_dir(&failure_journal).unwrap();
    let marker = dir.join("first-run");
    let wrapper = dir.join("failing-docker");
    let marker_quoted = format!("'{}'", marker.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&wrapper,format!("#!/bin/sh\nif [ \"$3\" = run ]; then if [ -e {marker_quoted} ]; then exit 1; fi; : > {marker_quoted}; fi\nexec /usr/local/bin/docker \"$@\"\n")).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let failure_executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_worker.py"
        ),
        wrapper.to_str().unwrap(),
        &image,
        &host,
        failure_journal.to_str().unwrap(),
    )
    .unwrap();
    let failure = sese_runtime_spike::event_host::compare_container_observation(
        &mut db,
        &failure_executor,
        "domain",
        different_observation.id,
        Duration::from_secs(40),
    )
    .err()
    .unwrap();
    let failure_seq: i64 = failure
        .split("container_event_seq=")
        .nth(1)
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let failure_record = db
        .inspect_container_observation_experiment(failure_seq)
        .unwrap();
    assert_eq!(failure_record["failure_stage"], "shadow");
    assert!(failure_record["champion"]["receipt"].as_str().is_some());
    assert!(failure_record["shadow"].is_null());
    assert!(failure.contains("champion_receipt_path=") && failure.contains("receipt_path="));
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            different_observation.id,
            container_replay.seq,
            failure_seq
        )
        .is_err());
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    let different_shadow = container_host
        .process(
            &mut db,
            &executor,
            &champion,
            &candidate,
            &other_event,
            Duration::from_secs(40),
        )
        .unwrap();
    let wrong_replay = event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &champion,
        &pack,
        Duration::from_secs(40),
    )
    .unwrap();
    assert!(!wrong_replay.passed);
    let other_local = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "other-fixture",
        &[case],
        Duration::from_secs(3),
    )
    .unwrap();
    let mut other_fixture = fixture.clone();
    other_fixture["version"] = serde_json::json!("other-fixture");
    let other_pack =
        sese_runtime_spike::fixture_file::parse(&serde_json::to_vec(&other_fixture).unwrap())
            .unwrap();
    let other_container = event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &other_pack,
        Duration::from_secs(40),
    )
    .unwrap();
    let later_checkpoint = db.ledger_checkpoint().unwrap();
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            different_observation.id,
            container_replay.seq,
            container_shadow.seq
        )
        .is_err());
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            container_replay.seq,
            different_shadow.seq
        )
        .is_err());
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            wrong_replay.seq,
            container_shadow.seq
        )
        .is_err());
    assert!(db
        .event_container_review_preview(
            "domain",
            other_local.run_id,
            observation.id,
            other_container.seq,
            container_shadow.seq
        )
        .is_err());
    assert_eq!(db.ledger_checkpoint().unwrap(), later_checkpoint);
    let replay_evidence = db
        .inspect_container_replay_experiment(container_replay.seq)
        .unwrap();
    let mut duplicate_host = ContainerShadowHost::open(&mut db, "domain", "duplicate", 2).unwrap();
    let duplicate = duplicate_host
        .process(
            &mut db,
            &executor,
            &champion,
            &candidate,
            &event,
            Duration::from_secs(40),
        )
        .unwrap();
    drop(db);
    let mut db = Storage::open_read_only(&path).unwrap();
    assert_eq!(
        db.event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            container_replay.seq,
            container_shadow.seq
        )
        .unwrap()["matched"],
        true
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-container-review-preview",
            path.to_str().unwrap(),
            "domain",
            &replay.run_id.to_string(),
            &observation.id.to_string(),
            &container_replay.seq.to_string(),
            &container_shadow.seq.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["promotion_evidence"], false);
    drop(db);
    // Reuse the replay receipt for an otherwise valid shadow execution, then rehash the last ledger event.
    // Each experiment is internally consistent, but the combined preview must reject reused execution.
    use sha2::{Digest, Sha256};
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [duplicate.seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    payload["shadow"]["receipt"] = replay_evidence["cases"][0]["receipt"].clone();
    payload["shadow"]["receipt_path"] = replay_evidence["cases"][0]["receipt_path"].clone();
    let payload = payload.to_string();
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
        rusqlite::params![payload, digest, duplicate.seq],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open_read_only(&path).unwrap();
    assert!(db
        .inspect_container_shadow_experiment(duplicate.seq)
        .is_ok());
    assert!(db
        .event_container_review_preview(
            "domain",
            replay.run_id,
            observation.id,
            container_replay.seq,
            duplicate.seq
        )
        .is_err());
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let historical = sese_runtime_spike::event_host::compare_container_observation(
        &mut db,
        &executor,
        "domain",
        different_observation.id,
        Duration::from_secs(40),
    )
    .unwrap();
    assert!(db
        .inspect_container_observation_experiment(historical.seq)
        .is_ok());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [historical.seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    payload["champion_generation"] = serde_json::json!(999);
    let payload = payload.to_string();
    let encoded = [
        "sese-ledger-v1",
        prev.as_str(),
        "container_observation_experiment",
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
        rusqlite::params![payload, digest, historical.seq],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open_read_only(&path).unwrap();
    assert!(db
        .inspect_container_observation_experiment(historical.seq)
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
