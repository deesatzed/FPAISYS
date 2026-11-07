//! Advisory consistency comparison; it does not authenticate execution or authorize promotion.
use serde_json::Value;
use std::collections::HashSet;

fn bits(v: &Value) -> Result<f64, String> {
    let s = v.as_str().ok_or("invalid float bits")?;
    if s.len() != 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid float bits".into());
    }
    let f = f64::from_bits(u64::from_str_radix(s, 16).map_err(|e| e.to_string())?);
    if !f.is_finite() {
        return Err("invalid float".into());
    }
    Ok(f)
}
fn same_float(local: &Value, container: f64) -> bool {
    local
        .as_f64()
        .is_some_and(|v| v.to_bits() == container.to_bits())
}
fn verdict_matches(local: &Value, remote: &Value) -> Result<bool, String> {
    let code = match local["decision"].as_str() {
        Some("allow") => 0,
        Some("throttle") => 1,
        Some("quarantine") => 2,
        Some("escalate") => 3,
        _ => return Err("invalid local observation verdict".into()),
    };
    Ok(remote["decision"].as_u64() == Some(code)
        && local["reason"].as_u64() == remote["reason"].as_u64()
        && local["reason"].as_u64().is_some()
        && same_float(&local["score"], bits(&remote["score_bits"])?))
}

/// Inputs have already been validated against their ledger and captured receipt bindings.
pub(crate) fn compare(
    preview: &Value,
    replay: &Value,
    shadow: &Value,
    domain: &str,
    observation_id: i64,
    current: &(String, i64),
) -> Result<(), String> {
    let local = &preview["replay"];
    if replay["passed"] != true
        || local["passed"] != true
        || replay["candidate_id"] != local["candidate_id"]
        || replay["fixture_version"] != local["fixture_version"]
        || replay["fixture_digest"] != local["fixture_digest"]
        || replay["state_semantics"] != "measured-v2"
        || local["state_semantics"] != "measured-v2"
    {
        return Err("container replay does not match local review fixture".into());
    }
    let remote_cases = replay["cases"]
        .as_array()
        .ok_or("invalid container cases")?;
    let local_cases = local["cases"].as_array().ok_or("invalid local cases")?;
    if remote_cases.is_empty() || remote_cases.len() != local_cases.len() {
        return Err("container case count mismatch".into());
    }
    for (a, b) in local_cases.iter().zip(remote_cases) {
        for field in [
            "id", "window", "event", "expected", "actual", "error", "passed",
        ] {
            if a[field] != b[field] {
                return Err(format!("container replay case mismatch: {field}"));
            }
        }
    }
    let historical = shadow["schema"] == "sese-container-observation-experiment-v1";
    if !(shadow["schema"] == "sese-container-shadow-experiment-v1" || historical)
        || shadow["domain"].as_str() != Some(domain)
        || shadow["champion_id"].as_str() != Some(current.0.as_str())
        || shadow["champion_generation"].as_i64() != Some(current.1)
        || shadow["shadow_id"] != local["candidate_id"]
        || shadow["image"] != replay["image"]
        || shadow["docker_host"] != replay["docker_host"]
    {
        return Err("container shadow does not match current review scope".into());
    }
    let observations = preview["observations"]
        .as_array()
        .ok_or("invalid review observations")?;
    let mut matching = observations
        .iter()
        .filter(|o| o["id"].as_i64() == Some(observation_id));
    let observation = matching.next().ok_or("observation not found")?;
    if historical && shadow["observation_id"].as_i64() != Some(observation_id) {
        return Err("container source observation mismatch".into());
    }
    if matching.next().is_some()
        || observation["champion_id"] != shadow["champion_id"]
        || observation["shadow_id"] != shadow["shadow_id"]
        || !observation["champion_error"].is_null()
        || !observation["shadow_error"].is_null()
    {
        return Err("observation does not match container shadow".into());
    }
    let prior = if historical {
        crate::container_observation::decode_source(shadow)?.0
    } else {
        let state: Vec<u8> =
            serde_json::from_value(shadow["prior_state"].clone()).map_err(|e| e.to_string())?;
        crate::event_state::EventState::decode(&state)?.snapshot()
    };
    let p = &observation["prior"];
    if p["state_semantics"] != "measured-v2"
        || p["count"].as_u64() != Some(prior.count)
        || p["p99_us"].as_u64() != Some(prior.p99_us)
        || p["last_error"].as_u64() != Some(prior.last_error as u64)
        || !same_float(&p["mean"], prior.mean)
    {
        return Err("container prior state mismatch".into());
    }
    let e = &observation["event"];
    let remote = &shadow["event"];
    for field in ["timestamp_ns", "entity_id", "flags"] {
        if e[field].as_u64().is_none() || e[field].as_u64() != remote[field].as_u64() {
            return Err("container shadow event mismatch".into());
        }
    }
    let remote_metric = if historical {
        remote["metric"]
            .as_f64()
            .ok_or("invalid container metric")?
    } else {
        bits(&remote["metric_bits"])?
    };
    if !same_float(&e["metric"], remote_metric)
        || !verdict_matches(&observation["champion"], &shadow["champion"]["verdict"])?
        || !verdict_matches(&observation["shadow"], &shadow["shadow"]["verdict"])?
    {
        return Err("container shadow verdict or event mismatch".into());
    }
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    let mut daemon: Option<String> = None;
    for entry in remote_cases
        .iter()
        .chain([&shadow["champion"], &shadow["shadow"]])
    {
        let receipt: Value = serde_json::from_str(
            entry["receipt"]
                .as_str()
                .ok_or("missing container receipt")?,
        )
        .map_err(|e| e.to_string())?;
        let name = receipt["name"].as_str().ok_or("missing container name")?;
        let id = receipt["container_id"]
            .as_str()
            .ok_or("missing container id")?;
        let d = receipt["daemon_id"].as_str().ok_or("missing daemon")?;
        if !names.insert(name.to_owned())
            || !ids.insert(id.to_owned())
            || daemon.as_ref().is_some_and(|old| old != d)
        {
            return Err("reused container or changed daemon".into());
        }
        daemon = Some(d.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // This exercises comparison logic only; the surrounding storage layer verifies receipts.
    #[test]
    fn exact_input_and_distinct_execution_checks_fail_closed() {
        let fixture_case = serde_json::json!({"id":"case","window":{"count":0},"event":{"timestamp_ns":9},"expected":{"decision":1},"actual":{"decision":1},"error":null,"passed":true});
        let receipt = |name: &str, id: &str| {
            serde_json::json!({"name":name,"container_id":id,"daemon_id":"daemon"}).to_string()
        };
        let mut remote_case = fixture_case.clone();
        remote_case["receipt"] = serde_json::json!(receipt("replay", "one"));
        let preview = serde_json::json!({"replay":{"candidate_id":"candidate","fixture_version":"v1","fixture_digest":"digest","state_semantics":"measured-v2","passed":true,"cases":[fixture_case]},
            "observations":[{"id":1,"champion_id":"champion","shadow_id":"candidate","champion_error":null,"shadow_error":null,
                "prior":{"state_semantics":"measured-v2","count":0,"mean":0.0,"p99_us":0,"last_error":0},
                "event":{"timestamp_ns":9,"entity_id":42,"metric":7.0,"flags":4},
                "champion":{"decision":"allow","score":7.0,"reason":4},"shadow":{"decision":"throttle","score":7.0,"reason":4}}]});
        let replay = serde_json::json!({"passed":true,"candidate_id":"candidate","fixture_version":"v1","fixture_digest":"digest","state_semantics":"measured-v2","image":"image","docker_host":"host","cases":[remote_case]});
        let mut shadow = serde_json::json!({"schema":"sese-container-shadow-experiment-v1","domain":"domain","champion_id":"champion","champion_generation":0,"shadow_id":"candidate","image":"image","docker_host":"host",
            "prior_state":crate::event_state::EventState::new(2).unwrap().encode(),"event":{"timestamp_ns":9,"entity_id":42,"metric_bits":format!("{:016x}",7.0f64.to_bits()),"flags":4},
            "champion":{"receipt":receipt("champion","two"),"verdict":{"decision":0,"score_bits":format!("{:016x}",7.0f64.to_bits()),"reason":4}},
            "shadow":{"receipt":receipt("shadow","three"),"verdict":{"decision":1,"score_bits":format!("{:016x}",7.0f64.to_bits()),"reason":4}}});
        let current = ("champion".to_owned(), 0);
        assert!(compare(&preview, &replay, &shadow, "domain", 1, &current).is_ok());
        assert!(compare(
            &preview,
            &replay,
            &shadow,
            "domain",
            1,
            &("champion".into(), 1)
        )
        .is_err());
        shadow["event"]["metric_bits"] = serde_json::json!(format!("{:016x}", 8.0f64.to_bits()));
        assert!(compare(&preview, &replay, &shadow, "domain", 1, &current).is_err());
        shadow["event"]["metric_bits"] = serde_json::json!(format!("{:016x}", 7.0f64.to_bits()));
        shadow["shadow"]["receipt"] = replay["cases"][0]["receipt"].clone();
        assert!(compare(&preview, &replay, &shadow, "domain", 1, &current).is_err());
    }
}
