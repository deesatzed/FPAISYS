//! Synthetic, non-promoting demonstration. No untrusted inputs are executed.
use sese_runtime_spike::{
    host::Host,
    replay::{replay, Case},
    storage::Storage,
};
use std::{
    io::{self, Write},
    time::Duration,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let worker = std::env::args()
        .nth(1)
        .ok_or("usage: sese-demo WORKER_PATH [DATABASE_PATH]")?;
    let path = std::env::args().nth(2);
    let conn = match &path {
        Some(p) => rusqlite::Connection::open(p)?,
        None => rusqlite::Connection::open_in_memory()?,
    };
    let mut db = Storage::from_connection(conn)?;
    let champion = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
    )?;
    let candidate = wat::parse_str("(module (func (export \"evaluate\") (param i32) (result i32) local.get 0 i32.const 1 i32.add))")?;
    let champ_id = db.register_synthetic_scalar(&champion)?;
    let candidate_id = db.register_synthetic_scalar(&candidate)?;
    let cases = [Case {
        id: "increment".into(),
        input: 1,
        expected: 2,
    }];
    let report = replay(
        &mut db,
        &worker,
        &candidate_id,
        "synthetic-v1",
        &cases,
        Duration::from_secs(2),
    )?;
    let mut host = Host::new(32)?;
    let observation = host.process(
        &mut db,
        &worker,
        &champ_id,
        &candidate_id,
        1,
        Duration::from_secs(2),
    )?;
    println!("Synthetic demo; no production data or automatic promotion.");
    println!(
        "Candidate: {candidate_id}\nReplay passed: {} (run {})",
        report.passed, report.run_id
    );
    println!(
        "Champion: {:?}; shadow: {:?}; divergence: {:?} (observation {})",
        observation.champion, observation.shadow, observation.diverged, observation.id
    );
    println!("Human working map: Review increment behavior.\nBridge: Replay and shadow are synthetic.\nProposal: Candidate returns input plus one.");
    print!("Enter approve or reject: ");
    io::stdout().flush()?;
    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let choice = choice.trim();
    if !["approve", "reject"].contains(&choice) {
        return Err("explicit approve or reject required".into());
    }
    let review_id = db.record_review(
        &candidate_id,
        report.run_id,
        observation.id,
        "local-operator",
        choice,
    )?;
    println!("Structured review: {review_id}");
    println!(
        "Decision recorded: {choice} (review {review_id}); ledger valid: {}",
        db.verify_ledger()?
    );
    println!("Promotion unavailable; this demo does not enforce promotion gates.");
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("demo failed: {e}");
        std::process::exit(1);
    }
}
