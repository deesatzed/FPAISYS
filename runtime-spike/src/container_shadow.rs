//! Separate fixed-artifact experiment; records are not authenticated or promotion evidence.
use crate::{
    event_abi::{Event, Verdict, Window},
    event_executor::SyntheticContainerExecutor,
    event_state::EventState,
    storage::Storage,
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct ContainerShadowHost {
    domain: String,
    stream: String,
    state: EventState,
}
pub struct ContainerShadowObservation {
    pub seq: i64,
    pub prior: Window,
    pub champion: Verdict,
    pub shadow: Verdict,
}

pub(crate) fn fixed_artifact(bytes: &[u8]) -> bool {
    crate::builtin_event::NAMES
        .iter()
        .copied()
        .any(|name| crate::builtin_event::artifact(name).is_ok_and(|v| v == bytes))
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn now() -> Result<String, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string())
}
pub(crate) fn record(
    v: Verdict,
    receipt: &crate::event_executor::SyntheticContainerResult,
) -> serde_json::Value {
    serde_json::json!({"verdict":{"decision":crate::event_replay::code(v.decision),"score_bits":format!("{:016x}",v.score.to_bits()),"reason":v.reason},"receipt":receipt.receipt_json,"receipt_path":receipt.receipt_path.to_string_lossy()})
}

impl ContainerShadowHost {
    pub fn open(
        db: &mut Storage,
        domain: &str,
        stream: &str,
        capacity: usize,
    ) -> Result<Self, String> {
        let (state, _, _, _) = db.load_container_shadow_stream(domain, stream, capacity)?;
        Ok(Self {
            domain: domain.into(),
            stream: stream.into(),
            state,
        })
    }
    pub fn snapshot(&self) -> Window {
        self.state.snapshot()
    }
    pub fn process(
        &mut self,
        db: &mut Storage,
        executor: &SyntheticContainerExecutor,
        expected: &str,
        shadow: &str,
        event: &Event,
        timeout: Duration,
    ) -> Result<ContainerShadowObservation, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(120) {
            return Err("invalid container deadline".into());
        }
        let (stored, checkpoint, champion, generation) =
            db.load_container_shadow_stream(&self.domain, &self.stream, self.state.capacity())?;
        if stored.encode() != self.state.encode() {
            return Err("stale container shadow stream".into());
        }
        if champion != expected {
            return Err("unexpected domain champion".into());
        }
        let champion_artifact = db.artifact_for_abi(&champion, 3)?;
        let shadow_artifact = db.artifact_for_abi(shadow, 3)?;
        if !fixed_artifact(&champion_artifact) || !fixed_artifact(&shadow_artifact) {
            return Err("unknown synthetic artifact".into());
        }
        let mut next = self.state.clone();
        // Validate the input before launching either container.
        next.record(event.metric, 0, 0)?;
        let prior = self.state.snapshot();
        let started = now()?;
        let start = Instant::now();
        let c = match executor.execute(&champion_artifact, &prior, event, timeout) {
            Ok(result) => result,
            Err(error) => {
                let elapsed =
                    u64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
                executor
                    .verify_helpers()
                    .map_err(|e| format!("{e}; champion_error={error}"))?;
                let finished = now()?;
                next = self.state.clone();
                next.record(event.metric, elapsed, 500)?;
                let payload = serde_json::json!({"schema":"sese-container-shadow-failure-v1", "status":"failed", "failure_stage":"champion", "error":error, "domain":self.domain,"stream":self.stream,"capacity":self.state.capacity(),"champion_id":champion,"champion_generation":generation,"shadow_id":shadow,"event":{"timestamp_ns":event.timestamp_ns,"entity_id":event.entity_id,"metric_bits":format!("{:016x}",event.metric.to_bits()),"flags":event.flags},"champion_us":elapsed,"shadow_us":null,"prior_state":self.state.encode(),"next_state":next.encode(),"champion":null,"shadow":null,"image":executor.image(),"docker_host":executor.host(),"started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
                let seq = db
                    .save_container_shadow_experiment(
                        &payload,
                        &self.state,
                        (checkpoint.0, &checkpoint.1),
                        &champion,
                        generation,
                        &finished,
                    )
                    .map_err(|e| format!("{e}; champion_error={error}"))?;
                self.state = next;
                return Err(format!("container_event_seq={seq}; champion_error={error}"));
            }
        };
        let elapsed = u64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
        let shadow_start = Instant::now();
        let s = match executor.execute(&shadow_artifact, &prior, event, timeout) {
            Ok(result) => result,
            Err(error) => {
                let shadow_elapsed =
                    u64::try_from(shadow_start.elapsed().as_micros()).map_err(|e| e.to_string())?;
                let context = || {
                    format!(
                        "shadow_error={error}; champion_receipt_path={}",
                        c.receipt_path.display()
                    )
                };
                executor
                    .verify_helpers()
                    .map_err(|e| format!("{e}; {}", context()))?;
                let finished = now()?;
                next = self.state.clone();
                next.record(event.metric, elapsed, 0)?;
                let payload = serde_json::json!({"schema":"sese-container-shadow-failure-v1", "status":"failed", "failure_stage":"shadow", "error":error, "domain":self.domain,"stream":self.stream,"capacity":self.state.capacity(),"champion_id":champion,"champion_generation":generation,"shadow_id":shadow,"event":{"timestamp_ns":event.timestamp_ns,"entity_id":event.entity_id,"metric_bits":format!("{:016x}",event.metric.to_bits()),"flags":event.flags},"champion_us":elapsed,"shadow_us":shadow_elapsed,"prior_state":self.state.encode(),"next_state":next.encode(),"champion":record(c.verdict,&c),"shadow":null,"image":executor.image(),"docker_host":executor.host(),"started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
                let seq = db
                    .save_container_shadow_experiment(
                        &payload,
                        &self.state,
                        (checkpoint.0, &checkpoint.1),
                        &champion,
                        generation,
                        &finished,
                    )
                    .map_err(|e| format!("{e}; {}", context()))?;
                self.state = next;
                return Err(format!("container_event_seq={seq}; {}", context()));
            }
        };
        let shadow_elapsed =
            u64::try_from(shadow_start.elapsed().as_micros()).map_err(|e| e.to_string())?;
        let receipt_paths = || {
            format!(
                "champion_receipt_path={}; shadow_receipt_path={}",
                c.receipt_path.display(),
                s.receipt_path.display()
            )
        };
        executor
            .verify_helpers()
            .map_err(|e| format!("{e}; {}", receipt_paths()))?;
        next = self.state.clone();
        next.record(event.metric, elapsed, 0)?;
        let finished = now()?;
        let payload = serde_json::json!({"schema":"sese-container-shadow-experiment-v1", "domain":self.domain,"stream":self.stream,"capacity":self.state.capacity(),"champion_id":champion,"champion_generation":generation,"shadow_id":shadow,"event":{"timestamp_ns":event.timestamp_ns,"entity_id":event.entity_id,"metric_bits":format!("{:016x}",event.metric.to_bits()),"flags":event.flags},"champion_us":elapsed,"shadow_us":shadow_elapsed,"prior_state":self.state.encode(),"next_state":next.encode(),"champion":record(c.verdict,&c),"shadow":record(s.verdict,&s),"image":executor.image(),"docker_host":executor.host(),"started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
        let seq = db
            .save_container_shadow_experiment(
                &payload,
                &self.state,
                (checkpoint.0, &checkpoint.1),
                &champion,
                generation,
                &finished,
            )
            .map_err(|e| format!("{e}; {}", receipt_paths()))?;
        self.state = next;
        Ok(ContainerShadowObservation {
            seq,
            prior,
            champion: c.verdict,
            shadow: s.verdict,
        })
    }
}

fn bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    value
        .as_array()
        .ok_or("invalid state")?
        .iter()
        .map(|x| {
            x.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| "invalid state byte".into())
        })
        .collect()
}
fn bits(value: &serde_json::Value) -> Result<f64, String> {
    let s = value.as_str().ok_or("invalid float bits")?;
    if s.len() != 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid float bits".into());
    }
    let f = f64::from_bits(u64::from_str_radix(s, 16).map_err(|e| e.to_string())?);
    if !f.is_finite() {
        return Err("nonfinite float".into());
    }
    Ok(f)
}
fn u32_value(v: &serde_json::Value) -> Result<u32, String> {
    u32::try_from(v.as_u64().ok_or("invalid integer")?).map_err(|e| e.to_string())
}

/// Reconstruct state and request/receipt digests; this is consistency, not authentication.
pub(crate) fn validate(v: &serde_json::Value, prior: &EventState) -> Result<EventState, String> {
    let failed = v["schema"] == "sese-container-shadow-failure-v1";
    let champion_failure = failed && v["failure_stage"] == "champion";
    let shadow_failure = failed && v["failure_stage"] == "shadow";
    if !(v["schema"] == "sese-container-shadow-experiment-v1" || failed)
        || v["synthetic"] != true
        || v["execution_authenticated"] != false
        || v["promotion_evidence"] != false
        || v["capacity"].as_u64() != Some(prior.capacity() as u64)
        || bytes(&v["prior_state"])? != prior.encode()
    {
        return Err("invalid container shadow provenance or prior state".into());
    }
    let e = &v["event"];
    let event = Event {
        timestamp_ns: e["timestamp_ns"].as_u64().ok_or("invalid timestamp")?,
        entity_id: e["entity_id"].as_u64().ok_or("invalid entity")?,
        metric: bits(&e["metric_bits"])?,
        flags: u32_value(&e["flags"])?,
    };
    let mut next = prior.clone();
    if failed {
        let error = v["error"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or("invalid container failure")?;
        if v["status"] != "failed"
            || !(champion_failure || shadow_failure)
            || !v["shadow"].is_null()
            || error.is_empty()
            || (champion_failure && (!v["champion"].is_null() || !v["shadow_us"].is_null()))
            || (shadow_failure && (!v["champion"].is_object() || v["shadow_us"].as_u64().is_none()))
        {
            return Err("invalid container failure".into());
        }
        for role in ["champion", "shadow"] {
            let id = v[format!("{role}_id")]
                .as_str()
                .ok_or("missing candidate")?;
            if !crate::builtin_event::NAMES.iter().copied().any(|name| {
                crate::builtin_event::artifact(name).is_ok_and(|artifact| hash(&artifact) == id)
            }) {
                return Err("invalid failure candidate".into());
            }
        }
    } else {
        v["shadow_us"].as_u64().ok_or("invalid shadow duration")?;
    }
    next.record(
        event.metric,
        v["champion_us"].as_u64().ok_or("invalid duration")?,
        if champion_failure { 500 } else { 0 },
    )?;
    if bytes(&v["next_state"])? != next.encode() {
        return Err("invalid container shadow state transition".into());
    }
    if champion_failure {
        return Ok(next);
    }
    let mut daemon = String::new();
    let mut container = String::new();
    let mut receipt_name = String::new();
    let roles: &[&str] = if shadow_failure {
        &["champion"]
    } else {
        &["champion", "shadow"]
    };
    for &role in roles {
        let id = v[format!("{role}_id")]
            .as_str()
            .ok_or("missing candidate")?;
        let entry = &v[role];
        let verdict = &entry["verdict"];
        let decision = u32_value(&verdict["decision"])?;
        if decision > 3 {
            return Err("invalid decision".into());
        }
        let score = bits(&verdict["score_bits"])?;
        let reason = u32_value(&verdict["reason"])?;
        let artifact = crate::builtin_event::NAMES
            .iter()
            .copied()
            .filter_map(|name| crate::builtin_event::artifact(name).ok())
            .find(|artifact| hash(artifact) == id)
            .ok_or("invalid shadow candidate")?;
        let decision = match decision {
            0 => crate::event_abi::Decision::Allow,
            1 => crate::event_abi::Decision::Throttle,
            2 => crate::event_abi::Decision::Quarantine,
            3 => crate::event_abi::Decision::Escalate,
            _ => unreachable!(),
        };
        let actual = Verdict {
            decision,
            score,
            reason,
        };
        let (name, c, d) = crate::event_executor::verify_captured_receipt(
            entry["receipt"].as_str().ok_or("missing receipt")?,
            entry["receipt_path"]
                .as_str()
                .ok_or("missing receipt path")?,
            &artifact,
            &prior.snapshot(),
            &event,
            actual,
            v["image"].as_str().ok_or("missing image")?,
            v["docker_host"].as_str().ok_or("missing endpoint")?,
        )?;
        if role == "champion" {
            daemon = d;
            container = c;
            receipt_name = name;
        } else if d != daemon || c == container || name == receipt_name {
            return Err("container shadow receipt collision or daemon mismatch".into());
        }
    }
    Ok(next)
}

#[cfg(test)]
mod captured_receipt_tests {
    use super::*;
    use crate::event_abi;

    #[test]
    fn shadow_rejects_unexpected_receipt_fields() {
        let prior = EventState::new(2).unwrap();
        let mut next = prior.clone();
        next.record(7.0, 1, 0).unwrap();
        let event = Event {
            timestamp_ns: 9,
            entity_id: 42,
            metric: 7.0,
            flags: 4,
        };
        let image = format!("sha256:{}", "0".repeat(64));
        let host = "unix:///tmp/example.sock";
        let make = |candidate: &str, decision: u32, name_byte: char| {
            let artifact = crate::builtin_event::artifact(candidate).unwrap();
            let request = event_abi::encode_request(&artifact, &prior.snapshot(), &event).unwrap();
            let mut output = Vec::new();
            output.extend_from_slice(&decision.to_le_bytes());
            output.extend_from_slice(&7.0f64.to_le_bytes());
            output.extend_from_slice(&4u32.to_le_bytes());
            let name = format!("sese-synthetic-{}", name_byte.to_string().repeat(32));
            let receipt = serde_json::json!({"schema":"sese-synthetic-container-intent-v4","name":name,"image":image,"state":"removed","daemon_id":"daemon","docker_host":host,"candidate":candidate,"artifact_sha256":hash(&artifact),"request_sha256":hash(&request),"container_id":name_byte.to_string().repeat(64),"result_status":"valid_verdict","output_sha256":hash(&output)});
            (
                hash(&artifact),
                serde_json::json!({"verdict":{"decision":decision,"score_bits":format!("{:016x}",7.0f64.to_bits()),"reason":4},"receipt":receipt.to_string(),"receipt_path":format!("/tmp/{name}.json")}),
            )
        };
        let (champion_id, champion) = make("allow", 0, 'a');
        let (shadow_id, shadow) = make("throttle", 1, 'b');
        let mut v = serde_json::json!({"schema":"sese-container-shadow-experiment-v1","synthetic":true,"execution_authenticated":false,"promotion_evidence":false,"capacity":2,"prior_state":prior.encode(),"next_state":next.encode(),"event":{"timestamp_ns":9,"entity_id":42,"metric_bits":format!("{:016x}",7.0f64.to_bits()),"flags":4},"champion_us":1,"shadow_us":1,"champion_id":champion_id,"shadow_id":shadow_id,"champion":champion,"shadow":shadow,"image":image,"docker_host":host});
        assert!(validate(&v, &prior).is_ok());
        let mut receipt: serde_json::Value =
            serde_json::from_str(v["champion"]["receipt"].as_str().unwrap()).unwrap();
        receipt["extra"] = serde_json::json!(true);
        v["champion"]["receipt"] = serde_json::json!(receipt.to_string());
        assert!(validate(&v, &prior).is_err());
    }
}
