//! Linux worker self-check. This is not attestation or a complete sandbox.
#[cfg(target_os = "linux")]
pub fn verify() -> Result<(), String> {
    fn read(path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|_| format!("isolation check unavailable: {path}"))
    }
    fn field<'a>(status: &'a str, key: &str) -> Option<&'a str> {
        let mut values = status
            .lines()
            .filter_map(|line| line.strip_prefix(key).map(str::trim));
        let value = values.next()?;
        if values.next().is_some() {
            return None;
        }
        Some(value)
    }
    if unsafe { libc::geteuid() } != 65534 || unsafe { libc::getegid() } != 65534 {
        return Err("isolation user mismatch".into());
    }
    let status = read("/proc/self/status")?;
    let identity = |key: &str| {
        field(&status, key).map(|value| value.split_whitespace().collect::<Vec<_>>())
            == Some(vec!["65534", "65534", "65534", "65534"])
    };
    let groups =
        field(&status, "Groups:").map(|value| value.split_whitespace().collect::<Vec<_>>());
    if !identity("Uid:") || !identity("Gid:") || groups != Some(vec!["65534"]) {
        return Err("isolation process identity mismatch".into());
    }
    if ["CapInh:", "CapPrm:", "CapEff:", "CapBnd:", "CapAmb:"]
        .iter()
        .any(|key| field(&status, key) != Some("0000000000000000"))
        || field(&status, "NoNewPrivs:") != Some("1")
        || field(&status, "Seccomp:") != Some("2")
    {
        return Err("isolation process restrictions missing".into());
    }
    if read("/sys/fs/cgroup/memory.max")?.trim() != "134217728"
        || read("/sys/fs/cgroup/memory.swap.max")?.trim() != "0"
        || read("/sys/fs/cgroup/pids.max")?.trim() != "16"
        || read("/sys/fs/cgroup/cpu.max")?
            .split_whitespace()
            .collect::<Vec<_>>()
            != ["100000", "100000"]
    {
        return Err("isolation resource limits missing".into());
    }
    let mounts = read("/proc/self/mountinfo")?;
    let root: Vec<_> = mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let mountpoint = fields.nth(4)?;
            let options = fields.next()?;
            (mountpoint == "/").then_some(options)
        })
        .collect();
    if root.len() != 1 || !root[0].split(',').any(|v| v == "ro") {
        return Err("isolation root filesystem is writable or unknown".into());
    }
    let interfaces =
        std::fs::read_dir("/sys/class/net").map_err(|_| "isolation network check unavailable")?;
    let mut saw_loopback = false;
    for interface in interfaces {
        let interface = interface.map_err(|_| "isolation network check failed")?;
        if interface
            .file_type()
            .map_err(|_| "isolation network check failed")?
            .is_file()
        {
            continue;
        }
        let flags = std::fs::read_to_string(interface.path().join("flags"))
            .map_err(|_| "isolation network flags unavailable")?;
        let flags = u32::from_str_radix(
            flags
                .trim()
                .strip_prefix("0x")
                .ok_or("invalid network flags")?,
            16,
        )
        .map_err(|_| "invalid network flags")?;
        if interface.file_name() == "lo" {
            saw_loopback = true;
        } else if flags & 1 != 0 {
            return Err("isolation active network interface present".into());
        }
    }
    if !saw_loopback
        || read("/proc/net/route")?
            .lines()
            .skip(1)
            .any(|line| !line.trim().is_empty())
    {
        return Err("isolation network route present or unknown".into());
    }
    match std::fs::symlink_metadata("/var/run/docker.sock") {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err("isolation Docker socket present or unknown".into()),
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn verify() -> Result<(), String> {
    Err("isolation self-check requires Linux".into())
}
