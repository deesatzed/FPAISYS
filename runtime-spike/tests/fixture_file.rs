use sese_runtime_spike::{fixture_file, storage::Storage};
use std::time::Duration;

const FIXTURE: &str = r#"{
  "schema":"sese-event-fixture-file-v1", "version":"synthetic-v1", "state_semantics":"legacy-v1",
  "cases":[{"id":"one", "window":{"count":0,"mean":0.0,"p99_us":0,"last_error":0},
  "event":{"timestamp_ns":1,"entity_id":2,"metric":3.0,"flags":4},
  "expected":{"decision":0,"score":3.0,"reason":4}}]
}"#;

#[test]
fn fixture_file_identity_matches_real_synthetic_replay() {
    let pack = fixture_file::parse(FIXTURE.as_bytes()).unwrap();
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let wasm=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))").unwrap();
    let candidate = db.register_synthetic_event(&wasm).unwrap();
    let result = pack
        .replay_synthetic(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(result.passed);
    let evidence = db.verify_event_replay(result.run_id).unwrap();
    assert_eq!(evidence.pack_digest, pack.digest);
    assert_eq!(evidence.pack_version, pack.version);
    let measured =
        fixture_file::parse(FIXTURE.replace("legacy-v1", "measured-v2").as_bytes()).unwrap();
    let result = measured
        .replay_synthetic(
            &mut db,
            env!("CARGO_BIN_EXE_event-worker"),
            &candidate,
            Duration::from_secs(2),
        )
        .unwrap();
    let evidence = db.verify_event_replay(result.run_id).unwrap();
    assert_eq!(evidence.pack_digest, measured.digest);
    assert_eq!(evidence.state_semantics, "measured-v2");
}

#[test]
fn rejects_ambiguous_invalid_and_oversized_fixture_files() {
    for malformed in [
        FIXTURE.replace(
            "\"version\":\"synthetic-v1\"",
            "\"version\":\"v1\",\"version\":\"v2\"",
        ),
        FIXTURE.replace("\"decision\":0", "\"decision\":4"),
        FIXTURE.replace("\"legacy-v1\"", "\"unknown\""),
        FIXTURE.replace("\"mean\":0.0", "\"mean\":1e400"),
        FIXTURE.replace("\"id\":\"one\"", "\"id\":\"one\",\"extra\":1"),
        FIXTURE.replace("\"cases\":[{", "\"cases\":[] , \"ignored\":[{"),
    ] {
        assert!(fixture_file::parse(malformed.as_bytes()).is_err());
    }
    assert!(fixture_file::parse(&vec![b' '; fixture_file::MAX_FIXTURE_BYTES + 1]).is_err());
    let mut empty: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    empty["cases"] = serde_json::json!([]);
    assert!(fixture_file::parse(empty.to_string().as_bytes()).is_err());
    let legacy = fixture_file::parse(FIXTURE.as_bytes()).unwrap();
    let measured =
        fixture_file::parse(FIXTURE.replace("legacy-v1", "measured-v2").as_bytes()).unwrap();
    assert_ne!(legacy.digest, measured.digest);
}

#[test]
fn inspection_command_does_not_execute_candidates() {
    use std::process::Command;
    let path = std::env::temp_dir().join(format!(
        "sese-fixture-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, FIXTURE).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sese-fixture"))
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["executed"], false);
    assert_eq!(value["case_count"], 1);
    assert_eq!(
        value["fixture_digest"],
        fixture_file::parse(FIXTURE.as_bytes()).unwrap().digest
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn inspection_can_export_validated_cases_for_synthetic_container_replay() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sese-fixture"))
        .args(["--cases", "fixtures/event-synthetic-v1.json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["executed"], false);
    assert_eq!(value["cases"][0]["id"], "throttle-example");
    assert_eq!(value["cases"][0]["expected"]["decision"], 1);
    assert_eq!(
        value["fixture_digest"],
        fixture_file::load("fixtures/event-synthetic-v1.json")
            .unwrap()
            .digest
    );
}
