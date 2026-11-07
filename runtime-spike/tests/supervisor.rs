use sese_runtime_spike::supervise;
use std::time::Duration;

#[test]
fn worker_returns_actual_wasm_result() {
    let wasm = wat::parse_str("(module (func (export \"evaluate\") (param i32) (result i32) local.get 0 i32.const 1 i32.add))").unwrap();
    assert_eq!(
        supervise(
            env!("CARGO_BIN_EXE_worker"),
            &wasm,
            41,
            Duration::from_secs(2)
        )
        .unwrap(),
        42
    );
}

#[test]
fn failed_worker_does_not_return_a_result() {
    assert!(supervise("/usr/bin/false", &[], 0, Duration::from_secs(2)).is_err());
}

#[test]
fn missing_worker_response_is_an_error() {
    assert!(supervise("/usr/bin/true", &[], 0, Duration::from_secs(2)).is_err());
}

#[test]
fn invalid_module_is_an_error() {
    assert!(supervise(
        env!("CARGO_BIN_EXE_worker"),
        b"invalid",
        0,
        Duration::from_secs(2)
    )
    .is_err());
}

#[test]
fn zero_deadline_fails_closed() {
    let wasm = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) (loop br 0) i32.const 0))",
    )
    .unwrap();
    assert!(supervise(env!("CARGO_BIN_EXE_worker"), &wasm, 0, Duration::ZERO).is_err());
}

#[cfg(unix)]
#[test]
fn deadline_includes_pipe_eof_after_worker_exits() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};
    let path = std::env::temp_dir().join(format!(
        "sese-worker-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, "#!/bin/sh\n/bin/sleep 1 &\nexit 0\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let start = Instant::now();
    let result = supervise(path.to_str().unwrap(), &[], 0, Duration::from_millis(80));
    let elapsed = start.elapsed();
    std::fs::remove_file(path).unwrap();
    assert!(result.is_err());
    assert!(elapsed < Duration::from_millis(600), "elapsed: {elapsed:?}");
}

#[cfg(target_os = "macos")]
#[test]
fn timeout_closes_parent_pipe_even_if_descendant_escapes_process_group() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sese-pipe-close-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let script = dir.join("worker.py");
    let marker = dir.join("result");
    let ready = dir.join("ready");
    let release = dir.join("release");
    let marker_literal = serde_json::to_string(marker.to_str().unwrap()).unwrap();
    let ready_literal = serde_json::to_string(ready.to_str().unwrap()).unwrap();
    let release_literal = serde_json::to_string(release.to_str().unwrap()).unwrap();
    std::fs::write(&script, format!("#!/usr/bin/python3\nimport os,time\nready={ready_literal}\nrelease={release_literal}\nif os.fork():\n deadline=time.monotonic()+5\n while not os.path.exists(ready) and time.monotonic()<deadline: time.sleep(0.01)\n os._exit(0)\nos.setsid()\nwith open(ready,'w') as f: f.write('ready')\ndeadline=time.monotonic()+8\nwhile not os.path.exists(release) and time.monotonic()<deadline: time.sleep(0.01)\nif not os.path.exists(release): os._exit(2)\ntry:\n os.write(1,b'x')\n result='open'\nexcept BrokenPipeError:\n result='closed'\nwith open({marker_literal},'w') as f: f.write(result)\nos._exit(0)\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let worker = script.to_str().unwrap().to_owned();
    let supervisor = std::thread::spawn(move || supervise(&worker, &[], 0, Duration::from_secs(3)));
    let mut ready_seen = false;
    for _ in 0..100 {
        if std::fs::read_to_string(&ready).unwrap_or_default() == "ready" {
            ready_seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let result = supervisor.join().unwrap();
    std::fs::write(&release, b"release").unwrap();
    let mut observed = String::new();
    for _ in 0..100 {
        observed = std::fs::read_to_string(&marker).unwrap_or_default();
        if !observed.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ready_seen, "escaped descendant did not signal readiness");
    assert!(result.unwrap_err().contains("deadline exceeded"));
    assert_eq!(observed, "closed");
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn deadline_covers_worker_that_never_reads_large_request() {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!(
        "sese-blocked-input-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, "#!/bin/sh\n/bin/sleep 1\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let start = std::time::Instant::now();
    assert!(supervise(
        path.to_str().unwrap(),
        &vec![0; 65_536],
        0,
        Duration::from_millis(100)
    )
    .is_err());
    assert!(start.elapsed() < Duration::from_millis(600));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn window_worker_receives_prior_snapshot_and_event() {
    let wasm = wat::parse_str("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 0 local.get 1 i32.wrap_i64 i32.add local.get 2 i32.add))").unwrap();
    let result = sese_runtime_spike::supervise_window(
        env!("CARGO_BIN_EXE_window-worker"),
        &wasm,
        &[10, 20],
        3,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result, 35);
}

#[test]
fn window_worker_rejects_wrong_abi_and_excessive_window() {
    let scalar = wat::parse_str(
        "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
    )
    .unwrap();
    assert!(sese_runtime_spike::supervise_window(
        env!("CARGO_BIN_EXE_window-worker"),
        &scalar,
        &[],
        1,
        Duration::from_secs(2)
    )
    .is_err());
    assert!(sese_runtime_spike::supervise_window(
        env!("CARGO_BIN_EXE_window-worker"),
        &scalar,
        &vec![0; 4097],
        1,
        Duration::from_secs(2)
    )
    .is_err());
}

#[test]
fn window_worker_rejects_truncated_protocol() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_window-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&[1, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert!(!child.wait().unwrap().success());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn deadline_terminates_worker_process_group_descendants() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!(
        "sese-descendant-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let marker = base.join("escaped");
    let script = base.join("worker.sh");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n(/bin/sleep 0.5; : > {}) &\nexit 0\n",
            quote(&marker)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(supervise(script.to_str().unwrap(), &[], 0, Duration::from_millis(80)).is_err());
    std::thread::sleep(Duration::from_millis(700));
    assert!(!marker.exists(), "descendant outlived deadline");
    std::fs::remove_dir_all(base).unwrap();
}

#[cfg(unix)]
#[test]
fn zero_deadline_does_not_spawn_worker() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!(
        "sese-zero-deadline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let marker = base.join("started");
    let script = base.join("worker.sh");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&script, format!("#!/bin/sh\n: > {}\n", quote(&marker))).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(supervise(script.to_str().unwrap(), &[], 0, Duration::ZERO).is_err());
    assert!(!marker.exists());
    std::fs::remove_dir_all(base).unwrap();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn worker_does_not_inherit_unrelated_open_descriptor() {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "sese-fd-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 200) };
    assert!(fd >= 200);
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let worker = dir.join("fd-worker.py");
    std::fs::write(&worker,format!("#!/usr/bin/python3\nimport os,struct,sys\ntry:\n os.fstat({fd})\n inherited=1\nexcept OSError:\n inherited=0\nsys.stdin.buffer.read()\nsys.stdout.buffer.write(struct.pack('<i',inherited))\n")).unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = supervise(worker.to_str().unwrap(), &[], 0, Duration::from_secs(3));
    unsafe {
        libc::close(fd);
    }
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(result.unwrap(), 0, "worker inherited unrelated descriptor");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_worker_does_not_start_when_descriptor_closure_is_denied() {
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    const CHILD: &str = "SESE_TEST_CLOSE_RANGE_DENIED_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let dir = std::env::temp_dir().join(format!("sese-close-denied-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let marker = dir.join("started");
        let worker = dir.join("worker.sh");
        std::fs::write(&worker, format!("#!/bin/sh\n: > '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error =
            supervise(worker.to_str().unwrap(), &[], 0, Duration::from_secs(3)).unwrap_err();
        assert!(error.contains("permitted"), "{error}");
        assert!(!marker.exists());
        std::fs::remove_dir_all(dir).unwrap();
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .arg("--exact")
        .arg("linux_worker_does_not_start_when_descriptor_closure_is_denied")
        .env(CHILD, "1");
    unsafe {
        child.pre_exec(|| {
            let filter = [
                libc::sock_filter {
                    code: 0x20,
                    jt: 0,
                    jf: 0,
                    k: 0,
                }, // load syscall number
                libc::sock_filter {
                    code: 0x15,
                    jt: 0,
                    jf: 1,
                    k: libc::SYS_close_range as u32,
                },
                libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
                },
                libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: libc::SECCOMP_RET_ALLOW,
                },
            ];
            let program = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_ptr() as *mut _,
            };
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn nonexistent_worker_is_reported_as_spawn_failure() {
    let path = std::env::temp_dir().join(format!(
        "sese-absent-worker-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let error = supervise(path.to_str().unwrap(), &[], 0, Duration::from_secs(2)).unwrap_err();
    assert!(!error.contains("deadline"), "{error}");
}

#[cfg(target_os = "macos")]
#[test]
fn worker_standard_streams_do_not_offer_reverse_channels() {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!(
        "sese-direction-worker-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path,"#!/usr/bin/python3\nimport os,struct,sys\nsys.stdin.buffer.read()\ntry:\n os.write(0,b'x')\n input_writable=True\nexcept OSError:\n input_writable=False\noutput_readable = os.read(1,1) != b''\nsys.stdout.buffer.write(struct.pack('<i',int(input_writable or output_readable)))\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = supervise(path.to_str().unwrap(), &[], 0, Duration::from_secs(3));
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        result.unwrap(),
        0,
        "worker received a reverse-direction channel"
    );
}
