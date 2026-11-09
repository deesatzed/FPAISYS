use std::process::Command;

#[test]
fn promotion_rejects_followup_selected_after_approval() {
    let dir = std::env::temp_dir().join(format!(
        "sese-late-followup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("db.sqlite");
    let run = |args: &[&str]| {
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
        output
    };
    run(&["event-init"]);
    let allow: serde_json::Value =
        serde_json::from_slice(&run(&["event-builtin-register", "allow"]).stdout).unwrap();
    let throttle: serde_json::Value =
        serde_json::from_slice(&run(&["event-builtin-register", "throttle"]).stdout).unwrap();
    let allow = allow["candidate_id"].as_str().unwrap();
    let throttle = throttle["candidate_id"].as_str().unwrap();
    let main = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-synthetic-v1.json"
    );
    let follow = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/event-followup-synthetic-v1.json"
    );
    run(&["event-builtin-replay", "throttle", main]);
    run(&["event-pin", "test", "replay:1", "operator"]);
    run(&["event-builtin-replay", "throttle", follow]);
    run(&["event-builtin-replay", "throttle", main]);
    run(&["event-bootstrap", "test", allow, "operator"]);
    run(&[
        "event-builtin-observe",
        "test",
        "stream",
        "allow",
        "throttle",
        "legacy-v1",
        "1",
        "42",
        "3",
        "4",
    ]);
    run(&[
        "event-review",
        throttle,
        "3",
        "1",
        "operator",
        "approve",
        "human",
        "bridge",
        "proposal",
    ]);
    run(&[
        "event-followup-pin",
        "test",
        "2",
        "post-promotion",
        "operator",
    ]);
    let promotion = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-promote",
            path.to_str().unwrap(),
            "test",
            allow,
            "1",
            "operator",
        ])
        .output()
        .unwrap();
    assert!(
        !promotion.status.success(),
        "a selection made after approval cannot qualify that approval"
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let current: String = conn
        .query_row(
            "SELECT champion_id FROM event_champions WHERE domain='test'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, allow);
    // Build a coherent historical copy with the transition that an older
    // version allowed, then audit it with the independent Python verifier.
    use sha2::{Digest, Sha256};
    let legacy = dir.join("legacy.sqlite");
    conn.execute("VACUUM INTO ?1", [legacy.to_str().unwrap()])
        .unwrap();
    let old = rusqlite::Connection::open(&legacy).unwrap();
    let (seq, prev): (i64, String) = old
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
    old.execute("INSERT INTO event_champion_transitions(domain,previous_id,next_id,reference,actor,created_unix_ns,kind) VALUES ('test',?1,?2,1,'operator',?3,'promote')",rusqlite::params![allow,throttle,time]).unwrap();
    let id = old.last_insert_rowid();
    let encode = |fields: &[&str]| {
        fields
            .iter()
            .map(|s| format!("{}:{}", s.len(), s))
            .collect::<String>()
    };
    let payload = encode(&[
        "synthetic-event-champion-v1",
        "test",
        allow,
        throttle,
        "1",
        "operator",
        "promote",
    ]);
    let digest = format!(
        "{:x}",
        Sha256::digest(
            encode(&[
                "sese-ledger-v1",
                &prev,
                "event_champion_transition",
                &id.to_string(),
                &payload,
                &time
            ])
            .as_bytes()
        )
    );
    old.execute(
        "INSERT INTO ledger_events VALUES (?1,'event_champion_transition',?2,?3,?4,?5,?6)",
        rusqlite::params![seq + 1, id, payload, time, prev, digest],
    )
    .unwrap();
    old.execute(
        "UPDATE event_champions SET champion_id=?1,generation=1 WHERE domain='test'",
        [throttle],
    )
    .unwrap();
    drop(old);
    let audit = Command::new("python3")
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
            legacy.to_str().unwrap(),
            "--seq",
            &(seq + 1).to_string(),
            "--hash",
            &digest,
            "--verify-event-followups",
        ])
        .output()
        .unwrap();
    assert!(!audit.status.success());
    assert!(
        String::from_utf8_lossy(&audit.stderr)
            .contains("follow-up selection must precede approval"),
        "{}",
        String::from_utf8_lossy(&audit.stderr)
    );
    run(&[
        "event-review",
        throttle,
        "3",
        "1",
        "operator",
        "approve",
        "human",
        "bridge",
        "proposal",
    ]);
    run(&["event-promote", "test", allow, "2", "operator"]);
    let current: String = conn
        .query_row(
            "SELECT champion_id FROM event_champions WHERE domain='test'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, throttle);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}
