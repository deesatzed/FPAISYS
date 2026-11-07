//! Worker file provenance; not executable attestation or an isolation boundary.
use sha2::{Digest, Sha256};
use std::io::Read;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkerIdentity {
    pub path: String,
    pub sha256: String,
}
pub(crate) fn capture(path: &str) -> Result<WorkerIdentity, String> {
    let canonical = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let path = canonical.to_str().ok_or("non-UTF8 worker path")?.to_owned();
    let file = std::fs::File::open(&canonical).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 134_217_728 {
        return Err("invalid worker file".into());
    }
    let mut reader = file.take(134_217_729);
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0usize;
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n;
        if total > 134_217_728 {
            return Err("worker file too large".into());
        }
        hash.update(&buffer[..n]);
    }
    Ok(WorkerIdentity {
        path,
        sha256: format!("{:x}", hash.finalize()),
    })
}
