//! Local synthetic worker execution and file consistency; not an OS sandbox.
use crate::{
    event_abi::{self, Event, Verdict, Window},
    worker_identity::{self, WorkerIdentity},
};
use std::time::Duration;

/// A captured local worker identity shared by all executions in an operation.
pub struct LocalEventExecutor {
    pub(crate) identity: WorkerIdentity,
}

impl LocalEventExecutor {
    pub fn capture(path: &str) -> Result<Self, String> {
        Ok(Self {
            identity: worker_identity::capture(path)?,
        })
    }

    pub fn execute(
        &self,
        artifact: &[u8],
        window: &Window,
        event: &Event,
        timeout: Duration,
    ) -> Result<Verdict, String> {
        self.verify_unchanged()?;
        event_abi::supervise_event(&self.identity.path, artifact, window, event, timeout)
    }

    pub fn verify_unchanged(&self) -> Result<(), String> {
        if worker_identity::capture(&self.identity.path)? != self.identity {
            return Err("worker changed during execution".into());
        }
        Ok(())
    }
}

/// Runs only built-in synthetic artifacts through the experimental container bridge.
/// Its reports do not authenticate Docker or authorize promotion.
#[cfg(unix)]
pub struct SyntheticContainerExecutor {
    python: WorkerIdentity,
    bridge: WorkerIdentity,
    lifecycle: WorkerIdentity,
    docker: WorkerIdentity,
    image: String,
    host: String,
    journal: std::path::PathBuf,
}

#[cfg(unix)]
pub struct SyntheticContainerResult {
    pub verdict: Verdict,
    pub receipt_path: std::path::PathBuf,
    pub receipt_json: String,
}

/// Check retained receipt bytes against the exact fixed-artifact request and verdict.
/// This verifies consistency, not historical permissions or execution authenticity.
#[cfg(unix)]
pub(crate) fn verify_captured_receipt(
    raw: &str,
    path: &str,
    artifact: &[u8],
    window: &Window,
    event: &Event,
    verdict: Verdict,
    image: &str,
    host: &str,
) -> Result<(String, String, String), String> {
    use sha2::{Digest, Sha256};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Receipt {
        schema: String,
        name: String,
        image: String,
        state: String,
        daemon_id: String,
        docker_host: String,
        candidate: String,
        artifact_sha256: String,
        request_sha256: String,
        container_id: String,
        result_status: String,
        output_sha256: String,
    }
    let hex = |s: &str, n: usize| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if raw.len() > 4096 {
        return Err("captured receipt too large".into());
    }
    let r: Receipt =
        serde_json::from_str(raw).map_err(|e| format!("invalid captured receipt: {e}"))?;
    let name_suffix = r
        .name
        .strip_prefix("sese-synthetic-")
        .ok_or("invalid receipt name")?;
    let path = std::path::Path::new(path);
    let name_file = format!("{}.json", r.name);
    let candidate = crate::builtin_event::NAMES
        .iter()
        .copied()
        .find(|name| crate::builtin_event::artifact(name).is_ok_and(|a| a == artifact))
        .ok_or("unknown synthetic artifact")?;
    let request = event_abi::encode_request(artifact, window, event)?;
    let mut output = Vec::with_capacity(16);
    output.extend_from_slice(&crate::event_replay::code(verdict.decision).to_le_bytes());
    output.extend_from_slice(&verdict.score.to_le_bytes());
    output.extend_from_slice(&verdict.reason.to_le_bytes());
    if !hex(name_suffix, 32)
        || !path.is_absolute()
        || path.file_name().and_then(|s| s.to_str()) != Some(name_file.as_str())
        || !image.strip_prefix("sha256:").is_some_and(|s| hex(s, 64))
        || !host.starts_with("unix:///")
        || host.len() > 1024
        || host
            .bytes()
            .any(|b| matches!(b, 0 | b'\n' | b'\r' | b'?' | b'#' | b'%'))
        || r.schema != "sese-synthetic-container-intent-v4"
        || r.state != "removed"
        || r.result_status != "valid_verdict"
        || r.image != image
        || r.docker_host != host
        || r.candidate != candidate
        || r.daemon_id.is_empty()
        || r.daemon_id.len() > 128
        || !r
            .daemon_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":_.-".contains(&b))
        || !hex(&r.container_id, 64)
        || r.artifact_sha256 != format!("{:x}", Sha256::digest(artifact))
        || r.request_sha256 != format!("{:x}", Sha256::digest(&request))
        || r.output_sha256 != format!("{:x}", Sha256::digest(&output))
    {
        return Err("captured container receipt mismatch".into());
    }
    Ok((r.name, r.container_id, r.daemon_id))
}

#[cfg(unix)]
impl SyntheticContainerExecutor {
    pub(crate) fn image(&self) -> &str {
        &self.image
    }
    pub(crate) fn host(&self) -> &str {
        &self.host
    }
    pub fn new(
        python: &str,
        bridge: &str,
        docker: &str,
        image: &str,
        host: &str,
        journal: &str,
    ) -> Result<Self, String> {
        for path in [python, bridge, docker] {
            if !std::path::Path::new(path).is_absolute() {
                return Err("container helper paths must be absolute".into());
            }
        }
        let hex = |s: &str, n: usize| {
            s.len() == n
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if !image.strip_prefix("sha256:").is_some_and(|s| hex(s, 64)) {
            return Err("invalid image digest".into());
        }
        if !host.starts_with("unix:///")
            || host.len() > 1024
            || host
                .bytes()
                .any(|b| matches!(b, 0 | b'\n' | b'\r' | b'?' | b'#' | b'%'))
        {
            return Err("invalid Docker endpoint".into());
        }
        let journal = std::fs::canonicalize(journal).map_err(|e| e.to_string())?;
        if !journal.is_dir() || journal.to_str().is_none() {
            return Err("invalid journal directory".into());
        }
        let bridge = worker_identity::capture(bridge)?;
        let lifecycle = std::path::Path::new(&bridge.path)
            .parent()
            .ok_or("invalid bridge path")?
            .join("synthetic_container_lifecycle.py");
        Ok(Self {
            python: worker_identity::capture(python)?,
            lifecycle: worker_identity::capture(
                lifecycle.to_str().ok_or("invalid lifecycle path")?,
            )?,
            bridge,
            docker: worker_identity::capture(docker)?,
            image: image.into(),
            host: host.into(),
            journal,
        })
    }

    pub(crate) fn verify_helpers(&self) -> Result<(), String> {
        for identity in [&self.python, &self.bridge, &self.lifecycle, &self.docker] {
            if worker_identity::capture(&identity.path)? != *identity {
                return Err("container helper changed".into());
            }
        }
        Ok(())
    }

    pub fn execute(
        &self,
        artifact: &[u8],
        window: &Window,
        event: &Event,
        timeout: Duration,
    ) -> Result<SyntheticContainerResult, String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let candidate = crate::builtin_event::NAMES
            .iter()
            .copied()
            .find(|name| crate::builtin_event::artifact(name).is_ok_and(|v| v == artifact))
            .ok_or("unknown synthetic artifact")?;
        if timeout.is_zero() || timeout > Duration::from_secs(120) {
            return Err("invalid container deadline".into());
        }
        let request = event_abi::encode_request(artifact, window, event)?;
        self.verify_helpers()?;
        let mut random = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|e| e.to_string())?;
        let name = format!(
            "sese-synthetic-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let path = self.journal.join(format!("{name}.json"));
        let journal = self.journal.to_str().ok_or("invalid journal path")?;
        let inner_timeout = timeout.as_secs_f64().min(60.0).to_string();
        let args = [
            "-B",
            self.bridge.path.as_str(),
            self.image.as_str(),
            self.host.as_str(),
            journal,
            self.docker.path.as_str(),
            name.as_str(),
            inner_timeout.as_str(),
        ];
        let output = crate::supervise_request_with_args(
            &self.python.path,
            &args,
            request.clone(),
            timeout,
            16,
        )
        .map_err(|e| format!("{e}; receipt_path={}", path.display()))?;
        self.verify_helpers()
            .map_err(|e| format!("{e}; receipt_path={}", path.display()))?;
        let verdict = event_abi::decode_verdict(&output)
            .map_err(|e| format!("{e}; receipt_path={}", path.display()))?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Receipt {
            schema: String,
            name: String,
            image: String,
            state: String,
            daemon_id: String,
            docker_host: String,
            candidate: String,
            artifact_sha256: String,
            request_sha256: String,
            container_id: String,
            result_status: String,
            output_sha256: String,
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| {
                format!(
                    "invalid container receipt: {e}; receipt_path={}",
                    path.display()
                )
            })?;
        let meta = file
            .metadata()
            .map_err(|e| format!("{e}; receipt_path={}", path.display()))?;
        if !meta.is_file()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o077 != 0
            || meta.len() > 4096
        {
            return Err(format!(
                "invalid container receipt metadata; receipt_path={}",
                path.display()
            ));
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{e}; receipt_path={}", path.display()))?;
        if bytes.len() > 4096 {
            return Err(format!(
                "container receipt too large; receipt_path={}",
                path.display()
            ));
        }
        let r: Receipt = serde_json::from_slice(&bytes).map_err(|e| {
            format!(
                "invalid container receipt: {e}; receipt_path={}",
                path.display()
            )
        })?;
        let hex64 = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if r.schema != "sese-synthetic-container-intent-v4"
            || r.name != name
            || r.image != self.image
            || r.docker_host != self.host
            || r.state != "removed"
            || r.result_status != "valid_verdict"
            || r.candidate != candidate
            || r.daemon_id.is_empty()
            || r.daemon_id.len() > 128
            || !r
                .daemon_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":_.-".contains(&b))
            || !hex64(&r.container_id)
            || r.artifact_sha256 != format!("{:x}", Sha256::digest(artifact))
            || r.request_sha256 != format!("{:x}", Sha256::digest(&request))
            || r.output_sha256 != format!("{:x}", Sha256::digest(&output))
        {
            return Err(format!(
                "container receipt mismatch; receipt_path={}",
                path.display()
            ));
        }
        let receipt_json = String::from_utf8(bytes)
            .map_err(|_| format!("invalid receipt UTF-8; receipt_path={}", path.display()))?;
        Ok(SyntheticContainerResult {
            verdict,
            receipt_path: path,
            receipt_json,
        })
    }
}
