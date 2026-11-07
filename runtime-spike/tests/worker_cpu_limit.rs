#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::process::Command;

#[test]
fn built_in_worker_cpu_limit_is_installed_without_raising_inherited_limit() {
    if let Ok(mode) = std::env::var("SESE_CPU_LIMIT_TEST_CHILD") {
        if mode == "inherited" || mode == "burn" {
            let limit = libc::rlimit {
                rlim_cur: 1,
                rlim_max: 1,
            };
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CPU, &limit) }, 0);
        }
        sese_runtime_spike::worker_limits::apply().unwrap();
        if mode == "burn" {
            loop {
                std::hint::black_box(1_u64);
            }
        }
        let mut actual = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CPU, &mut actual) }, 0);
        let expected_max = if mode == "inherited" { 1 } else { 10 };
        assert!(actual.rlim_cur <= expected_max);
        assert!(actual.rlim_max <= expected_max);
        return;
    }
    for mode in ["default", "inherited"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "built_in_worker_cpu_limit_is_installed_without_raising_inherited_limit",
                "--nocapture",
            ])
            .env("SESE_CPU_LIMIT_TEST_CHILD", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn kernel_stops_a_cpu_bound_worker_with_a_stricter_inherited_limit() {
    use std::os::unix::process::ExitStatusExt;
    use std::time::{Duration, Instant};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "built_in_worker_cpu_limit_is_installed_without_raising_inherited_limit",
        ])
        .env("SESE_CPU_LIMIT_TEST_CHILD", "burn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("CPU limit did not stop the child within the test deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        matches!(status.signal(), Some(libc::SIGXCPU) | Some(libc::SIGKILL)),
        "{status}"
    );
}
