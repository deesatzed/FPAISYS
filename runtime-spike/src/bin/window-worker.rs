//! Synthetic worker protocol: u32 count, i32 event, count i32 values, then Wasm bytes.
use std::io::{Read, Write};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    sese_runtime_spike::worker_limits::apply()?;
    let mut data = Vec::new();
    std::io::stdin().take(81_929).read_to_end(&mut data)?;
    if data.len() < 8 || data.len() > 81_928 {
        return Err("invalid request size".into());
    }
    let count = u32::from_le_bytes(data[0..4].try_into()?) as usize;
    if count > 4096 || data.len() < 8 + count * 4 {
        return Err("invalid window size".into());
    }
    let input = i32::from_le_bytes(data[4..8].try_into()?);
    let end = 8 + count * 4;
    let window: Vec<i32> = data[8..end]
        .chunks_exact(4)
        .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    let result = sese_runtime_spike::evaluate_window(&data[end..], &window, input)?;
    std::io::stdout().write_all(&result.to_le_bytes())?;
    Ok(())
}
fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}
