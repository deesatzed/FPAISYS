use sese_runtime_spike::ledger::Ledger;

#[test]
fn outcome_requires_a_decision_and_only_resolves_once() {
    let mut ledger = Ledger::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    assert!(ledger
        .outcome(999, "validated", "operator", "synthetic audit")
        .is_err());
    let id = ledger
        .decision(
            "candidate-1",
            "operator",
            "approve",
            "human map",
            "bridge",
            "proposal",
        )
        .unwrap();
    ledger
        .outcome(id, "validated", "operator", "synthetic audit")
        .unwrap();
    assert!(ledger
        .outcome(id, "falsified", "operator", "contradiction")
        .is_err());
    assert!(ledger.verify().unwrap());
}

#[test]
fn detects_modified_event() {
    let path = std::env::temp_dir().join(format!(
        "sese-ledger-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut ledger = Ledger::new(rusqlite::Connection::open(&path).unwrap()).unwrap();
    ledger
        .decision(
            "candidate-1",
            "operator",
            "reject",
            "map",
            "bridge",
            "proposal",
        )
        .unwrap();
    drop(ledger);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE ledger_events SET payload='altered'", [])
        .unwrap();
    let ledger = Ledger::new(conn).unwrap();
    assert!(!ledger.verify().unwrap());
    drop(ledger);
    std::fs::remove_file(path).unwrap();
}
