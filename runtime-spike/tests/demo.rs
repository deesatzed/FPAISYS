use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn container_comparison_refreshes_prepared_decision_without_an_extra_review_step() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-prepared-container-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("app.db");
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let pin = std::env::var("SESE_TEST_WORKER_SHA256").ok();
    let marker = dir.join("copies");
    let docker = if pin.is_some() {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("docker-wrapper");
        let source = format!("#!/bin/sh\nif [ \"$3\" = cp ]; then printf x >> '{}'; fi\nexec /usr/local/bin/docker \"$@\"\n", marker.display());
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path.to_str().unwrap().to_owned()
    } else {
        "/usr/local/bin/docker".to_owned()
    };
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-measured-synthetic-v1.json"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_event-worker"))
        .arg(&db)
        .args(["--measured", "--prepare-only", "--fixture", fixture])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let prepared = dir.join("prepared.json");
    std::fs::write(&prepared, &output.stdout).unwrap();
    let run_comparison = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sese-outcome"));
        command.args([
            "event-container-evaluate",
            db.to_str().unwrap(),
            "typed-cycle",
            &snapshot["replay_id"].as_i64().unwrap().to_string(),
            &snapshot["observation_id"].as_i64().unwrap().to_string(),
            fixture,
            &image,
            &host,
            journal.to_str().unwrap(),
            "/opt/homebrew/bin/python3",
            &docker,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/scripts/synthetic_container_worker.py"
            ),
            "40",
            prepared.to_str().unwrap(),
        ]);
        if let Some(pin) = &pin {
            command.args(["--worker-sha256", pin]);
        }
        command.output().unwrap()
    };
    let mut stale = snapshot.clone();
    stale["checkpoint"]["hash"] = serde_json::json!("0".repeat(64));
    std::fs::write(&prepared, serde_json::to_vec(&stale).unwrap()).unwrap();
    let rejected = run_comparison();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("no longer matches"));
    assert_eq!(std::fs::read_dir(&journal).unwrap().count(), 0);
    std::fs::write(&prepared, &output.stdout).unwrap();
    let result = run_comparison();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if pin.is_some() {
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            b"xxx",
            "pin checked for replay, champion and shadow"
        );
    }
    let combined: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(combined["promotion_evidence"], false);
    if let Some(pin) = &pin {
        for raw in [
            &combined["container_replay"]["cases"][0]["receipt"],
            &combined["container_shadow"]["champion"]["receipt"],
            &combined["container_shadow"]["shadow"]["receipt"],
        ] {
            let receipt: serde_json::Value = serde_json::from_str(raw.as_str().unwrap()).unwrap();
            assert_eq!(receipt["expected_worker_sha256"], *pin);
            assert_eq!(receipt["worker_pin_status"], "matched");
        }
    }
    assert_eq!(combined["prepared"]["checkpoint"], combined["checkpoint"]);
    let fresh = dir.join("comparison.json");
    std::fs::write(&fresh, result.stdout).unwrap();
    let decide = |file: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-review-prepared",
                db.to_str().unwrap(),
                file.to_str().unwrap(),
                "operator",
                "reject",
            ])
            .output()
            .unwrap()
    };
    assert!(!decide(&prepared).status.success());
    let decision = decide(&fresh);
    assert!(
        decision.status.success(),
        "{}",
        String::from_utf8_lossy(&decision.stderr)
    );
    let decision: serde_json::Value = serde_json::from_slice(&decision.stdout).unwrap();
    assert_eq!(decision["promoted"], false);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepared_approval_rolls_back_review_if_promotion_write_fails() {
    let dir = std::env::temp_dir().join(format!(
        "sese-atomic-review-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("cycle.db");
    let result = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_event-worker"))
        .arg(&db)
        .arg("--prepare-only")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let prepared = dir.join("prepared.json");
    let snapshot: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    std::fs::write(&prepared, result.stdout).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TRIGGER test_deny_promotion BEFORE UPDATE ON event_champions BEGIN SELECT RAISE(ABORT,'injected promotion failure'); END;").unwrap();
    drop(conn);
    let decision = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .arg("event-review-prepared")
        .arg(&db)
        .arg(&prepared)
        .args(["operator", "approve"])
        .output()
        .unwrap();
    assert!(!decision.status.success());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (reviews, transitions): (i64, i64) = conn.query_row("SELECT (SELECT count(*) FROM event_reviews), (SELECT count(*) FROM event_champion_transitions)", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(
        (reviews, transitions),
        (0, 1),
        "failed promotion must not leave an approval"
    );
    let checkpoint: (i64, String) = conn
        .query_row(
            "SELECT seq, hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        checkpoint.0,
        snapshot["checkpoint"]["seq"].as_i64().unwrap()
    );
    assert_eq!(
        checkpoint.1,
        snapshot["checkpoint"]["hash"].as_str().unwrap()
    );
    conn.execute_batch("DROP TRIGGER test_deny_promotion")
        .unwrap();
    drop(conn);
    let retried = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .arg("event-review-prepared")
        .arg(&db)
        .arg(&prepared)
        .args(["operator", "approve"])
        .output()
        .unwrap();
    assert!(
        retried.status.success(),
        "{}",
        String::from_utf8_lossy(&retried.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepared_synthetic_evaluation_finishes_without_a_prompt_and_keeps_evidence() {
    let dir = std::env::temp_dir().join(format!(
        "sese-prepared-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db_path = dir.join("cycle.db");
    let output = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_event-worker"))
        .arg(&db_path)
        .args([
            "--measured",
            "--fixture",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/event-measured-synthetic-v1.json"
            ),
            "--prepare-only",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("Enter approve"));
    let prepared: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(prepared["schema"], "sese-prepared-synthetic-evaluation-v1");
    assert_eq!(prepared["state_semantics"], "measured-v2");
    assert_eq!(prepared["synthetic"], true);
    assert_eq!(prepared["review_recorded"], false);
    let mut db = sese_runtime_spike::storage::Storage::open_read_only(&db_path).unwrap();
    let preview = db
        .event_review_preview(prepared["replay_id"].as_i64().unwrap(), "typed-cycle")
        .unwrap();
    assert_eq!(preview["checkpoint"], prepared["checkpoint"]);
    assert_eq!(preview["replay"]["candidate_id"], prepared["candidate_id"]);
    assert_eq!(preview["observations"][0]["id"], prepared["observation_id"]);
    assert_eq!(
        db.current_event_champion("typed-cycle").unwrap(),
        prepared["champion_id"].as_str().unwrap()
    );
    drop(db);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let (reviews, transitions): (i64, i64) = conn.query_row("SELECT (SELECT count(*) FROM event_reviews), (SELECT count(*) FROM event_champion_transitions)", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((reviews, transitions), (0, 1));
    drop(conn);
    let prepared_path = dir.join("prepared.json");
    std::fs::write(&prepared_path, &text).unwrap();
    let mut old_preview = prepared.clone();
    old_preview["preview"]["replay"]
        .as_object_mut()
        .unwrap()
        .remove("runtime_provenance_verified");
    let old_path = dir.join("old-prepared.json");
    std::fs::write(&old_path, old_preview.to_string()).unwrap();
    let old_decision = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .arg("event-review-prepared")
        .arg(&db_path)
        .arg(&old_path)
        .args(["operator", "approve"])
        .output()
        .unwrap();
    assert!(
        !old_decision.status.success(),
        "old preview must be regenerated"
    );
    let decide = || {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .arg("event-review-prepared")
            .arg(&db_path)
            .arg(&prepared_path)
            .args(["operator", "approve"])
            .output()
            .unwrap()
    };
    let decision = decide();
    assert!(
        decision.status.success(),
        "{}",
        String::from_utf8_lossy(&decision.stderr)
    );
    let decision: serde_json::Value = serde_json::from_slice(&decision.stdout).unwrap();
    assert_eq!(decision["promoted"], true);
    let mut db = sese_runtime_spike::storage::Storage::open_read_only(&db_path).unwrap();
    assert_eq!(
        db.current_event_champion("typed-cycle").unwrap(),
        prepared["candidate_id"].as_str().unwrap()
    );
    drop(db);
    assert!(
        !decide().status.success(),
        "stale preparation must not be reused"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepared_review_rejects_altered_evidence_and_can_record_rejection() {
    let dir = std::env::temp_dir().join(format!(
        "sese-prepared-reject-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("cycle.db");
    let result = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_event-worker"))
        .arg(&db)
        .arg("--prepare-only")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut prepared: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let path = dir.join("prepared.json");
    prepared["preview"]["observations"][0]["event"]["metric"] = 100.into();
    std::fs::write(&path, serde_json::to_vec(&prepared).unwrap()).unwrap();
    let decide = |action: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .arg("event-review-prepared")
            .arg(&db)
            .arg(&path)
            .args(["operator", action])
            .output()
            .unwrap()
    };
    assert!(!decide("approve").status.success());
    std::fs::write(&path, &result.stdout).unwrap();
    let rejected = decide("reject");
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(response["promoted"], false);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (reviews, transitions, action): (i64, i64, String) = conn.query_row("SELECT (SELECT count(*) FROM event_reviews), (SELECT count(*) FROM event_champion_transitions), (SELECT action FROM event_reviews LIMIT 1)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((reviews, transitions, action.as_str()), (1, 1, "reject"));
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepared_evaluation_requires_persistent_storage_and_never_promotes_failed_replay() {
    let demo = env!("CARGO_BIN_EXE_sese-event-cycle-demo");
    let worker = env!("CARGO_BIN_EXE_event-worker");
    let without_path = Command::new(demo)
        .args([worker, "--prepare-only"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!without_path.status.success());
    let dir = std::env::temp_dir().join(format!(
        "sese-prepared-failed-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("cycle.db");
    let failed = Command::new(demo)
        .arg(worker)
        .arg(&db)
        .args([
            "--prepare-only",
            "--candidate",
            "quarantine",
            "--fixture",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/event-synthetic-v1.json"
            ),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(serde_json::from_slice::<serde_json::Value>(&failed.stdout).is_err());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (reviews, transitions): (i64, i64) = conn.query_row("SELECT (SELECT count(*) FROM event_reviews), (SELECT count(*) FROM event_champion_transitions)", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((reviews, transitions), (0, 0));
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn typed_cycle_selects_a_known_synthetic_candidate_and_rejects_wrong_fixture() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let good = std::env::temp_dir().join(format!("sese-choice-good-{suffix}.db"));
    let bad = std::env::temp_dir().join(format!("sese-choice-bad-{suffix}.db"));
    let rejected = std::env::temp_dir().join(format!("sese-choice-rejected-{suffix}.db"));
    let invalid = std::env::temp_dir().join(format!("sese-choice-invalid-{suffix}.db"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            good.to_str().unwrap(),
            "--candidate",
            "quarantine",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Before review: champion=allow shadow=quarantine"));
    assert!(text.contains("After promotion: champion=quarantine"));
    assert!(text.contains("After rollback: champion=allow"));
    let wrong = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            bad.to_str().unwrap(),
            "--candidate",
            "quarantine",
            "--fixture",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/event-synthetic-v1.json"
            ),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    let conn = rusqlite::Connection::open(&bad).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_champions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    let mut reject = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            rejected.to_str().unwrap(),
            "--candidate",
            "quarantine",
            "--measured",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    reject.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let reject = reject.wait_with_output().unwrap();
    assert!(
        reject.status.success(),
        "{}",
        String::from_utf8_lossy(&reject.stderr)
    );
    let conn = rusqlite::Connection::open(&rejected).unwrap();
    let (transitions, action): (i64, String) = conn.query_row("SELECT (SELECT count(*) FROM event_champion_transitions), action FROM event_reviews WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((transitions, action.as_str()), (1, "reject"));
    drop(conn);
    let unknown = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            invalid.to_str().unwrap(),
            "--candidate",
            "not-a-built-in",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(!invalid.exists());
    std::fs::remove_file(good).unwrap();
    std::fs::remove_file(bad).unwrap();
    std::fs::remove_file(rejected).unwrap();
}

#[test]
fn synthetic_demo_records_explicit_rejection_without_promotion() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-demo"))
        .arg(env!("CARGO_BIN_EXE_worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Replay passed: true"));
    assert!(text.contains("Decision recorded: reject"));
    assert!(text.contains("Promotion unavailable"));
}

#[test]
fn window_demo_runs_review_and_requires_explicit_choice() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .arg(env!("CARGO_BIN_EXE_window-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Window replay passed: true"));
    assert!(text.contains("Prior window: [3]"));
    assert!(text.contains("Decision recorded: reject"));
    assert!(text.contains("Promotion unavailable"));
}

#[test]
fn window_demo_does_not_create_review_without_explicit_input() {
    let path = std::env::temp_dir().join(format!(
        "sese-window-demo-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_sese-window-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM window_review_records", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn cycle_demo_approval_and_rollback_change_synthetic_host_output() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_window-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Before review: champion=5 shadow=8"));
    assert!(text.contains("After promotion: champion=7"));
    assert!(text.contains("After rollback: champion=5"));
}
#[test]
fn cycle_demo_rejection_leaves_champion_unchanged() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_window-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Rejected; champion unchanged"));
    assert!(!text.contains("After promotion"));
}
#[test]
fn cycle_demo_refuses_existing_database_path() {
    let path = std::env::temp_dir().join(format!(
        "sese-cycle-existing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    drop(sese_runtime_spike::storage::Storage::open(&path).unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_sese-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_window-worker"), path.to_str().unwrap()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM candidates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn typed_event_demo_approval_rollback_and_explicit_rejection() {
    for (input, expected) in [
        (
            b"approve\nrollback\n".as_slice(),
            "After rollback: champion=allow",
        ),
        (b"reject\n".as_slice(), "Rejected; champion unchanged"),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
            .arg(env!("CARGO_BIN_EXE_event-worker"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Before review: champion=allow shadow=throttle"));
        assert!(text.contains(expected));
        if input.starts_with(b"approve") {
            assert!(text.contains("After promotion: champion=throttle"));
        } else {
            assert!(!text.contains("After promotion"));
        }
    }
}

#[test]
fn typed_demo_persists_review_for_inspection_and_outcome() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-demo-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let result = Command::new(cli)
        .args(["event-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["action"], "reject");
    assert_eq!(value["outcome"], "pending");
    let result = Command::new(cli)
        .args([
            "event-resolve",
            path.to_str().unwrap(),
            "1",
            "validated",
            "auditor",
            "synthetic follow-up",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let result = Command::new(cli)
        .args(["event-export", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["decision"], "reject");
    assert_eq!(value["outcome"], "validated");
    let second = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!second.status.success());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn typed_demo_records_operator_supplied_review_card() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let card = std::env::temp_dir().join(format!("sese-card-{suffix}.json"));
    let db = std::env::temp_dir().join(format!("sese-card-{suffix}.db"));
    std::fs::write(&card, r#"{"schema":"sese-synthetic-review-card-v1","human":"My concern","bridge":"Observed mismatch","proposal":"Prefer throttle"}"#).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db.to_str().unwrap(),
            "--card",
            card.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("My concern")
            && text.contains("Observed mismatch")
            && text.contains("Prefer throttle")
    );
    assert!(text.find("My concern").unwrap() < text.find("Enter approve or reject:").unwrap());
    let inspect = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-inspect", db.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let review: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(review["card"]["human"], "My concern");
    assert_eq!(review["card"]["bridge"], "Observed mismatch");
    assert_eq!(review["card"]["proposal"], "Prefer throttle");
    std::fs::remove_file(&db).unwrap();
    std::fs::write(
        &card,
        r#"{"schema":"sese-synthetic-review-card-v1","human":"","bridge":"ok","proposal":"ok"}"#,
    )
    .unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db.to_str().unwrap(),
            "--card",
            card.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!db.exists());
    std::fs::remove_file(card).unwrap();
}

#[test]
fn typed_demo_requires_explicit_review() {
    let path = std::env::temp_dir().join(format!(
        "sese-event-no-choice-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_reviews", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn typed_demo_rejects_ledger_changes_during_operator_review() {
    use std::io::Read;
    let path = std::env::temp_dir().join(format!(
        "sese-review-race-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut seen = Vec::new();
    while !seen.ends_with(b"Enter approve or reject: ") {
        let mut byte = [0];
        stdout.read_exact(&mut byte).unwrap();
        seen.push(byte[0]);
        assert!(seen.len() < 100_000);
    }
    let other = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    other
        .register_synthetic_event(b"another synthetic identity")
        .unwrap();
    drop(other);
    child.stdin.take().unwrap().write_all(b"approve\n").unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    let mut errors = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    assert!(errors.contains("ledger changed during review"), "{errors}");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (reviews, transitions): (i64,i64) = conn.query_row("SELECT (SELECT count(*) FROM event_reviews),(SELECT count(*) FROM event_champion_transitions)", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((reviews, transitions), (0, 1));
    drop(conn);
    let displayed = String::from_utf8(seen).unwrap();
    let stale: serde_json::Value = serde_json::from_str(
        displayed
            .lines()
            .find_map(|l| l.strip_prefix("Review snapshot: "))
            .unwrap(),
    )
    .unwrap();
    let preview = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-review-preview",
            path.to_str().unwrap(),
            "typed-cycle",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    let candidate = preview["replay"]["candidate_id"].as_str().unwrap();
    let observation = preview["observations"][0]["id"]
        .as_i64()
        .unwrap()
        .to_string();
    let submit = |snapshot: &serde_json::Value, domain: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-review-at",
                path.to_str().unwrap(),
                domain,
                candidate,
                "1",
                &observation,
                "operator",
                "reject",
                "human",
                "bridge",
                "proposal",
                &snapshot["checkpoint"]["seq"].as_i64().unwrap().to_string(),
                snapshot["checkpoint"]["hash"].as_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let stale_result = submit(&stale, "typed-cycle");
    assert!(!stale_result.status.success());
    assert!(String::from_utf8_lossy(&stale_result.stderr).contains("ledger changed during review"));
    let wrong_domain = submit(&preview, "other-domain");
    assert!(!wrong_domain.status.success());
    assert!(String::from_utf8_lossy(&wrong_domain.stderr)
        .contains("observation is not in the specified preview domain"));
    let result = submit(&preview, "typed-cycle");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["promoted"], false);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (reviews, transitions): (i64,i64) = conn.query_row("SELECT (SELECT count(*) FROM event_reviews),(SELECT count(*) FROM event_champion_transitions)", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((reviews, transitions), (1, 1));
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn measured_event_demo_runs_review_promotion_and_persists_state() {
    let path = std::env::temp_dir().join(format!(
        "sese-measured-demo-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            path.to_str().unwrap(),
            "--measured",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("state semantics: measured-v2"));
    assert!(text.contains("After promotion: champion=throttle"));
    assert!(text.contains("After rollback: champion=allow"));
    let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-inspect", path.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let review: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(review["replay"]["state_semantics"], "measured-v2");
    assert_eq!(
        review["observation"]["prior"]["state_semantics"],
        "measured-v2"
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM measured_streams WHERE name='typed-cycle'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(generation, 3);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn measured_event_demo_rejection_leaves_champion_unchanged() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), "--measured"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("state semantics: measured-v2"));
    assert!(text.contains("Rejected; champion unchanged"));
    assert!(!text.contains("After promotion"));
}

#[test]
fn typed_cycle_uses_operator_fixture_and_failed_replay_never_bootstraps() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let fixture_path = std::env::temp_dir().join(format!("sese-cycle-fixture-{suffix}.json"));
    let db_path = std::env::temp_dir().join(format!("sese-cycle-fixture-{suffix}.db"));
    let failed_path = std::env::temp_dir().join(format!("sese-cycle-failed-{suffix}.db"));
    let mismatch_path = std::env::temp_dir().join(format!("sese-cycle-mismatch-{suffix}.db"));
    let mut fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/event-synthetic-v1.json")).unwrap();
    fixture["version"] = "operator-v1".into();
    fixture["cases"][0]["id"] = "operator-case".into();
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db_path.to_str().unwrap(),
            "--fixture",
            fixture_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let version: String = conn
        .query_row("SELECT pack_version FROM event_replay_runs", [], |r| {
            r.get(0)
        })
        .unwrap();
    let case_id: String = conn
        .query_row("SELECT case_id FROM event_replay_cases", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, "operator-v1");
    assert_eq!(case_id, "operator-case");
    let text = String::from_utf8_lossy(&output.stdout);
    let snapshot_line = text
        .lines()
        .find_map(|line| line.strip_prefix("Review snapshot: "))
        .expect("missing review snapshot");
    let snapshot: serde_json::Value = serde_json::from_str(snapshot_line).unwrap();
    assert_eq!(snapshot["replay"]["cases"][0]["id"], "operator-case");
    assert_eq!(snapshot["observations"][0]["event"]["entity_id"], 42);
    assert!(snapshot["checkpoint"]["seq"].as_i64().unwrap() > 0);
    let checkpoint_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE seq=?1",
            [snapshot["checkpoint"]["seq"].as_i64().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(snapshot["checkpoint"]["hash"], checkpoint_hash);
    let replay_line = text
        .lines()
        .find_map(|line| line.strip_prefix("Replay case: "))
        .expect("missing replay evidence");
    let replay_case: serde_json::Value = serde_json::from_str(replay_line).unwrap();
    assert_eq!(replay_case["id"], "operator-case");
    assert_eq!(replay_case["expected"]["decision"], "Throttle");
    assert_eq!(replay_case["actual"]["decision"], "Throttle");
    assert_eq!(replay_case["passed"], true);
    let observation_line = text
        .lines()
        .find_map(|line| line.strip_prefix("Synthetic observation: "))
        .unwrap();
    let observation: serde_json::Value = serde_json::from_str(observation_line).unwrap();
    assert_eq!(observation["champion"]["decision"], "Allow");
    assert_eq!(observation["shadow"]["decision"], "Throttle");
    assert_eq!(observation["event"]["metric"], 3.0);
    assert!(text.find("Replay case: ").unwrap() < text.find("Enter approve or reject:").unwrap());
    drop(conn);
    let mismatch = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            mismatch_path.to_str().unwrap(),
            "--measured",
            "--fixture",
            fixture_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    assert!(!mismatch_path.exists());
    fixture["cases"][0]["expected"]["decision"] = 0.into();
    std::fs::write(&fixture_path, fixture.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            failed_path.to_str().unwrap(),
            "--fixture",
            fixture_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let failed_text = String::from_utf8_lossy(&output.stdout);
    let failed_case: serde_json::Value = serde_json::from_str(
        failed_text
            .lines()
            .find_map(|line| line.strip_prefix("Replay case: "))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(failed_case["expected"]["decision"], "Allow");
    assert_eq!(failed_case["actual"]["decision"], "Throttle");
    assert_eq!(failed_case["passed"], false);
    let conn = rusqlite::Connection::open(&failed_path).unwrap();
    let counts: (i64,i64,i64) = conn.query_row("SELECT (SELECT count(*) FROM event_replay_runs WHERE passed=0),(SELECT count(*) FROM event_champions),(SELECT count(*) FROM event_reviews)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts, (1, 0, 0));
    drop(conn);
    let inspect_failed = || {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args(["event-replay-inspect", failed_path.to_str().unwrap(), "1"])
            .output()
            .unwrap()
    };
    let output = inspect_failed();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replay: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(replay["passed"], false);
    assert_eq!(replay["fixture_version"], "operator-v1");
    assert_eq!(replay["cases"][0]["id"], "operator-case");
    assert_eq!(replay["cases"][0]["expected"]["decision"], 0);
    assert_eq!(replay["cases"][0]["actual"]["decision"], 1);
    let conn = rusqlite::Connection::open(&failed_path).unwrap();
    conn.execute("UPDATE event_replay_cases SET passed=1", [])
        .unwrap();
    drop(conn);
    assert!(!inspect_failed().status.success());
    std::fs::remove_file(db_path).unwrap();
    std::fs::remove_file(failed_path).unwrap();
    std::fs::remove_file(fixture_path).unwrap();
}

#[test]
fn typed_cycle_processes_synthetic_event_file_in_order() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let events = std::env::temp_dir().join(format!("sese-events-{suffix}.json"));
    let db = std::env::temp_dir().join(format!("sese-events-{suffix}.db"));
    std::fs::write(&events, r#"{"schema":"sese-synthetic-events-v1","events":[{"timestamp_ns":10,"entity_id":11,"metric":2.0,"flags":1},{"timestamp_ns":20,"entity_id":22,"metric":5.0,"flags":2}]}"#).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            db.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let conn = rusqlite::Connection::open(&db).unwrap();
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT event_json,prior_json FROM event_observations ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    let second_event: serde_json::Value = serde_json::from_str(&rows[1].0).unwrap();
    let prior: serde_json::Value = serde_json::from_str(&rows[1].1).unwrap();
    assert_eq!(second_event["entity_id"], 22);
    assert_eq!(prior["count"], 1);
    let reviewed: i64 = conn
        .query_row("SELECT observation_id FROM event_reviews", [], |r| r.get(0))
        .unwrap();
    let last: i64 = conn
        .query_row("SELECT max(id) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reviewed, last);
    drop(conn);
    let inspect = || {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-observations",
                db.to_str().unwrap(),
                "typed-cycle",
                "0",
            ])
            .output()
            .unwrap()
    };
    let output = inspect();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let page: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["observations"].as_array().unwrap().len(), 2);
    assert_eq!(page["observations"][0]["event"]["entity_id"], 11);
    assert_eq!(page["observations"][1]["event"]["entity_id"], 22);
    assert_eq!(page["observations"][1]["prior"]["count"], 1);
    assert_eq!(page["next_after_id"], serde_json::Value::Null);
    let first = page["observations"][0]["id"].as_i64().unwrap().to_string();
    let page_two = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-observations",
            db.to_str().unwrap(),
            "typed-cycle",
            &first,
        ])
        .output()
        .unwrap();
    assert!(page_two.status.success());
    let page_two: serde_json::Value = serde_json::from_slice(&page_two.stdout).unwrap();
    assert_eq!(page_two["observations"].as_array().unwrap().len(), 1);
    assert_eq!(page_two["observations"][0]["event"]["entity_id"], 22);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE event_observations SET event_json='{}' WHERE id=(SELECT min(id) FROM event_observations)", []).unwrap();
    drop(conn);
    assert!(!inspect().status.success());
    std::fs::remove_file(db).unwrap();
    std::fs::write(&events, r#"{"schema":"sese-synthetic-events-v1","events":[{"timestamp_ns":10,"entity_id":11,"metric":-2.0,"flags":1}]}"#).unwrap();
    let rejected_db = std::env::temp_dir().join(format!("sese-events-invalid-{suffix}.db"));
    let output = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            rejected_db.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!rejected_db.exists());
    std::fs::remove_file(events).unwrap();
}

#[test]
fn prepare_review_for_existing_domain_uses_latest_observation_without_writes() {
    let dir = std::env::temp_dir().join(format!(
        "sese-existing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("cycle.db");
    let initial = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_event-worker"))
        .arg(&db)
        .args(["--measured", "--prepare-only"])
        .output()
        .unwrap();
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&initial.stdout).unwrap();
    let initial_path = dir.join("initial.json");
    std::fs::write(&initial_path, &initial.stdout).unwrap();
    let outcome = env!("CARGO_BIN_EXE_sese-outcome");
    let reject = Command::new(outcome)
        .arg("event-review-prepared")
        .arg(&db)
        .arg(&initial_path)
        .args(["operator", "reject"])
        .output()
        .unwrap();
    assert!(
        reject.status.success(),
        "{}",
        String::from_utf8_lossy(&reject.stderr)
    );
    let observed = Command::new(outcome)
        .arg("event-builtin-observe")
        .arg(&db)
        .args([
            "typed-cycle",
            "iteration-2",
            "allow",
            "throttle",
            "measured-v2",
            "2",
            "42",
            "4",
            "0",
        ])
        .output()
        .unwrap();
    assert!(
        observed.status.success(),
        "{}",
        String::from_utf8_lossy(&observed.stderr)
    );
    let mut storage = sese_runtime_spike::storage::Storage::open_read_only(&db).unwrap();
    let before = storage.ledger_checkpoint().unwrap();
    drop(storage);
    let replay = first["replay_id"].as_i64().unwrap().to_string();
    let prepared = Command::new(outcome)
        .arg("event-review-prepare")
        .arg(&db)
        .args(["typed-cycle", &replay])
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let next: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_ne!(next["observation_id"], first["observation_id"]);
    assert_eq!(next["review_recorded"], false);
    let mut storage = sese_runtime_spike::storage::Storage::open_read_only(&db).unwrap();
    assert_eq!(storage.ledger_checkpoint().unwrap(), before);
    drop(storage);
    let path = dir.join("next.json");
    std::fs::write(&path, &prepared.stdout).unwrap();
    let approve = Command::new(outcome)
        .arg("event-review-prepared")
        .arg(&db)
        .arg(&path)
        .args(["operator", "approve"])
        .output()
        .unwrap();
    assert!(
        approve.status.success(),
        "{}",
        String::from_utf8_lossy(&approve.stderr)
    );
    let mut storage = sese_runtime_spike::storage::Storage::open_read_only(&db).unwrap();
    assert_eq!(
        storage.current_event_champion("typed-cycle").unwrap(),
        next["candidate_id"].as_str().unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn existing_domain_review_remains_available_after_one_hundred_observations() {
    let dir = std::env::temp_dir().join(format!(
        "sese-long-domain-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db = dir.join("cycle.db");
    let events = dir.join("events.json");
    let data = serde_json::json!({"schema":"sese-synthetic-events-v1", "events": (0..100).map(|i| serde_json::json!({"timestamp_ns":i+1,"entity_id":42,"metric":4.0,"flags":0})).collect::<Vec<_>>()});
    std::fs::write(&events, data.to_string()).unwrap();
    let initial = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .arg(env!("CARGO_BIN_EXE_event-worker"))
        .arg(&db)
        .args(["--measured", "--prepare-only", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&initial.stdout).unwrap();
    let outcome = env!("CARGO_BIN_EXE_sese-outcome");
    let observed = Command::new(outcome)
        .arg("event-builtin-observe")
        .arg(&db)
        .args([
            "typed-cycle",
            "iteration-101",
            "allow",
            "throttle",
            "measured-v2",
            "101",
            "42",
            "4",
            "0",
        ])
        .output()
        .unwrap();
    assert!(
        observed.status.success(),
        "{}",
        String::from_utf8_lossy(&observed.stderr)
    );
    let replay = first["replay_id"].as_i64().unwrap().to_string();
    let prepared = Command::new(outcome)
        .arg("event-review-prepare")
        .arg(&db)
        .args(["typed-cycle", &replay])
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let next: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(
        next["preview"]["observations"].as_array().unwrap().len(),
        100
    );
    assert_eq!(next["preview"]["observations_truncated"], true);
    let oldest = first["preview"]["observations"][0]["id"]
        .as_i64()
        .unwrap()
        .to_string();
    let seq = next["checkpoint"]["seq"].as_i64().unwrap().to_string();
    let hash = next["checkpoint"]["hash"].as_str().unwrap();
    let excluded = Command::new(outcome)
        .arg("event-review-at")
        .arg(&db)
        .args([
            "typed-cycle",
            next["candidate_id"].as_str().unwrap(),
            &replay,
            &oldest,
            "operator",
            "reject",
            "h",
            "b",
            "p",
            &seq,
            hash,
        ])
        .output()
        .unwrap();
    assert!(!excluded.status.success());
    assert!(
        String::from_utf8_lossy(&excluded.stderr).contains("not in the specified preview domain")
    );
    let path = dir.join("next.json");
    std::fs::write(&path, &prepared.stdout).unwrap();
    let reject = Command::new(outcome)
        .arg("event-review-prepared")
        .arg(&db)
        .arg(&path)
        .args(["operator", "reject"])
        .output()
        .unwrap();
    assert!(
        reject.status.success(),
        "{}",
        String::from_utf8_lossy(&reject.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
