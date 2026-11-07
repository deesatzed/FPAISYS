//! Fixed synthetic lifecycle. This does not provide isolation for untrusted modules.
use sese_runtime_spike::{
    host::DurableHost,
    replay::{replay_window, WindowCase},
    storage::Storage,
};
use std::{
    io::{self, Write},
    time::Duration,
};
fn choice(prompt: &str, allowed: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let answer = line.trim();
    if !allowed.contains(&answer) {
        return Err("explicit valid choice required".into());
    }
    Ok(answer.to_owned())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        return Err("usage: sese-cycle-demo WINDOW_WORKER_PATH [NEW_DATABASE_PATH]".into());
    }
    let conn = if args.len() == 2 {
        let _new_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?;
        rusqlite::Connection::open_with_flags(
            &args[1],
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?
    } else {
        rusqlite::Connection::open_in_memory()?
    };
    let mut db = Storage::from_connection(conn)?;
    let original=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))")?;
    let candidate=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 1 i32.wrap_i64 local.get 2 i32.add))")?;
    let original = db.register_synthetic_window(&original)?;
    let candidate = db.register_synthetic_window(&candidate)?;
    let cases = [WindowCase {
        id: "prior-sum".into(),
        window: vec![3],
        input: 5,
        expected: 8,
    }];
    let replay = replay_window(
        &mut db,
        &args[0],
        &candidate,
        "synthetic-cycle-v1",
        &cases,
        Duration::from_secs(2),
    )?;
    let (version, digest) = db.window_replay_identity(replay.run_id)?;
    db.pin_window_fixture("synthetic-cycle", &version, &digest, "local-operator")?;
    db.bootstrap_champion("synthetic-cycle", &original, "local-operator")?;
    let mut host = DurableHost::open(&mut db, "synthetic-cycle", 32)?;
    let seed = host.process_domain(
        &mut db,
        &args[0],
        "synthetic-cycle",
        &candidate,
        3,
        Duration::from_secs(2),
    )?;
    if seed.champion != Some(3) || seed.shadow != Some(3) {
        return Err("synthetic seed failed".into());
    }
    let before = host.process_domain(
        &mut db,
        &args[0],
        "synthetic-cycle",
        &candidate,
        5,
        Duration::from_secs(2),
    )?;
    if before.champion != Some(5) || before.shadow != Some(8) || !replay.passed {
        return Err("synthetic evidence failed".into());
    }
    println!("Synthetic cycle only; no untrusted modules or production traffic.");
    println!(
        "Before review: champion=5 shadow=8; replay run {} observation {}",
        replay.run_id, before.id
    );
    println!("Human: review prior-window influence.\nBridge: compare synthetic replay and shadow results.\nProposal: add the previous window sum to the event.");
    let decision = choice("Enter approve or reject: ", &["approve", "reject"])?;
    let review = db.record_window_review_with_card(
        &candidate,
        replay.run_id,
        before.id,
        "local-operator",
        &decision,
        "Review prior-window influence.",
        "Compare synthetic replay and shadow results.",
        "Add the previous window sum to the event.",
    )?;
    println!("Review {review}: {decision}; outcome pending.");
    if decision == "reject" {
        println!(
            "Rejected; champion unchanged: {}",
            db.current_champion("synthetic-cycle")?
        );
        return Ok(());
    }
    db.promote_window("synthetic-cycle", &original, review, "local-operator")?;
    let after = host.process_domain(
        &mut db,
        &args[0],
        "synthetic-cycle",
        &original,
        -1,
        Duration::from_secs(2),
    )?;
    let value = after.champion.ok_or("promoted synthetic guest failed")?;
    println!("After promotion: champion={value}");
    let action = choice("Enter rollback or keep: ", &["rollback", "keep"])?;
    if action == "rollback" {
        db.rollback_window("synthetic-cycle", &candidate, "local-operator")?;
        let rolled = host.process_domain(
            &mut db,
            &args[0],
            "synthetic-cycle",
            &candidate,
            5,
            Duration::from_secs(2),
        )?;
        println!(
            "After rollback: champion={}",
            rolled.champion.ok_or("rolled back guest failed")?
        );
    }
    println!("Ledger valid: {}", db.verify_ledger()?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("cycle demo failed: {error}");
        std::process::exit(1);
    }
}
