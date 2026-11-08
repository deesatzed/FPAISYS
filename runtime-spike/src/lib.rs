//! Experimental interpreter only: not an OS sandbox, not for untrusted artifacts.
use wasmi::{Config, EnforcedLimits, Engine, Linker, Module, Store, StoreLimitsBuilder};
#[cfg(all(test, unix))]
mod supervisor_argument_tests {
    #[test]
    fn passes_literal_arguments_without_a_shell() {
        let output = super::supervise_request_with_args(
            "/usr/bin/printf",
            &["%s", "a b;$x"],
            Vec::new(),
            std::time::Duration::from_secs(2),
            6,
        )
        .unwrap();
        assert_eq!(output, b"a b;$x");
    }
}
pub mod attestation;
pub mod builtin_event;
#[cfg(unix)]
mod container_observation;
#[cfg(unix)]
mod container_review;
#[cfg(unix)]
mod container_shadow;
pub mod event_executor;
pub mod fixture_file;
pub mod isolation_canary;
pub mod proposal;
pub mod reward_rule;
mod synthetic_followup;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod worker_limits;

fn instantiate(
    bytes: &[u8],
) -> Result<(wasmi::Store<wasmi::StoreLimits>, wasmi::Instance), String> {
    if bytes.len() > 65_536 {
        return Err("module too large".into());
    }
    let mut config = Config::default();
    config.consume_fuel(true);
    config.enforced_limits(EnforcedLimits::strict());
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes).map_err(|e| e.to_string())?;
    if module.imports().next().is_some() {
        return Err("imports are forbidden".into());
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(262_144)
        .memories(1)
        .tables(1)
        .table_elements(1024)
        .trap_on_grow_failure(true)
        .instances(1)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store.set_fuel(10_000).map_err(|e| e.to_string())?;
    let linker = Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| e.to_string())?
        .start(&mut store)
        .map_err(|e| e.to_string())?;
    Ok((store, instance))
}

pub fn evaluate(bytes: &[u8], input: i32) -> Result<i32, String> {
    let (mut store, instance) = instantiate(bytes)?;
    let function = instance
        .get_typed_func::<i32, i32>(&store, "evaluate")
        .map_err(|e| e.to_string())?;
    function.call(&mut store, input).map_err(|e| e.to_string())
}

/// Synthetic ABI v1: prior window count and exact i64 sum, then current scalar event.
/// This in-process function is not a containment boundary for untrusted modules.
pub fn evaluate_window(bytes: &[u8], window: &[i32], input: i32) -> Result<i32, String> {
    if window.len() > 4096 {
        return Err("window too large".into());
    }
    let sum: i64 = window.iter().map(|v| i64::from(*v)).sum();
    let (mut store, instance) = instantiate(bytes)?;
    let function = instance
        .get_typed_func::<(i32, i64, i32), i32>(&store, "evaluate_window_v1")
        .map_err(|e| e.to_string())?;
    function
        .call(&mut store, (window.len() as i32, sum, input))
        .map_err(|e| e.to_string())
}

/// Experimental process supervision; this is not an OS security sandbox.
pub fn supervise(
    worker: &str,
    bytes: &[u8],
    input: i32,
    timeout: std::time::Duration,
) -> Result<i32, String> {
    if bytes.len() > 65_536 {
        return Err("module too large".into());
    }
    let mut data = input.to_le_bytes().to_vec();
    data.extend_from_slice(bytes);
    let result = supervise_request(worker, data, timeout, 4)?;
    Ok(i32::from_le_bytes(
        result.try_into().map_err(|_| "invalid worker response")?,
    ))
}

/// Supervise a synthetic window ABI worker. This is not an OS sandbox.
pub fn supervise_window(
    worker: &str,
    bytes: &[u8],
    window: &[i32],
    input: i32,
    timeout: std::time::Duration,
) -> Result<i32, String> {
    if bytes.len() > 65_536 || window.len() > 4096 {
        return Err("request too large".into());
    }
    let mut data = Vec::with_capacity(8 + 4 * window.len() + bytes.len());
    data.extend_from_slice(&(window.len() as u32).to_le_bytes());
    data.extend_from_slice(&input.to_le_bytes());
    for value in window {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(bytes);
    let result = supervise_request(worker, data, timeout, 4)?;
    Ok(i32::from_le_bytes(
        result.try_into().map_err(|_| "invalid worker response")?,
    ))
}

pub(crate) fn supervise_request(
    worker: &str,
    data: Vec<u8>,
    timeout: std::time::Duration,
    expected: usize,
) -> Result<Vec<u8>, String> {
    supervise_request_with_args(worker, &[], data, timeout, expected)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn supervise_request_with_args(
    worker: &str,
    args: &[&str],
    data: Vec<u8>,
    timeout: std::time::Duration,
    expected: usize,
) -> Result<Vec<u8>, String> {
    if timeout.is_zero() {
        return Err("worker deadline exceeded".into());
    }
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::Instant;
    let mut child = Command::new(worker)
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("missing stdin")?;
    let mut stdout = child.stdout.take().ok_or("missing stdout")?;
    let start = Instant::now();
    let (write_tx, write_rx) = std::sync::mpsc::sync_channel(1);
    let (read_tx, read_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = stdin.write_all(&data);
        let _ = write_tx.send(result);
    });
    std::thread::spawn(move || {
        let mut result = Vec::new();
        let outcome = stdout
            .by_ref()
            .take((expected + 1) as u64)
            .read_to_end(&mut result)
            .map(|_| result);
        let _ = read_tx.send(outcome);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("worker deadline exceeded".to_string());
            }
            Ok(None) => std::thread::sleep(
                std::time::Duration::from_millis(2).min(timeout.saturating_sub(start.elapsed())),
            ),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    if !status.success() {
        return Err("worker failed".into());
    }
    let remaining = timeout.saturating_sub(start.elapsed());
    let write_result = write_rx
        .recv_timeout(remaining)
        .map_err(|_| "worker input deadline exceeded")?;
    write_result.map_err(|e| e.to_string())?;
    let remaining = timeout.saturating_sub(start.elapsed());
    let output = read_rx
        .recv_timeout(remaining)
        .map_err(|_| "worker output deadline exceeded")?
        .map_err(|e| e.to_string())?;
    if output.len() != expected {
        return Err("invalid worker response".into());
    }
    Ok(output)
}

#[cfg(target_os = "macos")]
mod macos_worker;
#[cfg(target_os = "macos")]
pub(crate) use macos_worker::supervise_request_with_args;

/// Process-group cleanup is best effort, not a containment boundary.
#[cfg(target_os = "linux")]
pub(crate) fn supervise_request_with_args(
    worker: &str,
    args: &[&str],
    data: Vec<u8>,
    timeout: std::time::Duration,
    expected: usize,
) -> Result<Vec<u8>, String> {
    if timeout.is_zero() {
        return Err("worker deadline exceeded".into());
    }
    use std::io::{Read, Write};
    use std::os::unix::{io::AsRawFd, process::CommandExt};
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;
    struct GroupChild {
        child: Child,
        pid: libc::pid_t,
        reaped: bool,
    }
    impl GroupChild {
        fn cleanup(&mut self) -> Result<std::process::ExitStatus, String> {
            // The leader is deliberately not reaped until after killpg, preventing PID reuse.
            let rc = unsafe { libc::killpg(self.pid, libc::SIGKILL) };
            let error = if rc != 0 {
                Some(std::io::Error::last_os_error())
            } else {
                None
            };
            if error.is_some() {
                let _ = self.child.kill();
            }
            let status = self.child.wait().map_err(|e| e.to_string())?;
            self.reaped = true;
            // macOS can report EPERM for a group containing only the exited leader.
            // Cleanup is best effort; it does not certify that all descendants were killed.
            if let Some(e) = error {
                if ![Some(libc::ESRCH), Some(libc::EPERM)].contains(&e.raw_os_error()) {
                    return Err(format!("worker group cleanup failed: {e}"));
                }
            }
            Ok(status)
        }
    }
    impl Drop for GroupChild {
        fn drop(&mut self) {
            if !self.reaped {
                let _ = self.cleanup();
            }
        }
    }
    let mut command = Command::new(worker);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    // Run only async-signal-safe operations after fork. Marking descriptors
    // rather than closing them preserves Rust's spawn-error pipe until exec.
    // Unsupported kernels or blocked syscalls fail the spawn closed.
    unsafe {
        command.pre_exec(|| {
            if libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().map_err(|e| e.to_string())?;
    let pid = i32::try_from(child.id()).map_err(|_| "invalid worker pid")?;
    let mut group = GroupChild {
        child,
        pid,
        reaped: false,
    };
    let mut stdin = Some(group.child.stdin.take().ok_or("missing stdin")?);
    let mut stdout = group.child.stdout.take().ok_or("missing stdout")?;
    for fd in [stdin.as_ref().unwrap().as_raw_fd(), stdout.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    let start = Instant::now();
    let mut written = 0;
    let mut output = Vec::with_capacity(expected + 1);
    let mut eof = false;
    let mut exited = false;
    loop {
        if start.elapsed() >= timeout {
            return Err("worker deadline exceeded".into());
        }
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(&data[written..]) {
                Ok(0) if written < data.len() => return Err("worker input closed".into()),
                Ok(count) => written += count,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    ()
                }
                Err(e) => return Err(e.to_string()),
            }
            if written == data.len() {
                stdin.take();
            }
        }
        if !eof {
            let mut buffer = [0u8; 256];
            let max = (expected + 1 - output.len()).min(buffer.len());
            match stdout.read(&mut buffer[..max]) {
                Ok(0) => eof = true,
                Ok(count) => {
                    output.extend_from_slice(&buffer[..count]);
                    if output.len() > expected {
                        return Err("invalid worker response".into());
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    ()
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        // waitid with WNOWAIT observes exit without releasing the leader's PID.
        if !exited {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc != 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() != std::io::ErrorKind::Interrupted {
                    return Err(e.to_string());
                }
            } else if unsafe { info.si_pid() } == pid {
                exited = true;
            }
        }
        if exited && eof && stdin.is_none() {
            break;
        }
        std::thread::sleep(
            std::time::Duration::from_millis(2).min(timeout.saturating_sub(start.elapsed())),
        );
    }
    let status = group.cleanup()?;
    if !status.success() {
        return Err("worker failed".into());
    }
    if output.len() != expected {
        return Err("invalid worker response".into());
    }
    Ok(output)
}

pub mod event_abi;
pub mod event_host;
pub mod event_replay;
pub mod event_state;
pub mod host;
pub mod ledger;
pub mod replay;
pub mod shadow;
pub mod storage;
mod worker_identity;
