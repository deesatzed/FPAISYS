use std::process::Command;

#[test]
fn known_failed_followup_cannot_be_selected() {
    let dir = std::env::temp_dir().join(format!(
        "sese-negative-followup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("db.sqlite");
    let follow_path = dir.join("follow.json");
    let mut fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/event-followup-synthetic-v1.json")).unwrap();
    fixture["cases"][0]["expected"]["decision"] = 0.into();
    std::fs::write(&follow_path, serde_json::to_vec(&fixture).unwrap()).unwrap();
    // The normal demo and lower-level selector must both reject a known failure.
    let output = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            path.to_str().unwrap(),
            "--fixture",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/event-synthetic-v1.json"
            ),
            "--followup-fixture",
            follow_path.to_str().unwrap(),
            "--followup-case",
            "post-promotion",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("synthetic follow-up replay failed"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-followup-pin",
            path.to_str().unwrap(),
            "typed-cycle",
            "2",
            "post-promotion",
            "operator",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("follow-up replay failed"));
    let selections: i64 = conn
        .query_row("SELECT count(*) FROM event_followup_pins", [], |r| r.get(0))
        .unwrap();
    assert_eq!(selections, 0);
    // Recreate the ledger-bound selection permitted by earlier versions, then
    // verify that promotion also fails closed for that historical database.
    use sha2::{Digest, Sha256};
    let candidate: String = conn
        .query_row(
            "SELECT candidate_id FROM event_replay_runs WHERE id=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let original: String = conn
        .query_row(
            "SELECT id FROM candidates WHERE id != ?1",
            [&candidate],
            |r| r.get(0),
        )
        .unwrap();
    let run_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_replay' AND reference=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let (seq, prev): (i64, String) = conn
        .query_row(
            "SELECT seq,hash FROM ledger_events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    conn.execute("INSERT INTO event_followup_pins(id,domain,candidate_id,replay_id,case_id,actor,created_unix_ns) VALUES (1,'typed-cycle',?1,2,'post-promotion','operator',?2)", rusqlite::params![candidate,time]).unwrap();
    let encode = |fields: &[&str]| {
        fields
            .iter()
            .map(|s| format!("{}:{}", s.len(), s))
            .collect::<String>()
    };
    let payload = encode(&[
        "synthetic-event-followup-selection-v1",
        "typed-cycle",
        &candidate,
        "2",
        "post-promotion",
        "operator",
        &run_hash,
    ]);
    let hash = format!(
        "{:x}",
        Sha256::digest(
            encode(&[
                "sese-ledger-v1",
                &prev,
                "event_followup_pin",
                "1",
                &payload,
                &time
            ])
            .as_bytes()
        )
    );
    conn.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_followup_pin',1,?2,?3,?4,?5)",
        rusqlite::params![seq + 1, payload, time, prev, hash],
    )
    .unwrap();
    let cli = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .arg(args[0])
            .arg(&path)
            .args(&args[1..])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr)
        );
    };
    cli(&[
        "event-builtin-replay",
        "throttle",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/event-synthetic-v1.json"
        ),
    ]);
    cli(&["event-bootstrap", "typed-cycle", &original, "operator"]);
    cli(&[
        "event-builtin-observe",
        "typed-cycle",
        "stream",
        "allow",
        "throttle",
        "legacy-v1",
        "1",
        "42",
        "3",
        "4",
    ]);
    cli(&[
        "event-review",
        &candidate,
        "3",
        "1",
        "operator",
        "approve",
        "human",
        "bridge",
        "proposal",
    ]);
    let promotion = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-promote",
            path.to_str().unwrap(),
            "typed-cycle",
            &original,
            "1",
            "operator",
        ])
        .output()
        .unwrap();
    assert!(!promotion.status.success());
    assert!(String::from_utf8_lossy(&promotion.stderr).contains("follow-up replay failed"));
    let current: String = conn
        .query_row(
            "SELECT champion_id FROM event_champions WHERE domain='typed-cycle'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, original);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}
