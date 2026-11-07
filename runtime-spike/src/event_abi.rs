//! Experimental core-Wasm ABI inspired by the WIT sketch, not a Component Model implementation.
//! In-process execution is for fixed synthetic modules only, not untrusted candidates.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub count: u64,
    pub mean: f64,
    pub p99_us: u64,
    pub last_error: u32,
}
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub timestamp_ns: u64,
    pub entity_id: u64,
    pub metric: f64,
    pub flags: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Throttle,
    Quarantine,
    Escalate,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    pub decision: Decision,
    pub score: f64,
    pub reason: u32,
}
/// Flattened core-Wasm signature: (u64,f64,u64,u32,u64,u64,f64,u32) -> (u32,f64,u32).
pub fn evaluate_event(bytes: &[u8], window: &Window, event: &Event) -> Result<Verdict, String> {
    if !window.mean.is_finite() || !event.metric.is_finite() {
        return Err("nonfinite input".into());
    }
    let (mut store, instance) = crate::instantiate(bytes)?;
    let function = instance
        .get_typed_func::<(u64, f64, u64, u32, u64, u64, f64, u32), (u32, f64, u32)>(
            &store,
            "evaluate_event_v1",
        )
        .map_err(|e| e.to_string())?;
    let (code, score, reason) = function
        .call(
            &mut store,
            (
                window.count,
                window.mean,
                window.p99_us,
                window.last_error,
                event.timestamp_ns,
                event.entity_id,
                event.metric,
                event.flags,
            ),
        )
        .map_err(|e| e.to_string())?;
    let decision = match code {
        0 => Decision::Allow,
        1 => Decision::Throttle,
        2 => Decision::Quarantine,
        3 => Decision::Escalate,
        _ => return Err("invalid decision code".into()),
    };
    if !score.is_finite() {
        return Err("nonfinite score".into());
    }
    Ok(Verdict {
        decision,
        score,
        reason,
    })
}

/// Supervised synthetic event evaluation; no OS isolation is provided.
pub fn supervise_event(
    worker: &str,
    bytes: &[u8],
    window: &Window,
    event: &Event,
    timeout: std::time::Duration,
) -> Result<Verdict, String> {
    let data = encode_request(bytes, window, event)?;
    let output = crate::supervise_request(worker, data, timeout, 16)?;
    decode_verdict(&output)
}

pub(crate) fn encode_request(
    bytes: &[u8],
    window: &Window,
    event: &Event,
) -> Result<Vec<u8>, String> {
    if bytes.len() > 65_536 || !window.mean.is_finite() || !event.metric.is_finite() {
        return Err("invalid event request".into());
    }
    let mut data = Vec::with_capacity(56 + bytes.len());
    data.extend_from_slice(&window.count.to_le_bytes());
    data.extend_from_slice(&window.mean.to_le_bytes());
    data.extend_from_slice(&window.p99_us.to_le_bytes());
    data.extend_from_slice(&window.last_error.to_le_bytes());
    data.extend_from_slice(&event.timestamp_ns.to_le_bytes());
    data.extend_from_slice(&event.entity_id.to_le_bytes());
    data.extend_from_slice(&event.metric.to_le_bytes());
    data.extend_from_slice(&event.flags.to_le_bytes());
    data.extend_from_slice(bytes);
    Ok(data)
}

pub(crate) fn decode_verdict(output: &[u8]) -> Result<Verdict, String> {
    if output.len() != 16 {
        return Err("invalid worker response".into());
    }
    let code = u32::from_le_bytes(output[0..4].try_into().map_err(|_| "invalid code")?);
    let score = f64::from_le_bytes(output[4..12].try_into().map_err(|_| "invalid score")?);
    let reason = u32::from_le_bytes(output[12..16].try_into().map_err(|_| "invalid reason")?);
    let decision = match code {
        0 => Decision::Allow,
        1 => Decision::Throttle,
        2 => Decision::Quarantine,
        3 => Decision::Escalate,
        _ => return Err("invalid decision code".into()),
    };
    if !score.is_finite() {
        return Err("nonfinite score".into());
    }
    Ok(Verdict {
        decision,
        score,
        reason,
    })
}
