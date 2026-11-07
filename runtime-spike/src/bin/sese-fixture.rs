//! Inspect a typed-event fixture without executing any candidate.
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (show_cases, path) = match args.as_slice() {
        [path] => (false, path),
        [flag, path] if flag == "--cases" => (true, path),
        _ => return Err("usage: sese-fixture [--cases] FIXTURE.json".into()),
    };
    let pack = sese_runtime_spike::fixture_file::load(path)?;
    let mut output = serde_json::json!({"schema":"sese-event-fixture-inspection-v1","version":pack.version,"state_semantics":pack.state_semantics,"fixture_digest":pack.digest,"case_count":pack.cases.len(),"executed":false});
    if show_cases {
        let cases: Vec<_> = pack.cases.iter().map(|c| {
            let decision = match c.expected.decision {
                sese_runtime_spike::event_abi::Decision::Allow => 0,
                sese_runtime_spike::event_abi::Decision::Throttle => 1,
                sese_runtime_spike::event_abi::Decision::Quarantine => 2,
                sese_runtime_spike::event_abi::Decision::Escalate => 3,
            };
            serde_json::json!({"id":c.id,"window":{"count":c.window.count,"mean":c.window.mean,"p99_us":c.window.p99_us,"last_error":c.window.last_error},"event":{"timestamp_ns":c.event.timestamp_ns,"entity_id":c.event.entity_id,"metric":c.event.metric,"flags":c.event.flags},"expected":{"decision":decision,"score":c.expected.score,"reason":c.expected.reason}})
        }).collect();
        output["cases"] = serde_json::json!(cases);
    }
    println!("{output}");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("fixture inspection failed: {error}");
        std::process::exit(1);
    }
}
