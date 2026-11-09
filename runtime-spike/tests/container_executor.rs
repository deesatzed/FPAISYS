#![cfg(unix)]
use sese_runtime_spike::{
    event_abi::{Event, Window},
    event_executor::SyntheticContainerExecutor,
};
use std::time::Duration;

fn request() -> (Event, Window) {
    (
        Event {
            timestamp_ns: 10,
            entity_id: 42,
            metric: 3.5,
            flags: 7,
        },
        Window {
            count: 1,
            mean: 2.0,
            p99_us: 3,
            last_error: 4,
        },
    )
}

#[test]
fn rejects_non_builtin_artifacts_before_backend() {
    let journal = std::env::temp_dir().join(format!(
        "sese-container-executor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&journal).unwrap();
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        bridge,
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        journal.to_str().unwrap(),
    )
    .unwrap();
    let (event, window) = request();
    assert!(executor
        .execute(b"not a builtin", &window, &event, Duration::from_secs(2))
        .is_err());
    assert_eq!(std::fs::read_dir(&journal).unwrap().count(), 0);
    std::fs::remove_dir(journal).unwrap();
}

#[test]
fn invalid_bridge_verdict_keeps_receipt_path_in_error() {
    let dir = std::env::temp_dir().join(format!(
        "sese-container-bad-verdict-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let bridge = dir.join("bridge.py");
    std::fs::write(&bridge, "import struct,sys\nsys.stdin.buffer.read()\nsys.stdout.buffer.write(struct.pack('<IdI',4,0.0,0))\n").unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/synthetic_container_lifecycle.py"
        ),
        dir.join("synthetic_container_lifecycle.py"),
    )
    .unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/python3",
        bridge.to_str().unwrap(),
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        dir.to_str().unwrap(),
    )
    .unwrap();
    let (event, window) = request();
    let artifact = sese_runtime_spike::builtin_event::artifact("throttle").unwrap();
    let error = match executor.execute(&artifact, &window, &event, Duration::from_secs(5)) {
        Ok(_) => panic!("invalid verdict accepted"),
        Err(e) => e,
    };
    assert!(error.contains("receipt_path="), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn executes_real_synthetic_container_and_retains_receipt() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let journal = std::env::temp_dir().join(format!(
        "sese-container-executor-live-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&journal).unwrap();
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        bridge,
        "/usr/local/bin/docker",
        &image,
        &host,
        journal.to_str().unwrap(),
    )
    .unwrap();
    let artifact = sese_runtime_spike::builtin_event::artifact("throttle").unwrap();
    let (event, window) = request();
    let result = executor
        .execute(&artifact, &window, &event, Duration::from_secs(40))
        .unwrap();
    assert_eq!(result.verdict.score, 3.5);
    assert!(result.receipt_path.is_file());
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&result.receipt_path).unwrap()).unwrap();
    assert_eq!(receipt["state"], "removed");
    assert_eq!(receipt["candidate"], "throttle");
    std::fs::remove_file(result.receipt_path).unwrap();
    std::fs::remove_dir(journal).unwrap();
}

#[test]
fn real_container_window_probe_consumes_distinct_prior_windows() {
    use sese_runtime_spike::event_abi::Decision;
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let journal = std::env::temp_dir().join(format!(
        "sese-window-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
    let artifact = sese_runtime_spike::builtin_event::artifact("window-probe").unwrap();
    let (event, window) = request();
    let first = executor
        .execute(&artifact, &window, &event, Duration::from_secs(40))
        .unwrap();
    assert_eq!(first.verdict.decision, Decision::Allow);
    assert_eq!(first.verdict.score, 6.0);
    assert_eq!(first.verdict.reason, 4);
    let different = Window {
        count: 4,
        mean: 2.0,
        p99_us: 3,
        last_error: 8,
    };
    let second = executor
        .execute(&artifact, &different, &event, Duration::from_secs(40))
        .unwrap();
    assert_eq!(second.verdict.score, 9.0);
    assert_eq!(second.verdict.reason, 8);
    assert_ne!(first.receipt_path, second.receipt_path);
    std::fs::remove_dir_all(journal).unwrap();
}

#[test]
fn container_replay_is_stored_separately_from_promotion_evidence() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let directory = std::env::temp_dir().join(format!(
        "sese-container-app-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("app.db");
    let journal = directory.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        bridge,
        "/usr/local/bin/docker",
        &image,
        &host,
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let artifact = sese_runtime_spike::builtin_event::artifact("throttle").unwrap();
    let candidate = db.register_synthetic_event(&artifact).unwrap();
    let pack = sese_runtime_spike::fixture_file::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-synthetic-v1.json"
    ))
    .unwrap();
    let result = sese_runtime_spike::event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &pack,
        Duration::from_secs(40),
    )
    .unwrap();
    assert!(result.passed);
    let wrong = db
        .register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("allow").unwrap())
        .unwrap();
    let wrong_result = sese_runtime_spike::event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &wrong,
        &pack,
        Duration::from_secs(40),
    )
    .unwrap();
    assert!(!wrong_result.passed);
    assert_eq!(
        db.inspect_container_replay_experiment(wrong_result.seq)
            .unwrap()["cases"][0]["actual"]["decision"],
        0
    );
    // A valid wrong verdict must not prevent later cases from running.
    let mut multi: serde_json::Value = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let mut second = multi["cases"][0].clone();
    second["id"] = serde_json::json!("second");
    multi["cases"][0]["expected"]["decision"] = serde_json::json!(0);
    multi["cases"].as_array_mut().unwrap().push(second);
    let multi = sese_runtime_spike::fixture_file::parse(multi.to_string().as_bytes()).unwrap();
    let replay = sese_runtime_spike::event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &multi,
        Duration::from_secs(40),
    )
    .unwrap();
    assert!(!replay.passed);
    let observed = db.inspect_container_replay_experiment(replay.seq).unwrap();
    assert_eq!(observed["cases"][0]["passed"], false);
    assert_eq!(observed["cases"][1]["passed"], true);
    assert!(observed["cases"][1]["receipt"].is_string());
    drop(db);
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let stored = db.inspect_container_replay_experiment(result.seq).unwrap();
    assert_eq!(stored["promotion_evidence"], false);
    assert_eq!(stored["cases"][0]["actual"]["decision"], 1);
    assert!(stored["cases"][0]["receipt"]
        .as_str()
        .unwrap()
        .contains("valid_verdict"));
    assert!(db
        .pin_event_fixture("container", result.seq, "operator")
        .is_err());
    let checkpoint = db.ledger_checkpoint().unwrap();
    let verifier = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py");
    let checked = std::process::Command::new("/usr/bin/python3")
        .args([
            verifier,
            path.to_str().unwrap(),
            "--seq",
            &checkpoint.0.to_string(),
            "--hash",
            &checkpoint.1,
            "--verify-synthetic-candidates",
        ])
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let verification: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(verification["checkpoint_match"], true);
    assert_eq!(verification["evidence_bindings_checked"], false);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_container_bridge_records_failure_and_cannot_pin_fixture() {
    let journal = std::env::temp_dir().join(format!(
        "sese-container-app-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&journal).unwrap();
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        bridge,
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = sese_runtime_spike::storage::Storage::from_connection(
        rusqlite::Connection::open_in_memory().unwrap(),
    )
    .unwrap();
    let candidate = db
        .register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let mut fixture: serde_json::Value = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let mut second = fixture["cases"][0].clone();
    second["id"] = serde_json::json!("second");
    fixture["cases"].as_array_mut().unwrap().push(second);
    let pack = sese_runtime_spike::fixture_file::parse(fixture.to_string().as_bytes()).unwrap();
    let result = sese_runtime_spike::event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &pack,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(!result.passed);
    let evidence = db.inspect_container_replay_experiment(result.seq).unwrap();
    assert_eq!(evidence["cases"][0]["passed"], false);
    assert!(evidence["cases"][0]["error"]
        .as_str()
        .unwrap()
        .contains("receipt_path="));
    assert_eq!(
        evidence["cases"][1]["error"],
        "not run after earlier worker failure"
    );
    assert!(evidence["cases"][1]["started_unix_ns"].is_null());
    assert!(db
        .pin_event_fixture("domain", result.seq, "operator")
        .is_err());
    std::fs::remove_dir(journal).unwrap();
}

#[test]
fn inspection_rejects_inconsistent_container_results_even_with_rehashed_chain() {
    use sha2::{Digest, Sha256};
    let dir = std::env::temp_dir().join(format!(
        "sese-container-invalid-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let db_path = dir.join("app.db");
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let executor = SyntheticContainerExecutor::new(
        "/usr/bin/true",
        bridge,
        "/usr/bin/true",
        &format!("sha256:{}", "0".repeat(64)),
        "unix:///tmp/nonexistent.sock",
        journal.to_str().unwrap(),
    )
    .unwrap();
    let mut db = sese_runtime_spike::storage::Storage::open(&db_path).unwrap();
    let candidate = db
        .register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let pack = sese_runtime_spike::fixture_file::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-synthetic-v1.json"
    ))
    .unwrap();
    let result = sese_runtime_spike::event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &pack,
        Duration::from_secs(2),
    )
    .unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [result.seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut data: serde_json::Value = serde_json::from_str(&payload).unwrap();
    data["cases"][0]["passed"] = serde_json::json!(true);
    let payload = data.to_string();
    let fields = [
        "sese-ledger-v1",
        prev.as_str(),
        "container_replay_experiment",
        "0",
        payload.as_str(),
        time.as_str(),
    ];
    let encoded = fields
        .iter()
        .map(|s| format!("{}:{}", s.len(), s))
        .collect::<String>();
    let digest = format!("{:x}", Sha256::digest(encoded.as_bytes()));
    conn.execute(
        "UPDATE ledger_events SET payload=?1,hash=?2 WHERE seq=?3",
        rusqlite::params![payload, digest, result.seq],
    )
    .unwrap();
    drop(conn);
    let mut db = sese_runtime_spike::storage::Storage::open(&db_path).unwrap();
    assert!(db.inspect_container_replay_experiment(result.seq).is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn inspection_rejects_rehashed_replay_with_tampered_success_receipt() {
    use sha2::{Digest, Sha256};
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-replay-receipt-tamper-{}-{}",
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
    let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let candidate = db
        .register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let mut fixture: serde_json::Value = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let mut second = fixture["cases"][0].clone();
    second["id"] = serde_json::json!("second-case");
    fixture["cases"].as_array_mut().unwrap().push(second);
    let pack =
        sese_runtime_spike::fixture_file::parse(&serde_json::to_vec(&fixture).unwrap()).unwrap();
    let report = sese_runtime_spike::event_replay::replay_container_synthetic(
        &mut db,
        &executor,
        &candidate,
        &pack,
        Duration::from_secs(40),
    )
    .unwrap();
    assert!(report.passed);
    assert!(db.inspect_container_replay_experiment(report.seq).is_ok());
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (payload, time, prev): (String, String, String) = conn
        .query_row(
            "SELECT payload,created_ns,prev FROM ledger_events WHERE seq=?1",
            [report.seq],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let base: serde_json::Value = serde_json::from_str(&payload).unwrap();
    for variant in ["request", "output", "reuse", "daemon", "extra"] {
        let mut value = base.clone();
        let mut receipt: serde_json::Value =
            serde_json::from_str(value["cases"][1]["receipt"].as_str().unwrap()).unwrap();
        match variant {
            "request" => receipt["request_sha256"] = serde_json::json!("0".repeat(64)),
            "output" => receipt["output_sha256"] = serde_json::json!("0".repeat(64)),
            "reuse" => {
                value["cases"][1]["receipt"] = base["cases"][0]["receipt"].clone();
                value["cases"][1]["receipt_path"] = base["cases"][0]["receipt_path"].clone();
            }
            "daemon" => receipt["daemon_id"] = serde_json::json!("different-daemon"),
            "extra" => receipt["extra"] = serde_json::json!(true),
            _ => unreachable!(),
        }
        if variant != "reuse" {
            value["cases"][1]["receipt"] = serde_json::json!(receipt.to_string());
        }
        let payload = value.to_string();
        let encoded = [
            "sese-ledger-v1",
            prev.as_str(),
            "container_replay_experiment",
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
            rusqlite::params![payload, digest, report.seq],
        )
        .unwrap();
        let mut db = sese_runtime_spike::storage::Storage::open(&path).unwrap();
        assert!(
            db.inspect_container_replay_experiment(report.seq).is_err(),
            "accepted {variant}"
        );
    }
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn operator_can_execute_and_inspect_container_replay() {
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-container-cli-{}-{}",
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
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let call = |args: &[&str]| std::process::Command::new(cli).args(args).output().unwrap();
    let dbstr = db.to_str().unwrap();
    assert!(call(&["event-init", dbstr]).status.success());
    assert!(call(&["event-builtin-register", dbstr, "throttle"])
        .status
        .success());
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-synthetic-v1.json"
    );
    let output = call(&[
        "event-container-replay",
        dbstr,
        "throttle",
        fixture,
        &image,
        &host,
        journal.to_str().unwrap(),
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/docker",
        bridge,
        "40",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["passed"], true);
    assert_eq!(value["promotion_evidence"], false);
    let seq = value["container_event_seq"].as_i64().unwrap().to_string();
    let inspected = call(&["event-container-replay-inspect", dbstr, &seq]);
    assert!(inspected.status.success());
    let evidence: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(evidence["cases"][0]["id"], "throttle-example");
    assert!(call(&["event-builtin-register", dbstr, "allow"])
        .status
        .success());
    let wrong = call(&[
        "event-container-replay",
        dbstr,
        "allow",
        fixture,
        &image,
        &host,
        journal.to_str().unwrap(),
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/docker",
        bridge,
        "40",
    ]);
    assert!(!wrong.status.success());
    let failed: serde_json::Value = serde_json::from_slice(&wrong.stdout).unwrap();
    assert_eq!(failed["passed"], false);
    let seq = failed["container_event_seq"].as_i64().unwrap().to_string();
    let inspected = call(&["event-container-replay-inspect", dbstr, &seq]);
    let evidence: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(evidence["cases"][0]["actual"]["decision"], 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn timed_out_container_replay_retains_cleanup_identity() {
    let image = match std::env::var("SESE_TEST_TIMEOUT_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let directory = std::env::temp_dir().join(format!(
        "sese-container-timeout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    // The sleep image is a deliberately non-worker fault injector. Exercise the
    // lifecycle deadline with a test-only bridge, not the application profile.
    let helper_dir = directory.with_extension("helpers");
    std::fs::create_dir(&helper_dir).unwrap();
    let scripts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts");
    let source = std::fs::read_to_string(scripts.join("synthetic_container_worker.py")).unwrap();
    assert!(source.contains("require_worker_profile=True"));
    let bridge = helper_dir.join("synthetic_container_worker.py");
    std::fs::write(
        &bridge,
        source.replace(
            "require_worker_profile=True",
            "require_worker_profile=False",
        ),
    )
    .unwrap();
    std::fs::copy(
        scripts.join("synthetic_container_lifecycle.py"),
        helper_dir.join("synthetic_container_lifecycle.py"),
    )
    .unwrap();
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        bridge.to_str().unwrap(),
        "/usr/local/bin/docker",
        &image,
        &host,
        directory.to_str().unwrap(),
    )
    .unwrap();
    let (event, window) = request();
    let artifact = sese_runtime_spike::builtin_event::artifact("throttle").unwrap();
    let started = std::time::Instant::now();
    let error = match executor.execute(&artifact, &window, &event, Duration::from_secs(5)) {
        Ok(_) => panic!("sleep worker returned a verdict"),
        Err(error) => error,
    };
    assert!(started.elapsed() >= Duration::from_secs(4));
    assert!(started.elapsed() < Duration::from_secs(30));
    let receipt = std::path::PathBuf::from(
        error
            .split("receipt_path=")
            .nth(1)
            .expect("missing receipt path"),
    );
    assert_eq!(receipt.parent().unwrap(), directory.canonicalize().unwrap());
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    let name = record["name"].as_str().unwrap();
    let inspected = std::process::Command::new("/usr/local/bin/docker")
        .args([
            "--host",
            &host,
            "inspect",
            "--format",
            "{{.Id}}|{{index .Config.Labels \"io.sese.synthetic-probe\"}}|{{.Image}}",
            name,
        ])
        .output()
        .unwrap();
    let container_remained = inspected.status.success();
    if container_remained {
        let text = String::from_utf8(inspected.stdout).unwrap();
        let fields: Vec<_> = text.trim().split('|').collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[1], name);
        assert_eq!(fields[2], image);
        let removed = std::process::Command::new("/usr/local/bin/docker")
            .args(["--host", &host, "rm", "-f", fields[0]])
            .output()
            .unwrap();
        assert!(
            removed.status.success(),
            "{}",
            String::from_utf8_lossy(&removed.stderr)
        );
        let remaining = std::process::Command::new("/usr/local/bin/docker")
            .args([
                "--host",
                &host,
                "ps",
                "-aq",
                "--no-trunc",
                "--filter",
                &format!("id={}", fields[0]),
            ])
            .output()
            .unwrap();
        assert!(remaining.status.success());
        assert!(remaining.stdout.is_empty());
    }
    assert!(error.contains("worker failed"), "{error}");
    assert!(!container_remained, "timeout left a test container behind");
    assert_eq!(record["state"], "removed");
    assert_eq!(record["result_status"], "failed");
    std::fs::remove_dir_all(directory).unwrap();
    std::fs::remove_dir_all(helper_dir).unwrap();
}

#[test]
fn container_replay_rejects_a_changed_ledger_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let image = match std::env::var("SESE_TEST_IMAGE") {
        Ok(v) => v,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sese-container-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let db_path = dir.join("app.db");
    let journal = dir.join("journal");
    std::fs::create_dir(&journal).unwrap();
    let marker = dir.join("started");
    let release = dir.join("release");
    let wrapper = dir.join("docker-wrapper");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    let script = format!("#!/bin/sh\nif [ \"$3\" = start ]; then : > {}; while [ ! -f {} ]; do /bin/sleep 0.05; done; fi\nexec /usr/local/bin/docker \"$@\"\n", quote(&marker), quote(&release));
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut db = sese_runtime_spike::storage::Storage::open(&db_path).unwrap();
    let candidate = db
        .register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("throttle").unwrap())
        .unwrap();
    let bridge = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/synthetic_container_worker.py"
    );
    let executor = SyntheticContainerExecutor::new(
        "/opt/homebrew/bin/python3",
        bridge,
        wrapper.to_str().unwrap(),
        &image,
        &host,
        journal.to_str().unwrap(),
    )
    .unwrap();
    let worker_db = db_path.clone();
    let task = std::thread::spawn(move || {
        let mut db = sese_runtime_spike::storage::Storage::open(&worker_db).unwrap();
        let pack = sese_runtime_spike::fixture_file::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ))
        .unwrap();
        sese_runtime_spike::event_replay::replay_container_synthetic(
            &mut db,
            &executor,
            &candidate,
            &pack,
            Duration::from_secs(40),
        )
        .err()
    });
    let start = std::time::Instant::now();
    while !marker.exists() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = marker.exists();
    let updated = if started {
        db.register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("allow").unwrap())
            .is_ok()
    } else {
        false
    };
    std::fs::write(&release, b"release").unwrap();
    let error = task.join().unwrap();
    assert!(started && updated);
    assert!(error
        .unwrap_or_default()
        .contains("ledger changed during container replay"));
    drop(db);
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM ledger_events WHERE kind='container_replay_experiment'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}
