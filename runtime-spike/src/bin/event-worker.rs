//! Fixed synthetic protocol; this process is not an OS sandbox.
use sese_runtime_spike::event_abi::{evaluate_event, Decision, Event, Window};
use std::io::{Read, Write};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => (),
        [arg] if arg == "--require-isolation-v1" => sese_runtime_spike::isolation_canary::verify()?,
        _ => return Err("invalid worker arguments".into()),
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    sese_runtime_spike::worker_limits::apply()?;
    let mut data = Vec::new();
    std::io::stdin().take(65_593).read_to_end(&mut data)?;
    if data.len() < 56 || data.len() > 65_592 {
        return Err("invalid request length".into());
    }
    let u64_at = |i: usize| u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
    let u32_at = |i: usize| u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
    let f64_at = |i: usize| f64::from_bits(u64_at(i));
    let window = Window {
        count: u64_at(0),
        mean: f64_at(8),
        p99_us: u64_at(16),
        last_error: u32_at(24),
    };
    let event = Event {
        timestamp_ns: u64_at(28),
        entity_id: u64_at(36),
        metric: f64_at(44),
        flags: u32_at(52),
    };
    let result = evaluate_event(&data[56..], &window, &event)?;
    let code: u32 = match result.decision {
        Decision::Allow => 0,
        Decision::Throttle => 1,
        Decision::Quarantine => 2,
        Decision::Escalate => 3,
    };
    let mut out = std::io::stdout();
    out.write_all(&code.to_le_bytes())?;
    out.write_all(&result.score.to_le_bytes())?;
    out.write_all(&result.reason.to_le_bytes())?;
    Ok(())
}
fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}
