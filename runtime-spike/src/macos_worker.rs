//! macOS worker launch: close unlisted descriptors atomically. Not an OS sandbox.
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::{Duration, Instant};

fn checked(code: libc::c_int) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(code).to_string())
    }
}

struct SpawnConfig {
    actions: libc::posix_spawn_file_actions_t,
    attr: libc::posix_spawnattr_t,
}
impl SpawnConfig {
    fn new() -> Result<Self, String> {
        unsafe {
            let mut actions = std::mem::zeroed();
            checked(libc::posix_spawn_file_actions_init(&mut actions))?;
            let mut attr = std::mem::zeroed();
            let rc = libc::posix_spawnattr_init(&mut attr);
            if rc != 0 {
                libc::posix_spawn_file_actions_destroy(&mut actions);
                return Err(std::io::Error::from_raw_os_error(rc).to_string());
            }
            Ok(Self { actions, attr })
        }
    }
}
impl Drop for SpawnConfig {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.attr);
            libc::posix_spawn_file_actions_destroy(&mut self.actions);
        }
    }
}

struct GroupChild {
    pid: libc::pid_t,
    reaped: bool,
}
impl GroupChild {
    fn cleanup(&mut self) -> Result<std::process::ExitStatus, String> {
        let rc = unsafe { libc::killpg(self.pid, libc::SIGKILL) };
        let error = if rc == 0 {
            None
        } else {
            Some(std::io::Error::last_os_error())
        };
        if error.is_some() {
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
            }
        }
        let mut status = 0;
        loop {
            let rc = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if rc == self.pid {
                break;
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e.to_string());
            }
        }
        self.reaped = true;
        if let Some(e) = error {
            if ![Some(libc::ESRCH), Some(libc::EPERM)].contains(&e.raw_os_error()) {
                return Err(format!("worker group cleanup failed: {e}"));
            }
        }
        Ok(std::process::ExitStatus::from_raw(status))
    }
}
impl Drop for GroupChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.cleanup();
        }
    }
}

fn spawn(worker: &str, args: &[&str]) -> Result<(GroupChild, UnixStream, UnixStream), String> {
    let executable = CString::new(Path::new(worker).as_os_str().as_bytes())
        .map_err(|_| "invalid worker path")?;
    let (input_parent, input_child) = UnixStream::pair().map_err(|e| e.to_string())?;
    let (output_parent, output_child) = UnixStream::pair().map_err(|e| e.to_string())?;
    // Match pipe directionality: the child reads stdin and writes stdout only.
    input_child
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| e.to_string())?;
    output_child
        .shutdown(std::net::Shutdown::Read)
        .map_err(|e| e.to_string())?;
    if input_child.as_raw_fd() <= 2 || output_child.as_raw_fd() <= 2 {
        return Err("worker stdio descriptors unavailable".into());
    }
    let mut config = SpawnConfig::new()?;
    unsafe {
        checked(libc::posix_spawn_file_actions_adddup2(
            &mut config.actions,
            input_child.as_raw_fd(),
            libc::STDIN_FILENO,
        ))?;
        checked(libc::posix_spawn_file_actions_adddup2(
            &mut config.actions,
            output_child.as_raw_fd(),
            libc::STDOUT_FILENO,
        ))?;
        let devnull = b"/dev/null\0";
        checked(libc::posix_spawn_file_actions_addopen(
            &mut config.actions,
            libc::STDERR_FILENO,
            devnull.as_ptr().cast(),
            libc::O_WRONLY,
            0,
        ))?;
        checked(libc::posix_spawnattr_setflags(
            &mut config.attr,
            (libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETPGROUP) as libc::c_short,
        ))?;
        checked(libc::posix_spawnattr_setpgroup(&mut config.attr, 0))?;
        let arguments: Vec<CString> = args
            .iter()
            .map(|arg| CString::new(*arg).map_err(|_| "invalid worker argument".to_string()))
            .collect::<Result<_, _>>()?;
        let mut argv: Vec<*mut libc::c_char> =
            std::iter::once(executable.as_ptr() as *mut libc::c_char)
                .chain(
                    arguments
                        .iter()
                        .map(|arg| arg.as_ptr() as *mut libc::c_char),
                )
                .collect();
        argv.push(std::ptr::null_mut());
        let env = [std::ptr::null_mut()];
        let mut pid = 0;
        checked(libc::posix_spawn(
            &mut pid,
            executable.as_ptr(),
            &config.actions,
            &config.attr,
            argv.as_ptr(),
            env.as_ptr(),
        ))?;
        if pid <= 0 {
            return Err("invalid worker pid".into());
        }
        Ok((
            GroupChild { pid, reaped: false },
            input_parent,
            output_parent,
        ))
    }
}

pub(crate) fn supervise_request_with_args(
    worker: &str,
    args: &[&str],
    data: Vec<u8>,
    timeout: Duration,
    expected: usize,
) -> Result<Vec<u8>, String> {
    if timeout.is_zero() {
        return Err("worker deadline exceeded".into());
    }
    let (mut group, input, mut stdout) = spawn(worker, args)?;
    let mut stdin = Some(input);
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
                Ok(n) => {
                    output.extend_from_slice(&buffer[..n]);
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
        if !exited {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    group.pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc != 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() != std::io::ErrorKind::Interrupted {
                    return Err(e.to_string());
                }
            } else if unsafe { info.si_pid() } == group.pid {
                exited = true;
            }
        }
        if exited && eof && stdin.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2).min(timeout.saturating_sub(start.elapsed())));
    }
    if !group.cleanup()?.success() {
        return Err("worker failed".into());
    }
    if output.len() != expected {
        return Err("invalid worker response".into());
    }
    Ok(output)
}
