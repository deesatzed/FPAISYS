//! Controlled test-owned guests only. This does not authorize untrusted execution.
#![cfg(unix)]

use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn restricted_worker_accepts_valid_guest_and_rejects_import_fuel_and_memory_failures() {
    let image = match std::env::var("SESE_TEST_CANARY_IMAGE") {
        Ok(value) => value,
        Err(_) => return,
    };
    let host = std::env::var("SESE_TEST_DOCKER_HOST").expect("explicit Docker host required");
    assert!(host.starts_with("unix:///"));
    assert!(image
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())));
    let docker =
        std::env::var("SESE_TEST_DOCKER").unwrap_or_else(|_| "/usr/local/bin/docker".into());
    let signature = "(param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32)";
    let cases = [
        ("valid", format!("(module (func (export \"evaluate_event_v1\") {signature} i32.const 1 local.get 6 local.get 7))"), true),
        ("import", format!("(module (import \"host\" \"io\" (func)) (func (export \"evaluate_event_v1\") {signature} i32.const 1 local.get 6 local.get 7))"), false),
        ("fuel", format!("(module (func (export \"evaluate_event_v1\") {signature} (loop br 0) i32.const 1 local.get 6 local.get 7))"), false),
        ("memory", format!("(module (memory 1024) (func (export \"evaluate_event_v1\") {signature} i32.const 1 local.get 6 local.get 7))"), false),
    ];
    for (name, wat, expected_success) in cases {
        let artifact = wat::parse_str(wat).unwrap();
        let mut request = Vec::new();
        request.extend_from_slice(&0u64.to_le_bytes());
        request.extend_from_slice(&0f64.to_le_bytes());
        request.extend_from_slice(&0u64.to_le_bytes());
        request.extend_from_slice(&0u32.to_le_bytes());
        request.extend_from_slice(&1u64.to_le_bytes());
        request.extend_from_slice(&42u64.to_le_bytes());
        request.extend_from_slice(&3f64.to_le_bytes());
        request.extend_from_slice(&4u32.to_le_bytes());
        request.extend_from_slice(&artifact);
        let create = Command::new(&docker)
            .args([
                "--host",
                &host,
                "create",
                "--pull=never",
                "--interactive",
                "--network=none",
                "--read-only",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges",
                "--pids-limit=16",
                "--memory=128m",
                "--memory-swap=128m",
                "--cpus=1",
                "--user=65534:65534",
                &image,
            ])
            .output()
            .unwrap();
        assert!(
            create.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&create.stderr)
        );
        let id = String::from_utf8(create.stdout).unwrap().trim().to_owned();
        assert_eq!(id.len(), 64);
        struct Cleanup<'a> {
            docker: &'a str,
            host: &'a str,
            id: &'a str,
            removed: bool,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                if !self.removed {
                    let _ = Command::new(self.docker)
                        .args(["--host", self.host, "rm", "-f", self.id])
                        .output();
                }
            }
        }
        let mut cleanup = Cleanup {
            docker: &docker,
            host: &host,
            id: &id,
            removed: false,
        };
        let mut child = Command::new(&docker)
            .args(["--host", &host, "start", "--attach", "--interactive", &id])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&request).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{name}: worker exceeded test deadline");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let result = child.wait_with_output().unwrap();
        assert_eq!(
            result.status.success(),
            expected_success,
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if expected_success {
            let mut expected = Vec::new();
            expected.extend_from_slice(&1u32.to_le_bytes());
            expected.extend_from_slice(&3f64.to_le_bytes());
            expected.extend_from_slice(&4u32.to_le_bytes());
            assert_eq!(result.stdout, expected, "{name}");
        } else {
            assert!(result.stdout.is_empty(), "{name} returned a verdict");
        }
        let removed = Command::new(&docker)
            .args(["--host", &host, "rm", "-f", &id])
            .output()
            .unwrap();
        assert!(removed.status.success(), "{name}: removal failed");
        let remaining = Command::new(&docker)
            .args([
                "--host",
                &host,
                "ps",
                "-aq",
                "--no-trunc",
                "--filter",
                &format!("id={id}"),
            ])
            .output()
            .unwrap();
        assert!(
            remaining.status.success() && remaining.stdout.is_empty(),
            "{name}: removal unconfirmed"
        );
        cleanup.removed = true;
    }
}
