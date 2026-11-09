use sese_runtime_spike::{
    event_abi::{Decision, Event, Verdict, Window},
    event_host::EventHost,
    event_replay::{self, Case},
    storage::Storage,
};
use std::time::Duration;
#[cfg(unix)]
fn gated_worker(
    base: &std::path::Path,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    use std::os::unix::fs::PermissionsExt;
    let script = base.join("worker.sh");
    let armed = base.join("armed");
    let marker = base.join("started");
    let release = base.join("release");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    let worker = std::path::Path::new(env!("CARGO_BIN_EXE_event-worker"));
    let source=format!("#!/bin/sh\nif [ -e {} ]; then : > {}; while [ ! -e {} ]; do /bin/sleep 0.01; done; fi\nexec {}\n",quote(&armed),quote(&marker),quote(&release),quote(worker));
    std::fs::write(&script, source).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    (script, armed, marker, release)
}
fn fixture() -> (Storage, String, i64, i64) {
    let db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    fixture_with(db)
}
fn fixture_with(mut db: Storage) -> (Storage, String, i64, i64) {
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let window = Window {
        count: 0,
        mean: 0.0,
        p99_us: 0,
        last_error: 0,
    };
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let case = Case {
        id: "case".into(),
        window,
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 3.0,
            reason: 4,
        },
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let replay = event_replay::replay(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    let mut host = EventHost::new(2).unwrap();
    let obs = host
        .process(
            &mut db,
            worker,
            &candidate,
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    (db, candidate, replay.run_id, obs.id)
}

#[test]
fn container_comparison_assertion_is_checkpoint_guarded_and_never_a_review() {
    let (mut db, _candidate, replay, observation) = fixture();
    let checkpoint = db.ledger_checkpoint().unwrap();
    assert!(db
        .record_container_comparison_assertion(
            replay,
            0,
            "operator",
            &"a".repeat(64),
            &"b".repeat(64),
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db
        .record_container_comparison_assertion(
            replay,
            observation,
            "operator",
            "bad",
            &"b".repeat(64),
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let seq = db
        .record_container_comparison_assertion(
            replay,
            observation,
            "operator",
            &"a".repeat(64),
            &"b".repeat(64),
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
    let assertion = db.inspect_container_comparison_assertion(seq).unwrap();
    assert_eq!(assertion["promotion_evidence"], false);
    assert_eq!(assertion["evidence_origin"], "caller_asserted");
    assert_eq!(assertion["replay_id"], replay);
    assert!(db
        .record_container_comparison_assertion(
            replay,
            observation,
            "operator",
            &"a".repeat(64),
            &"b".repeat(64),
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db.inspect_event_review(seq).is_err());
}

#[test]
fn container_comparison_capture_retains_report_and_receipt_bytes() {
    let (mut db, _, replay, observation) = fixture();
    let checkpoint = db.ledger_checkpoint().unwrap();
    assert!(db
        .record_container_comparison_capture(
            replay,
            observation,
            "operator",
            b"{}",
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let bundle = serde_json::json!({"schema":"sese-synthetic-container-comparison-bundle-v1",
        "replay_report":"{\"schema\":\"replay\"}", "shadow_report":"{\"schema\":\"shadow\"}",
        "receipts":["{\"schema\":\"receipt\"}"]})
    .to_string();
    assert!(db
        .record_container_comparison_capture(
            replay,
            observation,
            "operator",
            &vec![b'x'; 4_194_305],
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let seq = db
        .record_container_comparison_capture(
            replay,
            observation,
            "operator",
            bundle.as_bytes(),
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
    let saved = db.inspect_container_comparison_assertion(seq).unwrap();
    assert_eq!(saved["evidence_origin"], "caller_asserted");
    assert_eq!(saved["promotion_evidence"], false);
    assert_eq!(saved["bundle"]["receipts"][0], "{\"schema\":\"receipt\"}");
    assert_eq!(saved["bundle"]["replay_report"], "{\"schema\":\"replay\"}");
}

#[test]
fn container_comparison_assertion_survives_restart_and_detects_tampering() {
    let path = std::env::temp_dir().join(format!(
        "sese-container-assertion-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (mut db, _, replay, observation) = fixture_with(Storage::open(&path).unwrap());
    let checkpoint = db.ledger_checkpoint().unwrap();
    let seq = db
        .record_container_comparison_assertion(
            replay,
            observation,
            "operator",
            &"a".repeat(64),
            &"b".repeat(64),
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    assert_eq!(
        db.inspect_container_comparison_assertion(seq).unwrap()["evidence_origin"],
        "caller_asserted"
    );
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE ledger_events SET payload=payload || 'x' WHERE seq=?1",
        [seq],
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    assert!(db.inspect_container_comparison_assertion(seq).is_err());
    drop(db);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn explicit_event_review_binds_matching_evidence_and_card() {
    let (mut db, candidate, replay, shadow) = fixture();
    let review = db
        .record_event_review(
            &candidate, replay, shadow, "operator", "approve", "human", "bridge", "proposal",
        )
        .unwrap();
    let value = db.inspect_event_review(review).unwrap();
    assert_eq!(value["candidate_id"], candidate);
    assert_eq!(value["action"], "approve");
    assert_eq!(value["card"]["proposal"], "proposal");
    assert_eq!(value["replay"]["passed"], true);
    let replay_worker = value["replay"]["worker"]["sha256"].as_str().unwrap();
    assert_eq!(replay_worker.len(), 64);
    assert_eq!(value["observation"]["worker"]["sha256"], replay_worker);
    assert_eq!(value["replay"]["cases"][0]["actual"]["score"], 3.0);
    assert_eq!(value["outcome"], "pending");
}
#[test]
fn review_rejects_wrong_candidate_and_incomplete_fields() {
    let (mut db, candidate, replay, shadow) = fixture();
    let other=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    assert!(db
        .record_event_review(
            &other, replay, shadow, "operator", "approve", "human", "bridge", "proposal"
        )
        .is_err());
    assert!(db
        .record_event_review(
            &candidate, replay, shadow, "operator", "approve", "human", "", "proposal"
        )
        .is_err());
}

#[test]
fn failed_replay_and_shadow_trap_cannot_be_approved() {
    let (mut db, candidate, passed_replay, observation) = fixture();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let window = Window {
        count: 0,
        mean: 0.0,
        p99_us: 0,
        last_error: 0,
    };
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 3.0,
        flags: 4,
    };
    let bad = Case {
        id: "wrong".into(),
        window,
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 99.0,
            reason: 4,
        },
    };
    let run = event_replay::replay(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[bad],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(db
        .record_event_review(
            &candidate,
            run.run_id,
            observation,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal"
        )
        .is_err());
    assert!(db
        .record_event_review(
            &candidate,
            run.run_id,
            observation,
            "operator",
            "reject",
            "human",
            "bridge",
            "proposal"
        )
        .is_ok());
    let trap=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) unreachable))").unwrap()).unwrap();
    let expected = Case {
        id: "trap".into(),
        window,
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 3.0,
            reason: 4,
        },
    };
    let failed = event_replay::replay(
        &mut db,
        worker,
        &trap,
        "v1",
        &[expected],
        Duration::from_secs(2),
    )
    .unwrap();
    let mut host = EventHost::new(1).unwrap();
    let obs = host
        .process(
            &mut db,
            worker,
            &candidate,
            &trap,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(obs.shadow_error.is_some());
    let champion_failed = host
        .process(
            &mut db,
            worker,
            &trap,
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(champion_failed.champion_error.is_some());
    assert!(db
        .record_event_review(
            &candidate,
            passed_replay,
            champion_failed.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal"
        )
        .is_err());
    assert!(db
        .record_event_review(
            &trap,
            failed.run_id,
            obs.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal"
        )
        .is_err());
}

#[test]
fn cli_records_and_inspects_review_across_processes() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-review-cli-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (db, candidate, replay, observation) = fixture_with(Storage::open(&path).unwrap());
    drop(db);
    let pin = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-pin",
            path.to_str().unwrap(),
            "domain",
            &replay.to_string(),
            "operator",
        ])
        .output()
        .unwrap();
    assert!(pin.status.success());
    let matched = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-pin-match",
            path.to_str().unwrap(),
            "domain",
            &replay.to_string(),
        ])
        .output()
        .unwrap();
    assert!(matched.status.success());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-review",
            path.to_str().unwrap(),
            &candidate,
            &replay.to_string(),
            &observation.to_string(),
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["promoted"], false);
    let id = value["review_id"].as_i64().unwrap().to_string();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-inspect", path.to_str().unwrap(), &id])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["card"]["human"], "human");
    let resolved = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-resolve",
            path.to_str().unwrap(),
            &id,
            "falsified",
            "auditor",
            "synthetic follow-up",
        ])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    let exported = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-export", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(exported.status.success());
    let row: serde_json::Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(row["outcome"], "falsified");
    assert_eq!(row["outcome_source"], "synthetic follow-up");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE event_fixture_pins SET actor='changed' WHERE domain='domain'",
        [],
    )
    .unwrap();
    let tampered = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-pin-match",
            path.to_str().unwrap(),
            "domain",
            &replay.to_string(),
        ])
        .output()
        .unwrap();
    assert!(!tampered.status.success());
    conn.execute(
        "UPDATE event_fixture_pins SET actor='operator' WHERE domain='domain'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE event_review_outcomes SET source='altered' WHERE review_id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    let denied = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-export", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    conn.execute(
        "UPDATE event_review_outcomes SET source='synthetic follow-up' WHERE review_id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    conn.execute(
        "UPDATE event_reviews SET human='altered' WHERE id=?1",
        [id.parse::<i64>().unwrap()],
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-inspect", path.to_str().unwrap(), &id])
        .output()
        .unwrap();
    assert!(!output.status.success());
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn outcome_and_export_distinguish_pending_and_resolved() {
    let (mut db, candidate, replay, observation) = fixture();
    let review = db
        .record_event_review(
            &candidate,
            replay,
            observation,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    assert_eq!(db.event_outcome_status(review).unwrap(), "pending");
    assert!(db.export_event_outcomes_jsonl().unwrap().is_empty());
    assert!(db
        .resolve_event_review(review, "validated", "", "source")
        .is_err());
    db.resolve_event_review(review, "validated", "auditor", "synthetic follow-up")
        .unwrap();
    assert!(db
        .resolve_event_review(review, "falsified", "auditor", "source")
        .is_err());
    let v: serde_json::Value =
        serde_json::from_str(db.export_event_outcomes_jsonl().unwrap().trim()).unwrap();
    assert_eq!(v["candidate_id"], candidate);
    assert_eq!(v["review_id"], review);
    assert_eq!(v["outcome_source"], "synthetic follow-up");
    assert_eq!(v["outcome_evidence"], "caller_asserted");
    assert_eq!(v["synthetic"], true);
    assert_eq!(v["training_ready"], false);
    assert_eq!(v["synthetic_followup_receipts"], serde_json::json!([]));
    assert_eq!(v["state_semantics"], "legacy-v1");
    assert_eq!(v["replay_worker_sha256"], v["observation_worker_sha256"]);
    assert_eq!(v["outcome"], "validated");
    assert_eq!(
        db.inspect_event_review(review).unwrap()["outcome"],
        "validated"
    );
}

#[test]
fn event_fixture_pin_requires_exact_verified_replay() {
    let (mut db, candidate, replay, _observation) = fixture();
    db.pin_event_fixture("synthetic-domain", replay, "operator")
        .unwrap();
    assert!(db
        .event_replay_matches_pin("synthetic-domain", replay)
        .unwrap());
    assert!(db
        .pin_event_fixture("synthetic-domain", replay, "operator")
        .is_err());
    let case = Case {
        id: "different".into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event: Event {
            timestamp_ns: 1,
            entity_id: 2,
            metric: 3.0,
            flags: 4,
        },
        expected: Verdict {
            decision: Decision::Allow,
            score: 3.0,
            reason: 4,
        },
    };
    let other = event_replay::replay(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!db
        .event_replay_matches_pin("synthetic-domain", other.run_id)
        .unwrap());
    assert!(db.event_replay_matches_pin("missing", replay).is_err());
}

#[test]
fn event_promotion_requires_pinned_review_and_supports_rollback() {
    let (mut db, candidate, replay, _) = fixture();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    assert!(db
        .bootstrap_event_champion("domain", &champion, "operator")
        .is_err());
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let mut measured = sese_runtime_spike::event_host::MeasuredEventHost::new(1).unwrap();
    assert!(measured
        .process_domain(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            "domain",
            &candidate,
            &event,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(measured.snapshot().count, 0);
    let mut host = EventHost::open(&mut db, "live", 1).unwrap();
    let observation = host
        .process_domain(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(observation.champion.unwrap().decision, Decision::Throttle);
    let rejected = db
        .record_event_review(
            &candidate,
            replay,
            observation.id,
            "operator",
            "reject",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    assert!(db
        .promote_event("domain", &champion, rejected, "operator")
        .is_err());
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    let approved = db
        .record_event_review(
            &candidate,
            replay,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    assert!(db
        .promote_event("domain", &candidate, approved, "operator")
        .is_err());
    db.promote_event("domain", &champion, approved, "operator")
        .unwrap();
    assert_eq!(db.current_event_champion("domain").unwrap(), candidate);
    let promoted = host
        .process_domain(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            "domain",
            &champion,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(promoted.champion.unwrap().decision, Decision::Allow);
    db.rollback_event("domain", &candidate, "operator").unwrap();
    let restored = host
        .process_domain(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(restored.champion.unwrap().decision, Decision::Throttle);
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    assert!(db.rollback_event("domain", &champion, "operator").is_err());
}

#[test]
fn newer_failed_event_replay_blocks_promotion() {
    let (mut db, candidate, replay, _) = fixture();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let mut host = EventHost::new(1).unwrap();
    let observation = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let approval = db
        .record_event_review(
            &candidate,
            replay,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    let bad = Case {
        id: "bad".into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 9.0,
            reason: 4,
        },
    };
    assert!(
        !event_replay::replay(
            &mut db,
            worker,
            &candidate,
            "v1",
            &[bad],
            Duration::from_secs(2)
        )
        .unwrap()
        .passed
    );
    assert!(db
        .promote_event("domain", &champion, approval, "operator")
        .is_err());
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
}

#[cfg(unix)]
#[test]
fn event_rollback_during_execution_rejects_stale_observation() {
    use std::time::Instant;
    let base = std::env::temp_dir().join(format!(
        "sese-event-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let path = base.join("data.db");
    let (mut db, candidate, _, _) = fixture_with(Storage::open(&path).unwrap());
    let (script, armed, marker, release) = gated_worker(&base);
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
    let replay = event_replay::replay(
        &mut db,
        script.to_str().unwrap(),
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap()
    .run_id;
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let mut host = EventHost::new(1).unwrap();
    let observation = host
        .process_domain(
            &mut db,
            script.to_str().unwrap(),
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let approval = db
        .record_event_review(
            &candidate,
            replay,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    db.promote_event("domain", &champion, approval, "operator")
        .unwrap();
    let initial_count: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    std::fs::write(&armed, b"go").unwrap();
    let path2 = path.clone();
    let script2 = script.clone();
    let shadow = champion.clone();
    let task = std::thread::spawn(move || {
        let mut db = Storage::open(&path2).unwrap();
        let mut host = EventHost::open(&mut db, "race", 2).unwrap();
        let result = host.process_domain(
            &mut db,
            script2.to_str().unwrap(),
            "domain",
            &shadow,
            &event,
            0,
            Duration::from_secs(3),
        );
        (result.err(), host.snapshot(0).count)
    });
    let start = Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let reached = marker.exists();
    let rollback = if reached {
        db.rollback_event("domain", &candidate, "operator")
    } else {
        Err("worker did not start".into())
    };
    std::fs::write(&release, b"go").unwrap();
    let (failed, count) = task.join().unwrap();
    assert!(reached);
    rollback.unwrap();
    assert!(failed.unwrap().contains("stale champion"));
    assert_eq!(count, 0);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM event_streams WHERE name='race'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(generation, 0);
    let final_count: i64 = conn
        .query_row("SELECT count(*) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(final_count, initial_count);
    drop(conn);
    drop(db);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn worker_timeout_crash_and_resource_exhaustion_block_promotion() {
    for failure in ["timeout", "crash", "fuel", "memory", "oversized_memory"] {
        let mut db =
            Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
        let candidate=db.register_synthetic_event(&wat::parse_str("(module (memory 1) (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) local.get 7 i32.const 9 i32.eq if (loop br 0) end local.get 7 i32.const 10 i32.eq if i32.const 10 memory.grow drop end local.get 7 i32.const 11 i32.eq if i32.const -1 memory.grow drop end i32.const 0 local.get 6 local.get 7))").unwrap()).unwrap();
        let worker = env!("CARGO_BIN_EXE_event-worker");
        let window = Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        };
        let event = Event {
            timestamp_ns: 1,
            entity_id: 2,
            metric: 3.0,
            flags: 4,
        };
        let good = Case {
            id: "good".into(),
            window,
            event,
            expected: Verdict {
                decision: Decision::Allow,
                score: 3.0,
                reason: 4,
            },
        };
        let run = event_replay::replay(
            &mut db,
            worker,
            &candidate,
            "v1",
            &[good],
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(run.passed);
        db.pin_event_fixture("domain", run.run_id, "operator")
            .unwrap();
        db.bootstrap_event_champion("domain", &champion, "operator")
            .unwrap();
        let mut host = EventHost::new(1).unwrap();
        let observation = host
            .process_domain(
                &mut db,
                worker,
                "domain",
                &candidate,
                &event,
                0,
                Duration::from_secs(2),
            )
            .unwrap();
        let review = db
            .record_event_review(
                &candidate,
                run.run_id,
                observation.id,
                "operator",
                "approve",
                "human",
                "bridge",
                "proposal",
            )
            .unwrap();
        let failed_event = Event {
            flags: match failure {
                "fuel" => 9,
                "memory" => 10,
                "oversized_memory" => 11,
                _ => 4,
            },
            ..event
        };
        let failed_case = Case {
            id: "failed".into(),
            window,
            event: failed_event,
            expected: Verdict {
                decision: Decision::Allow,
                score: 3.0,
                reason: failed_event.flags,
            },
        };
        let mut skipped_case = failed_case.clone();
        skipped_case.id = "skipped".into();
        let failed = event_replay::replay(
            &mut db,
            if failure == "crash" {
                "/usr/bin/false"
            } else {
                worker
            },
            &candidate,
            "v1",
            &[failed_case, skipped_case],
            if failure == "timeout" {
                Duration::ZERO
            } else {
                Duration::from_secs(2)
            },
        )
        .unwrap();
        assert!(!failed.passed, "{failure}");
        assert!(failed.cases[0].error.is_some(), "{failure}");
        assert_eq!(
            failed.cases[1].error.as_deref(),
            Some("not run after earlier worker failure"),
            "{failure}"
        );
        assert!(
            db.promote_event("domain", &champion, review, "operator")
                .is_err(),
            "{failure}"
        );
        assert_eq!(
            db.current_event_champion("domain").unwrap(),
            champion,
            "{failure}"
        );
    }
}

#[cfg(unix)]
#[test]
fn started_worker_timing_out_on_new_replay_blocks_prior_approval() {
    assert_started_worker_failure_blocks_prior_approval(false);
}

#[cfg(unix)]
#[test]
fn signal_killed_worker_on_new_replay_blocks_prior_approval() {
    assert_started_worker_failure_blocks_prior_approval(true);
}

#[cfg(unix)]
fn assert_started_worker_failure_blocks_prior_approval(kill_by_signal: bool) {
    let base = std::env::temp_dir().join(format!(
        "sese-event-timeout-promotion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let (script, armed, started, _) = gated_worker(&base);
    if kill_by_signal {
        let quote =
            |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
        let actual = std::path::Path::new(env!("CARGO_BIN_EXE_event-worker"));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ -e {} ]; then : > {}; kill -KILL $$; fi\nexec {}\n",
                quote(&armed),
                quote(&started),
                quote(actual)
            ),
        )
        .unwrap();
    }
    let worker = script.to_str().unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champion = db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    let candidate = db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap()).unwrap();
    let window = Window {
        count: 0,
        mean: 0.0,
        p99_us: 0,
        last_error: 0,
    };
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let case = Case {
        id: "case".into(),
        window,
        event,
        expected: Verdict {
            decision: Decision::Allow,
            score: 3.0,
            reason: 4,
        },
    };
    let run = event_replay::replay(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[case.clone()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(run.passed);
    db.pin_event_fixture("domain", run.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let observation = EventHost::new(1)
        .unwrap()
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate,
            run.run_id,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    std::fs::write(&armed, b"armed").unwrap();
    let failed = event_replay::replay(
        &mut db,
        worker,
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(
        started.exists(),
        "the worker must actually start before timing out"
    );
    assert!(!failed.passed);
    let error = failed.cases[0].error.as_deref().unwrap();
    if kill_by_signal {
        assert!(!error.contains("deadline exceeded"), "{error}");
    } else {
        assert!(error.contains("deadline exceeded"), "{error}");
    }
    assert!(!db.verify_event_replay(failed.run_id).unwrap().passed);
    assert!(db
        .promote_event("domain", &champion, review, "operator")
        .is_err());
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    assert!(db.verify_ledger().unwrap());
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn event_promotion_ledger_failure_rolls_back_and_reopen_detects_tampering() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-promotion-atomic-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (mut db, candidate, replay, _) = fixture_with(Storage::open(&path).unwrap());
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let mut host = EventHost::new(1).unwrap();
    let observation = host
        .process_domain(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate,
            replay,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_event_promotion BEFORE INSERT ON ledger_events WHEN NEW.kind='event_champion_transition' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(db
        .promote_event("domain", &champion, review, "operator")
        .is_err());
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    assert!(db.verify_ledger().unwrap());
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_champion_transitions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    conn.execute_batch("DROP TRIGGER fail_event_promotion")
        .unwrap();
    db.promote_event("domain", &champion, review, "operator")
        .unwrap();
    drop(db);
    let mut reopened = Storage::open(&path).unwrap();
    assert_eq!(
        reopened.current_event_champion("domain").unwrap(),
        candidate
    );
    let worker_hash: String = conn
        .query_row(
            "SELECT worker_sha256 FROM event_observations WHERE id=?1",
            [observation.id],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE event_observations SET worker_sha256=?1 WHERE id=?2",
        rusqlite::params!["0".repeat(64), observation.id],
    )
    .unwrap();
    assert!(!reopened.verify_ledger().unwrap());
    conn.execute(
        "UPDATE event_observations SET worker_sha256=?1 WHERE id=?2",
        rusqlite::params![worker_hash, observation.id],
    )
    .unwrap();
    assert!(reopened.verify_ledger().unwrap());
    conn.execute(
        "UPDATE event_observations SET domain='different' WHERE id=?1",
        [observation.id],
    )
    .unwrap();
    assert!(!reopened.verify_ledger().unwrap());
    conn.execute(
        "UPDATE event_observations SET domain='domain' WHERE id=?1",
        [observation.id],
    )
    .unwrap();
    assert!(reopened.verify_ledger().unwrap());
    conn.execute(
        "UPDATE event_champions SET champion_id=?1 WHERE domain='domain'",
        [&champion],
    )
    .unwrap();
    assert!(reopened.current_event_champion("domain").is_err());
    assert!(!reopened.verify_ledger().unwrap());
    drop(conn);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn measured_observations_cannot_be_reviewed_against_legacy_replay() {
    let (mut db, candidate, replay, _) = fixture();
    let mut host = sese_runtime_spike::event_host::MeasuredEventHost::new(2).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let obs = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            &candidate,
            &event,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(db
        .record_event_review(
            &candidate, replay, obs.id, "operator", "approve", "human", "bridge", "proposal"
        )
        .is_err());
    assert!(db
        .record_event_review(
            &candidate, replay, obs.id, "operator", "reject", "human", "bridge", "proposal"
        )
        .is_err());
}

#[test]
fn measured_replay_can_review_measured_observation_but_not_legacy() {
    let (mut db, candidate, legacy, legacy_observation) = fixture();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let mut host = sese_runtime_spike::event_host::MeasuredEventHost::new(2).unwrap();
    let observation = host
        .process(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            &candidate,
            &event,
            Duration::from_secs(2),
        )
        .unwrap();
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
    let measured = event_replay::replay_measured(
        &mut db,
        env!("CARGO_BIN_EXE_event-worker"),
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    let a = db.verify_event_replay(legacy).unwrap();
    let b = db.verify_event_replay(measured.run_id).unwrap();
    assert_eq!(b.state_semantics, "measured-v2");
    assert_ne!(a.pack_digest, b.pack_digest);
    assert!(db
        .record_event_review(
            &candidate,
            legacy,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal"
        )
        .is_err());
    assert!(db
        .record_event_review(
            &candidate,
            measured.run_id,
            legacy_observation,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal"
        )
        .is_err());
    let id = db
        .record_event_review(
            &candidate,
            measured.run_id,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    assert_eq!(
        db.inspect_event_review(id).unwrap()["replay"]["state_semantics"],
        "measured-v2"
    );
}

#[test]
fn legacy_replay_survives_semantics_migration_and_tampering_is_detected() {
    let path = std::env::temp_dir().join(format!(
        "sese-replay-semantics-migration-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (mut db, candidate, replay, _) = fixture_with(Storage::open(&path).unwrap());
    let digest = db.verify_event_replay(replay).unwrap().pack_digest;
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    // Reconstruct the pre-v29 chain, which had no synthetic-candidate events.
    use sha2::{Digest, Sha256};
    conn.execute(
        "DELETE FROM ledger_events WHERE kind IN ('synthetic_event_candidate','event_replay_worker','event_observation_worker','event_replay_timing')",
        [],
    )
    .unwrap();
    let entries: Vec<(i64, String, i64, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT seq,kind,reference,payload,created_ns FROM ledger_events ORDER BY seq")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        rows.collect::<Result<_, _>>().unwrap()
    };
    let mut previous = "GENESIS".to_string();
    for (index, (old, kind, reference, payload, time)) in entries.iter().enumerate() {
        let fields = [
            "sese-ledger-v1",
            previous.as_str(),
            kind.as_str(),
            &reference.to_string(),
            payload.as_str(),
            time.as_str(),
        ];
        let encoded: String = fields
            .iter()
            .map(|s| format!("{}:{}", s.len(), s))
            .collect();
        let hash = format!("{:x}", Sha256::digest(encoded.as_bytes()));
        conn.execute(
            "UPDATE ledger_events SET seq=?1,prev=?2,hash=?3 WHERE seq=?4",
            rusqlite::params![index as i64 + 1, previous, hash, old],
        )
        .unwrap();
        previous = hash;
    }
    conn.execute_batch("DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts; DROP TABLE event_followup_receipts; DROP TABLE event_followup_pins")
        .unwrap();
    conn.execute_batch(
        "DROP TABLE event_training_traces; DROP TABLE synthetic_scalar_candidates; DROP TABLE synthetic_window_candidates; DROP TABLE synthetic_event_candidates; ALTER TABLE event_replay_runs DROP COLUMN state_semantics; ALTER TABLE event_replay_runs DROP COLUMN worker_path; ALTER TABLE event_replay_runs DROP COLUMN worker_sha256; ALTER TABLE event_observations DROP COLUMN worker_path; ALTER TABLE event_observations DROP COLUMN worker_sha256; ALTER TABLE event_observations DROP COLUMN domain; PRAGMA user_version=26;",
    )
    .unwrap();
    drop(conn);
    let mut db = Storage::open(&path).unwrap();
    let restored = db.verify_event_replay(replay).unwrap();
    assert_eq!(restored.state_semantics, "legacy-v1");
    assert_eq!(restored.pack_digest, digest);
    let bytes = db.artifact(&candidate).unwrap();
    assert!(db.register_synthetic_event(&bytes).is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE event_replay_runs SET state_semantics='measured-v2' WHERE id=?1",
        [replay],
    )
    .unwrap();
    assert!(db.verify_event_replay(replay).is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn measured_domain_promotes_and_rolls_back_with_semantics_guard() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let original=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap()).unwrap();
    let candidate=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))").unwrap()).unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: -3.0,
        flags: 4,
    };
    let case = Case {
        id: "measured".into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event,
        expected: Verdict {
            decision: Decision::Throttle,
            score: -3.0,
            reason: 4,
        },
    };
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "v2",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &original, "operator")
        .unwrap();
    let mut legacy = EventHost::new(2).unwrap();
    assert!(legacy
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    let mut host =
        sese_runtime_spike::event_host::MeasuredEventHost::open(&mut db, "measured", 2).unwrap();
    let observation = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(observation.champion.unwrap().decision, Decision::Allow);
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
    db.promote_event("domain", &original, review, "operator")
        .unwrap();
    let promoted = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &original,
            &event,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(promoted.champion.unwrap().decision, Decision::Throttle);
    db.rollback_event("domain", &candidate, "operator").unwrap();
    let restored = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(restored.champion.unwrap().decision, Decision::Allow);
    assert_eq!(host.snapshot().count, 3);
}

#[cfg(unix)]
#[test]
fn measured_domain_rejects_rollback_during_execution() {
    use std::time::Instant;
    let base = std::env::temp_dir().join(format!(
        "sese-measured-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let path = base.join("data.db");
    let mut db = Storage::open(&path).unwrap();
    let original=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap()).unwrap();
    let candidate=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))").unwrap()).unwrap();
    let (script, armed, marker, release) = gated_worker(&base);
    let worker = script.to_str().unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 1,
        metric: 3.0,
        flags: 0,
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
            score: 3.0,
            reason: 0,
        },
    };
    let replay = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "v2",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    db.pin_event_fixture("domain", replay.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("domain", &original, "operator")
        .unwrap();
    let mut host = sese_runtime_spike::event_host::MeasuredEventHost::new(1).unwrap();
    let obs = host
        .process_domain(
            &mut db,
            worker,
            "domain",
            &candidate,
            &event,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate,
            replay.run_id,
            obs.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    db.promote_event("domain", &original, review, "operator")
        .unwrap();
    std::fs::write(&armed, b"go").unwrap();
    let path2 = path.clone();
    let script2 = script.clone();
    let shadow = original.clone();
    let task = std::thread::spawn(move || {
        let mut db = Storage::open(&path2).unwrap();
        let mut host =
            sese_runtime_spike::event_host::MeasuredEventHost::open(&mut db, "race", 2).unwrap();
        let result = host.process_domain(
            &mut db,
            script2.to_str().unwrap(),
            "domain",
            &shadow,
            &event,
            Duration::from_secs(3),
        );
        (result.err(), host.snapshot().count)
    });
    let start = Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let reached = marker.exists();
    let rollback = if reached {
        db.rollback_event("domain", &candidate, "operator")
    } else {
        Err("worker did not start".into())
    };
    std::fs::write(&release, b"go").unwrap();
    let (failed, count) = task.join().unwrap();
    assert!(reached);
    rollback.unwrap();
    assert!(failed.unwrap().contains("stale champion"));
    assert_eq!(count, 0);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM measured_streams WHERE name='race'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(generation, 0);
    assert!(db.verify_ledger().unwrap());
    drop(conn);
    drop(db);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn observation_from_one_domain_cannot_authorize_another_domain() {
    let (mut db, candidate, replay, _) = fixture();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    for domain in ["alpha", "beta"] {
        db.pin_event_fixture(domain, replay, "operator").unwrap();
        db.bootstrap_event_champion(domain, &champion, "operator")
            .unwrap();
    }
    let mut host = EventHost::new(2).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let obs = host
        .process_domain(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            "alpha",
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate, replay, obs.id, "operator", "approve", "human", "bridge", "proposal",
        )
        .unwrap();
    assert_eq!(
        db.inspect_event_review(review).unwrap()["observation"]["domain"],
        "alpha"
    );
    assert!(db
        .promote_event("beta", &champion, review, "operator")
        .is_err());
    assert_eq!(db.current_event_champion("beta").unwrap(), champion);
    db.promote_event("alpha", &champion, review, "operator")
        .unwrap();
    assert_eq!(db.current_event_champion("alpha").unwrap(), candidate);
}

#[test]
fn durable_event_window_cannot_be_reused_across_domains() {
    let (mut db, candidate, replay, _) = fixture();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    for domain in ["alpha", "beta"] {
        db.pin_event_fixture(domain, replay, "operator").unwrap();
        db.bootstrap_event_champion(domain, &champion, "operator")
            .unwrap();
    }
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let mut host = EventHost::open(&mut db, "shared", 2).unwrap();
    host.process_domain(
        &mut db,
        worker,
        "alpha",
        &candidate,
        &event,
        0,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(host
        .process_domain(
            &mut db,
            worker,
            "beta",
            &candidate,
            &event,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(host.snapshot(0).count, 1);
    drop(host);
    let mut reopened = EventHost::open(&mut db, "shared", 2).unwrap();
    assert!(reopened
        .process_domain(
            &mut db,
            worker,
            "beta",
            &candidate,
            &event,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(reopened.snapshot(0).count, 1);
}

#[test]
fn measured_window_rejects_cross_domain_and_unscoped_reuse() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
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
        &mut db,
        worker,
        &candidate,
        "v2",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    for domain in ["alpha", "beta"] {
        db.pin_event_fixture(domain, replay.run_id, "operator")
            .unwrap();
        db.bootstrap_event_champion(domain, &candidate, "operator")
            .unwrap();
    }
    let mut host =
        sese_runtime_spike::event_host::MeasuredEventHost::open(&mut db, "shared", 2).unwrap();
    host.process_domain(
        &mut db,
        worker,
        "alpha",
        &candidate,
        &event,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(host
        .process_domain(
            &mut db,
            worker,
            "beta",
            &candidate,
            &event,
            Duration::from_secs(2)
        )
        .is_err());
    assert!(host
        .process(
            &mut db,
            worker,
            &candidate,
            &candidate,
            &event,
            Duration::from_secs(2)
        )
        .is_err());
    drop(host);
    let mut reopened =
        sese_runtime_spike::event_host::MeasuredEventHost::open(&mut db, "shared", 2).unwrap();
    assert!(reopened
        .process_domain(
            &mut db,
            worker,
            "beta",
            &candidate,
            &event,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(reopened.snapshot().count, 1);
}

#[cfg(unix)]
#[test]
fn promotion_rejects_different_replay_and_observation_workers() {
    use std::os::unix::fs::PermissionsExt;
    let (mut db, candidate, replay, _) = fixture();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let path = std::env::temp_dir().join(format!(
        "sese-wrapper-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let escaped = worker.replace('\'', "'\\''");
    std::fs::write(&path, format!("#!/bin/sh\nexec '{escaped}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = EventHost::new(1).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let observation = host
        .process(
            &mut db,
            path.to_str().unwrap(),
            &champion,
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate,
            replay,
            observation.id,
            "operator",
            "approve",
            "human",
            "bridge",
            "proposal",
        )
        .unwrap();
    assert!(db
        .promote_event("domain", &champion, review, "operator")
        .unwrap_err()
        .contains("worker provenance missing or mismatched"));
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn promotion_rejects_worker_different_from_fixture_selection() {
    use std::os::unix::fs::PermissionsExt;
    let (mut db, candidate, replay, _) = fixture();
    let champion=db.register_synthetic_event(&wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 f64.const 0 i32.const 0))").unwrap()).unwrap();
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &champion, "operator")
        .unwrap();
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let path = std::env::temp_dir().join(format!(
        "sese-fixture-worker-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let escaped = worker.replace('\'', "'\\''");
    std::fs::write(&path, format!("#!/bin/sh\nexec '{escaped}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
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
    let new = event_replay::replay(
        &mut db,
        path.to_str().unwrap(),
        &candidate,
        "v1",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(db.event_replay_matches_pin("domain", new.run_id).unwrap());
    let mut host = EventHost::new(1).unwrap();
    let obs = host
        .process(
            &mut db,
            path.to_str().unwrap(),
            &champion,
            &candidate,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let review = db
        .record_event_review(
            &candidate, new.run_id, obs.id, "operator", "approve", "human", "bridge", "proposal",
        )
        .unwrap();
    assert!(db
        .promote_event("domain", &champion, review, "operator")
        .unwrap_err()
        .contains("worker differs from fixture selection"));
    assert_eq!(db.current_event_champion("domain").unwrap(), champion);
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn domain_host_rejects_worker_other_than_fixture_selection() {
    use std::os::unix::fs::PermissionsExt;
    let (mut db, candidate, replay, _) = fixture();
    db.pin_event_fixture("domain", replay, "operator").unwrap();
    db.bootstrap_event_champion("domain", &candidate, "operator")
        .unwrap();
    let path = std::env::temp_dir().join(format!(
        "sese-domain-worker-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let escaped = worker.replace('\'', "'\\''");
    std::fs::write(&path, format!("#!/bin/sh\nexec '{escaped}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 2,
        metric: 3.0,
        flags: 4,
    };
    let mut host = EventHost::new(1).unwrap();
    assert!(host
        .process_domain(
            &mut db,
            path.to_str().unwrap(),
            "domain",
            &candidate,
            &event,
            0,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(host.snapshot(0).count, 0);
    let case = Case {
        id: "measured".into(),
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
    let measured = event_replay::replay_measured(
        &mut db,
        worker,
        &candidate,
        "v2",
        &[case],
        Duration::from_secs(2),
    )
    .unwrap();
    db.pin_event_fixture("measured", measured.run_id, "operator")
        .unwrap();
    db.bootstrap_event_champion("measured", &candidate, "operator")
        .unwrap();
    let mut host = sese_runtime_spike::event_host::MeasuredEventHost::new(1).unwrap();
    assert!(host
        .process_domain(
            &mut db,
            path.to_str().unwrap(),
            "measured",
            &candidate,
            &event,
            Duration::from_secs(2)
        )
        .is_err());
    assert_eq!(host.snapshot().count, 0);
    std::fs::remove_file(path).unwrap();
}
