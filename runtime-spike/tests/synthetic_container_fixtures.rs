use std::process::Command;

#[test]
fn container_synthetic_artifacts_match_the_rust_builtin_registry() {
    let output = Command::new("python3")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "-c",
            "import sys; sys.path.insert(0, 'scripts'); from synthetic_container_lifecycle import _synthetic_request; [sys.stdout.buffer.write(_synthetic_request(n, (1,42,3.0,4), (0,0.0,0,0))[1]) for n in ('allow','throttle','quarantine','window-probe')]",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<u8> = sese_runtime_spike::builtin_event::NAMES
        .iter()
        .copied()
        .flat_map(|name| sese_runtime_spike::builtin_event::artifact(name).unwrap())
        .collect();
    assert_eq!(output.stdout, expected);
}
