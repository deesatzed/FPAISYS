use std::process::Command;

#[test]
fn measured_operator_stream_preserves_state_and_rejects_wrong_semantics() {
    let id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(format!("sese-measured-{id}.db"));
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-measured-synthetic-v1.json"
    );
    let db = path.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args(args)
            .output()
            .unwrap()
    };
    let success = |args: &[&str]| {
        let o = run(args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        o.stdout
    };
    success(&["event-init", db]);
    let allow: serde_json::Value =
        serde_json::from_slice(&success(&["event-builtin-register", db, "allow"])).unwrap();
    success(&["event-builtin-register", db, "throttle"]);
    let replay: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-replay",
        db,
        "throttle",
        fixture_path,
    ]))
    .unwrap();
    let bare = replay["run_id"].as_i64().unwrap().to_string();
    let ambiguous = run(&["event-pin", db, "domain", &bare, "operator"]);
    assert!(!ambiguous.status.success(), "bare replay IDs must be rejected");
    success(&[
        "event-pin",
        db,
        "domain",
        &format!("replay:{}", replay["run_id"].as_i64().unwrap()),
        "operator",
    ]);
    success(&[
        "event-bootstrap",
        db,
        "domain",
        allow["candidate_id"].as_str().unwrap(),
        "operator",
    ]);
    let wrong = run(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "allow",
        "throttle",
        "legacy-v1",
        "1",
        "42",
        "3",
        "4",
    ]);
    assert!(!wrong.status.success());
    assert!(wrong.stdout.is_empty());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let streams: i64 = conn
        .query_row(
            "SELECT (SELECT count(*) FROM event_streams)+(SELECT count(*) FROM measured_streams)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(streams, 0);
    drop(conn);
    let first: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "allow",
        "throttle",
        "measured-v2",
        "1",
        "42",
        "-4",
        "4",
    ]))
    .unwrap();
    assert_eq!(first["prior"]["count"], 0);
    let second: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "allow",
        "throttle",
        "measured-v2",
        "2",
        "43",
        "5",
        "0",
    ]))
    .unwrap();
    assert_eq!(second["prior"]["count"], 1);
    assert_eq!(second["prior"]["mean"], -4.0);
    let invalid = run(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "allow",
        "throttle",
        "measured-v2",
        "3",
        "44",
        "NaN",
        "0",
    ]);
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn operator_observes_and_reviews_a_durable_synthetic_stream() {
    let path = std::env::temp_dir().join(format!(
        "sese-observe-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = path.to_str().unwrap();
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let run = |args: &[&str]| Command::new(cli).args(args).output().unwrap();
    let success = |args: &[&str]| {
        let o = run(args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        o.stdout
    };
    success(&["event-init", db]);
    let allow: serde_json::Value =
        serde_json::from_slice(&success(&["event-builtin-register", db, "allow"])).unwrap();
    let shadow: serde_json::Value =
        serde_json::from_slice(&success(&["event-builtin-register", db, "throttle"])).unwrap();
    let replay: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-replay",
        db,
        "throttle",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ),
    ]))
    .unwrap();
    let run_id = replay["run_id"].as_i64().unwrap().to_string();
    success(&["event-pin", db, "domain", &format!("replay:{run_id}"), "operator"]);
    success(&[
        "event-bootstrap",
        db,
        "domain",
        allow["candidate_id"].as_str().unwrap(),
        "operator",
    ]);
    {
        let mut storage = sese_runtime_spike::storage::Storage::open(&path).unwrap();
        let mut host =
            sese_runtime_spike::event_host::EventHost::open(&mut storage, "guard-test", 32)
                .unwrap();
        let event = sese_runtime_spike::event_abi::Event {
            timestamp_ns: 1,
            entity_id: 42,
            metric: 3.0,
            flags: 4,
        };
        let error = host
            .process_domain_expected(
                &mut storage,
                "/nonexistent-worker",
                "domain",
                shadow["candidate_id"].as_str().unwrap(),
                shadow["candidate_id"].as_str().unwrap(),
                &event,
                0,
                std::time::Duration::from_secs(1),
            )
            .unwrap_err();
        assert_eq!(error, "unexpected domain champion");
    }
    let wrong = run(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "throttle",
        "throttle",
        "legacy-v1",
        "1",
        "42",
        "3",
        "4",
    ]);
    assert!(!wrong.status.success());
    assert!(wrong.stdout.is_empty());
    let first: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "allow",
        "throttle",
        "legacy-v1",
        "1",
        "42",
        "3",
        "4",
    ]))
    .unwrap();
    assert_eq!(first["prior"]["count"], 0);
    assert_eq!(first["champion"]["decision"], "allow");
    assert_eq!(first["shadow"]["decision"], "throttle");
    let second: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "allow",
        "throttle",
        "legacy-v1",
        "2",
        "43",
        "5",
        "1",
    ]))
    .unwrap();
    assert_eq!(second["prior"]["count"], 1);
    let preview: serde_json::Value =
        serde_json::from_slice(&success(&["event-review-preview", db, "domain", &run_id])).unwrap();
    let obs = second["observation_id"].as_i64().unwrap().to_string();
    let seq = preview["checkpoint"]["seq"].as_i64().unwrap().to_string();
    let review: serde_json::Value = serde_json::from_slice(&success(&[
        "event-review-at",
        db,
        "domain",
        shadow["candidate_id"].as_str().unwrap(),
        &run_id,
        &obs,
        "operator",
        "approve",
        "human",
        "bridge",
        "proposal",
        &seq,
        preview["checkpoint"]["hash"].as_str().unwrap(),
    ]))
    .unwrap();
    success(&[
        "event-promote",
        db,
        "domain",
        allow["candidate_id"].as_str().unwrap(),
        &review["review_id"].as_i64().unwrap().to_string(),
        "operator",
    ]);
    let promoted: serde_json::Value = serde_json::from_slice(&success(&[
        "event-builtin-observe",
        db,
        "domain",
        "stream",
        "throttle",
        "allow",
        "legacy-v1",
        "3",
        "44",
        "7",
        "0",
    ]))
    .unwrap();
    assert_eq!(promoted["prior"]["count"], 2);
    assert_eq!(promoted["champion"]["decision"], "throttle");
    success(&[
        "event-rollback",
        db,
        "domain",
        shadow["candidate_id"].as_str().unwrap(),
        "operator",
    ]);
    let review_id = review["review_id"].as_i64().unwrap().to_string();
    assert_eq!(
        String::from_utf8(success(&["event-status", db, &review_id]))
            .unwrap()
            .trim(),
        "pending"
    );
    success(&[
        "event-resolve",
        db,
        &review_id,
        "validated",
        "operator",
        "synthetic assertion",
    ]);
    let exported: serde_json::Value =
        serde_json::from_slice(&success(&["event-export", db])).unwrap();
    assert_eq!(exported["candidate_id"], shadow["candidate_id"]);
    assert_eq!(exported["outcome_evidence"], "caller_asserted");
    assert_eq!(exported["training_ready"], false);
    let history: serde_json::Value =
        serde_json::from_slice(&success(&["event-history", db, "domain", "0"])).unwrap();
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["detail"]["action"] == "rollback"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn operator_can_register_and_replay_fixed_candidates_across_processes() {
    let path = std::env::temp_dir().join(format!(
        "sese-operator-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let unclassified_path = path.with_extension("unclassified.db");
    let cli = env!("CARGO_BIN_EXE_sese-outcome");
    let run = |args: &[&str]| Command::new(cli).args(args).output().unwrap();
    let db = path.to_str().unwrap();
    let init = run(&["event-init", db]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(!run(&["event-init", db]).status.success());
    let missing = run(&[
        "event-builtin-replay",
        db,
        "throttle",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ),
    ]);
    assert!(!missing.status.success());
    let registered = run(&["event-builtin-register", db, "throttle"]);
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let candidate: serde_json::Value = serde_json::from_slice(&registered.stdout).unwrap();
    assert_eq!(candidate["synthetic"], true);
    assert_eq!(candidate["executed"], false);
    let replay = run(&[
        "event-builtin-replay",
        db,
        "throttle",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ),
    ]);
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay: serde_json::Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["candidate_id"], candidate["candidate_id"]);
    assert_eq!(replay["passed"], true);
    let inspect = run(&[
        "event-replay-inspect",
        db,
        &replay["run_id"].as_i64().unwrap().to_string(),
    ]);
    assert!(inspect.status.success());
    let inspected: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(inspected["cases"][0]["passed"], true);
    let quarantine = run(&["event-builtin-register", db, "quarantine"]);
    assert!(quarantine.status.success());
    let mismatch = run(&[
        "event-builtin-replay",
        db,
        "quarantine",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ),
    ]);
    assert!(!mismatch.status.success());
    let mismatch: serde_json::Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    assert_eq!(mismatch["passed"], false);
    let failed_run = mismatch["run_id"].as_i64().unwrap().to_string();
    let rejected_pin = run(&["event-pin", db, "failed-domain", &format!("replay:{failed_run}"), "operator"]);
    assert!(!rejected_pin.status.success());
    let allow = run(&["event-builtin-register", db, "allow"]);
    assert!(allow.status.success());
    let allow: serde_json::Value = serde_json::from_slice(&allow.stdout).unwrap();
    let rejected_bootstrap = run(&[
        "event-bootstrap",
        db,
        "failed-domain",
        allow["candidate_id"].as_str().unwrap(),
        "operator",
    ]);
    assert!(!rejected_bootstrap.status.success());
    assert!(!run(&["event-builtin-register", db, "unknown"])
        .status
        .success());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (replays, champions): (i64, i64) = conn.query_row("SELECT (SELECT count(*) FROM event_replay_runs),(SELECT count(*) FROM event_champions)", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((replays, champions), (2, 0));
    let failed_pins: i64 = conn
        .query_row(
            "SELECT count(*) FROM event_fixture_pins WHERE domain='failed-domain'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(failed_pins, 0);
    // Reconstruct a previously permitted failed-replay pin with its valid legacy ledger binding.
    let (version, digest, replay_hash): (String, String, String) = conn.query_row("SELECT r.pack_version,r.pack_digest,e.hash FROM event_replay_runs r JOIN ledger_events e ON e.kind='event_replay' AND e.reference=r.id WHERE r.id=?1", [mismatch["run_id"].as_i64().unwrap()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    conn.execute("INSERT INTO event_fixture_pins(domain,replay_id,pack_version,pack_digest,actor,created_unix_ns) VALUES ('legacy-failed',?1,?2,?3,'operator',?4)", rusqlite::params![mismatch["run_id"].as_i64().unwrap(),version,digest,now]).unwrap();
    let pin_id = conn.last_insert_rowid();
    let encode = |fields: &[&str]| {
        fields
            .iter()
            .map(|s| format!("{}:{s}", s.len()))
            .collect::<String>()
    };
    let payload = encode(&[
        "synthetic-event-fixture-pin-v1",
        "legacy-failed",
        &failed_run,
        &version,
        &digest,
        "operator",
        &replay_hash,
    ]);
    let (seq, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    use sha2::{Digest, Sha256};
    let hash = format!(
        "{:x}",
        Sha256::digest(
            encode(&[
                "sese-ledger-v1",
                &prev,
                "event_fixture_pin",
                &pin_id.to_string(),
                &payload,
                &now
            ])
            .as_bytes()
        )
    );
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_fixture_pin',?2,?3,?4,?5,?6)",
        rusqlite::params![seq + 1, pin_id, payload, now, prev, hash],
    )
    .unwrap();
    drop(conn);
    let mut legacy = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    assert!(legacy.verify_ledger().unwrap());
    assert!(legacy
        .bootstrap_event_champion(
            "legacy-failed",
            allow["candidate_id"].as_str().unwrap(),
            "operator"
        )
        .is_err());
    drop(legacy);
    let mut storage = sese_runtime_spike::storage::Storage::open(&path).unwrap();
    let other = wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 i32.const 99))").unwrap();
    let other_id = storage.register_synthetic_event(&other).unwrap();
    storage
        .pin_event_fixture(
            "other-domain",
            replay["run_id"].as_i64().unwrap(),
            "operator",
        )
        .unwrap();
    storage
        .bootstrap_event_champion("other-domain", &other_id, "operator")
        .unwrap();
    drop(storage);
    let other = run(&[
        "event-builtin-observe",
        db,
        "other-domain",
        "other-stream",
        "allow",
        "throttle",
        "legacy-v1",
        "1",
        "42",
        "3",
        "4",
    ]);
    assert!(!other.status.success());
    assert!(other.stdout.is_empty());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM event_observations WHERE domain='other-domain'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    let unclassified = sese_runtime_spike::storage::Storage::open(&unclassified_path).unwrap();
    unclassified
        .register_event(&sese_runtime_spike::builtin_event::artifact("throttle").unwrap())
        .unwrap();
    drop(unclassified);
    let unclassified_run = run(&[
        "event-builtin-replay",
        unclassified_path.to_str().unwrap(),
        "throttle",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ),
    ]);
    assert!(!unclassified_run.status.success());
    assert!(unclassified_run.stdout.is_empty());
    let conn = rusqlite::Connection::open(&unclassified_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_replay_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(unclassified_path).unwrap();
}
