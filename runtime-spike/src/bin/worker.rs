use std::io::{Read, Write};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    sese_runtime_spike::worker_limits::apply()?;
    let mut data = Vec::new();
    std::io::stdin().take(65_541).read_to_end(&mut data)?;
    if data.len() < 4 || data.len() > 65_540 {
        return Err("invalid input length".into());
    }
    let input = i32::from_le_bytes(data[..4].try_into()?);
    let value = sese_runtime_spike::evaluate(&data[4..], input)?;
    std::io::stdout().write_all(&value.to_le_bytes())?;
    Ok(())
}
fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}
