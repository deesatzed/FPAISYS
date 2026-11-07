//! Synthetic preview only; do not run untrusted modules.
use sese_runtime_spike::{
    host::Host,
    replay::{replay_window, WindowCase},
    storage::Storage,
};
use std::{
    io::{self, Write},
    time::Duration,
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        return Err("usage: sese-window-demo WINDOW_WORKER_PATH [DATABASE_PATH]".into());
    }
    let conn = if args.len() == 2 {
        rusqlite::Connection::open(&args[1])?
    } else {
        rusqlite::Connection::open_in_memory()?
    };
    let mut db = Storage::from_connection(conn)?;
    let champion=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 2))")?;
    let candidate=wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 1 i32.wrap_i64 local.get 2 i32.add))")?;
    let champion_id = db.register_synthetic_window(&champion)?;
    let candidate_id = db.register_synthetic_window(&candidate)?;
    let cases = [WindowCase {
        id: "prior-sum".into(),
        window: vec![3],
        input: 5,
        expected: 8,
    }];
    let report = replay_window(
        &mut db,
        &args[0],
        &candidate_id,
        "synthetic-window-v1",
        &cases,
        Duration::from_secs(2),
    )?;
    let mut host = Host::new(32)?;
    let seed = host.process_window(
        &mut db,
        &args[0],
        &champion_id,
        &candidate_id,
        3,
        Duration::from_secs(2),
    )?;
    if seed.champion.is_none() || seed.shadow.is_none() {
        return Err("seed comparison failed".into());
    }
    let prior = host.window();
    let observation = host.process_window(
        &mut db,
        &args[0],
        &champion_id,
        &candidate_id,
        5,
        Duration::from_secs(2),
    )?;
    println!("Synthetic window demo; no production data or promotion.");
    println!(
        "Candidate: {candidate_id}\nWindow replay passed: {} (run {})",
        report.passed, report.run_id
    );
    println!("Prior window: {prior:?}; input: 5; champion: {:?}; shadow: {:?}; divergence: {:?} (observation {})",observation.champion,observation.shadow,observation.diverged,observation.id);
    println!("Human working map: Review prior-window influence.\nBridge: Replay and observation are synthetic.\nProposal: Add prior-window sum to current input.");
    print!("Enter approve or reject: ");
    io::stdout().flush()?;
    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let choice = choice.trim();
    if !["approve", "reject"].contains(&choice) {
        return Err("explicit approve or reject required".into());
    }
    let review = db.record_window_review_with_card(
        &candidate_id,
        report.run_id,
        observation.id,
        "local-operator",
        choice,
        "Review prior-window influence.",
        "Replay and observation are synthetic.",
        "Add prior-window sum to current input.",
    )?;
    println!(
        "Decision recorded: {choice} (window review {review}); ledger valid: {}",
        db.verify_ledger()?
    );
    println!("Promotion unavailable; this demo does not enforce promotion gates.");
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("window demo failed: {e}");
        std::process::exit(1);
    }
}
