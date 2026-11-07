//! Process limits for the built-in synthetic workers; not a sandbox.
pub fn apply() -> Result<(), String> {
    let mut inherited = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_CPU, &mut inherited) } != 0 {
        return Err(format!(
            "cannot read worker CPU limit: {}",
            std::io::Error::last_os_error()
        ));
    }
    let maximum = inherited.rlim_max.min(10);
    let limit = libc::rlimit {
        rlim_cur: inherited.rlim_cur.min(maximum),
        rlim_max: maximum,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CPU, &limit) } != 0 {
        return Err(format!(
            "cannot set worker CPU limit: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
