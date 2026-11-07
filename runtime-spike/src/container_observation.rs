//! Experimental fixed-artifact comparison using an existing host observation's exact inputs.
use crate::{
    event_abi::{Decision, Event, Verdict, Window},
    event_executor::SyntheticContainerExecutor,
    storage::Storage,
};
use serde_json::Value;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct ContainerObservationReport {
    pub seq: i64,
}
fn number(v: &Value, key: &str) -> Result<u64, String> {
    v[key].as_u64().ok_or_else(|| format!("invalid {key}"))
}
fn small(v: &Value, key: &str) -> Result<u32, String> {
    u32::try_from(number(v, key)?).map_err(|e| e.to_string())
}
fn float(v: &Value, key: &str) -> Result<f64, String> {
    let f = v[key].as_f64().ok_or_else(|| format!("invalid {key}"))?;
    if !f.is_finite() {
        return Err("invalid float".into());
    }
    Ok(f)
}
fn bits(v: &Value) -> Result<f64, String> {
    let s = v.as_str().ok_or("invalid score bits")?;
    if s.len() != 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid score bits".into());
    }
    let f = f64::from_bits(u64::from_str_radix(s, 16).map_err(|e| e.to_string())?);
    if !f.is_finite() {
        return Err("invalid score".into());
    }
    Ok(f)
}
fn now() -> Result<String, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string())
}

pub(crate) fn decode_source(source: &Value) -> Result<(Window, Event), String> {
    let p = &source["prior"];
    let e = &source["event"];
    if p["state_semantics"] != "measured-v2" {
        return Err("source requires measured-v2 state".into());
    }
    Ok((
        Window {
            count: number(p, "count")?,
            mean: float(p, "mean")?,
            p99_us: number(p, "p99_us")?,
            last_error: small(p, "last_error")?,
        },
        Event {
            timestamp_ns: number(e, "timestamp_ns")?,
            entity_id: number(e, "entity_id")?,
            metric: float(e, "metric")?,
            flags: small(e, "flags")?,
        },
    ))
}

/// Run two fixed synthetic artifacts with exactly the inputs of a verified app observation.
/// Failed attempts are separately recorded when the ledger guard remains current.
pub fn compare_container_observation(
    db: &mut Storage,
    executor: &SyntheticContainerExecutor,
    domain: &str,
    observation: i64,
    timeout: Duration,
) -> Result<ContainerObservationReport, String> {
    if timeout.is_zero() || timeout > Duration::from_secs(120) {
        return Err("invalid container deadline".into());
    }
    let (source, checkpoint, generation) =
        db.load_container_observation_source(domain, observation)?;
    let (prior, event) = decode_source(&source)?;
    let champion = source["champion_id"].as_str().ok_or("invalid champion")?;
    let shadow = source["shadow_id"].as_str().ok_or("invalid shadow")?;
    let champion_artifact = db.artifact_for_abi(champion, 3)?;
    let shadow_artifact = db.artifact_for_abi(shadow, 3)?;
    if !crate::container_shadow::fixed_artifact(&champion_artifact)
        || !crate::container_shadow::fixed_artifact(&shadow_artifact)
    {
        return Err("unknown synthetic artifact".into());
    }
    let started = now()?;
    let start = Instant::now();
    let c = match executor.execute(&champion_artifact, &prior, &event, timeout) {
        Ok(result) => result,
        Err(error) => {
            let elapsed = u64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
            executor
                .verify_helpers()
                .map_err(|e| format!("{e}; champion_error={error}"))?;
            let finished = now()?;
            let payload = serde_json::json!({"schema":"sese-container-observation-failure-v1","status":"failed","failure_stage":"champion","error":error,"domain":domain,"observation_id":observation,"champion_id":champion,"champion_generation":generation,"shadow_id":shadow,"prior":source["prior"],"event":source["event"],"champion_us":elapsed,"shadow_us":null,"champion":null,"shadow":null,"image":executor.image(),"docker_host":executor.host(),"started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
            let seq = db
                .save_container_observation_experiment(
                    &payload,
                    (checkpoint.0, &checkpoint.1),
                    &finished,
                )
                .map_err(|e| format!("{e}; champion_error={error}"))?;
            return Err(format!("container_event_seq={seq}; champion_error={error}"));
        }
    };
    let champion_us = u64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let s = match executor.execute(&shadow_artifact, &prior, &event, timeout) {
        Ok(result) => result,
        Err(error) => {
            let shadow_us =
                u64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
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
            let payload = serde_json::json!({"schema":"sese-container-observation-failure-v1","status":"failed","failure_stage":"shadow","error":error,"domain":domain,"observation_id":observation,"champion_id":champion,"champion_generation":generation,"shadow_id":shadow,"prior":source["prior"],"event":source["event"],"champion_us":champion_us,"shadow_us":shadow_us,"champion":crate::container_shadow::record(c.verdict,&c),"shadow":null,"image":executor.image(),"docker_host":executor.host(),"started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
            let seq = db
                .save_container_observation_experiment(
                    &payload,
                    (checkpoint.0, &checkpoint.1),
                    &finished,
                )
                .map_err(|e| format!("{e}; {}", context()))?;
            return Err(format!("container_event_seq={seq}; {}", context()));
        }
    };
    let shadow_us = u64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
    let paths = || {
        format!(
            "champion_receipt_path={}; shadow_receipt_path={}",
            c.receipt_path.display(),
            s.receipt_path.display()
        )
    };
    executor
        .verify_helpers()
        .map_err(|e| format!("{e}; {}", paths()))?;
    let finished = now()?;
    let payload = serde_json::json!({"schema":"sese-container-observation-experiment-v1","domain":domain,"observation_id":observation,"champion_id":champion,"champion_generation":generation,"shadow_id":shadow,"prior":source["prior"],"event":source["event"],"champion_us":champion_us,"shadow_us":shadow_us,"champion":crate::container_shadow::record(c.verdict,&c),"shadow":crate::container_shadow::record(s.verdict,&s),"image":executor.image(),"docker_host":executor.host(),"started_unix_ns":started,"finished_unix_ns":finished,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false});
    let seq = db
        .save_container_observation_experiment(&payload, (checkpoint.0, &checkpoint.1), &finished)
        .map_err(|e| format!("{e}; {}", paths()))?;
    Ok(ContainerObservationReport { seq })
}

pub(crate) fn validate(v: &Value, source: &Value) -> Result<(), String> {
    let failed = v["schema"] == "sese-container-observation-failure-v1";
    let champion_failure = failed && v["failure_stage"] == "champion";
    let shadow_failure = failed && v["failure_stage"] == "shadow";
    if !(v["schema"] == "sese-container-observation-experiment-v1" || failed)
        || v["synthetic"] != true
        || v["execution_authenticated"] != false
        || v["promotion_evidence"] != false
        || v["champion_generation"].as_i64().is_none()
        || v["champion_us"].as_u64().is_none()
    {
        return Err("invalid container observation provenance".into());
    }
    if failed {
        if v["status"] != "failed"
            || !(champion_failure || shadow_failure)
            || !v["shadow"].is_null()
            || v["error"]
                .as_str()
                .is_none_or(|s| s.is_empty() || s.len() > 8192)
            || (champion_failure && (!v["champion"].is_null() || !v["shadow_us"].is_null()))
            || (shadow_failure && (!v["champion"].is_object() || v["shadow_us"].as_u64().is_none()))
        {
            return Err("invalid container observation failure".into());
        }
    } else if v["shadow_us"].as_u64().is_none() {
        return Err("invalid container observation duration".into());
    }
    for field in [
        "domain",
        "observation_id",
        "champion_id",
        "shadow_id",
        "prior",
        "event",
    ] {
        if v[field] != source[field] {
            return Err(format!("container observation source mismatch: {field}"));
        }
    }
    let (prior, event) = decode_source(source)?;
    let image = v["image"].as_str().ok_or("missing image")?;
    let host = v["docker_host"].as_str().ok_or("missing endpoint")?;
    let hex = |s: &str, n: usize| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if !image.strip_prefix("sha256:").is_some_and(|s| hex(s, 64))
        || !host.starts_with("unix:///")
        || host.len() > 1024
        || host
            .bytes()
            .any(|b| matches!(b, 0 | b'\n' | b'\r' | b'?' | b'#' | b'%'))
    {
        return Err("invalid container endpoint or image".into());
    }
    for role in ["champion", "shadow"] {
        let id = v[format!("{role}_id")]
            .as_str()
            .ok_or("invalid candidate")?;
        if !crate::builtin_event::NAMES.iter().copied().any(|name| {
            crate::builtin_event::artifact(name).is_ok_and(|a| {
                use sha2::{Digest, Sha256};
                format!("{:x}", Sha256::digest(a)) == id
            })
        }) {
            return Err("unknown synthetic artifact".into());
        }
    }
    if champion_failure {
        return Ok(());
    }
    let mut seen_name = String::new();
    let mut seen_container = String::new();
    let mut seen_daemon = String::new();
    let roles: &[&str] = if shadow_failure {
        &["champion"]
    } else {
        &["champion", "shadow"]
    };
    for &role in roles {
        let id = v[format!("{role}_id")]
            .as_str()
            .ok_or("invalid candidate")?;
        let artifact = crate::builtin_event::NAMES
            .iter()
            .copied()
            .filter_map(|name| crate::builtin_event::artifact(name).ok())
            .find(|a| {
                use sha2::{Digest, Sha256};
                format!("{:x}", Sha256::digest(a)) == id
            })
            .ok_or("unknown synthetic artifact")?;
        let entry = &v[role];
        let actual = &entry["verdict"];
        let decision = match number(actual, "decision")? {
            0 => Decision::Allow,
            1 => Decision::Throttle,
            2 => Decision::Quarantine,
            3 => Decision::Escalate,
            _ => return Err("invalid verdict".into()),
        };
        let verdict = Verdict {
            decision,
            score: bits(&actual["score_bits"])?,
            reason: small(actual, "reason")?,
        };
        let (name, container, daemon) = crate::event_executor::verify_captured_receipt(
            entry["receipt"].as_str().ok_or("missing receipt")?,
            entry["receipt_path"]
                .as_str()
                .ok_or("missing receipt path")?,
            &artifact,
            &prior,
            &event,
            verdict,
            image,
            host,
        )?;
        if role == "champion" {
            seen_name = name;
            seen_container = container;
            seen_daemon = daemon;
        } else if name == seen_name || container == seen_container || daemon != seen_daemon {
            return Err("reused container or changed daemon".into());
        }
    }
    Ok(())
}
