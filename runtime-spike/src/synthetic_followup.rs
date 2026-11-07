//! Compare a recorded synthetic observation with a predeclared fixture case.
use serde_json::Value;

fn number(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("invalid {key}"))
}
fn float(value: &Value, key: &str) -> Result<u64, String> {
    let number = value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("invalid {key}"))?;
    if !number.is_finite() {
        return Err(format!("invalid {key}"));
    }
    Ok(number.to_bits())
}

pub(crate) fn compare(
    window: &str,
    event: &str,
    expected: &str,
    prior: &str,
    observed_event: &str,
    actual: Option<&str>,
    error: Option<&str>,
) -> Result<&'static str, String> {
    let parse = |s: &str| serde_json::from_str::<Value>(s).map_err(|e| e.to_string());
    let window = parse(window)?;
    let event = parse(event)?;
    let expected = parse(expected)?;
    let prior = parse(prior)?;
    let observed_event = parse(observed_event)?;
    let same_input = number(&window, "count")? == number(&prior, "count")?
        && float(&window, "mean")? == float(&prior, "mean")?
        && number(&window, "p99_us")? == number(&prior, "p99_us")?
        && number(&window, "last_error")? == number(&prior, "last_error")?
        && number(&event, "timestamp_ns")? == number(&observed_event, "timestamp_ns")?
        && number(&event, "entity_id")? == number(&observed_event, "entity_id")?
        && float(&event, "metric")? == float(&observed_event, "metric")?
        && number(&event, "flags")? == number(&observed_event, "flags")?;
    if !same_input {
        return Ok("input_mismatch");
    }
    if let Some(message) = error {
        if message.is_empty() || actual.is_some() {
            return Err("invalid observation outcome".into());
        }
        return Ok("worker_error");
    }
    let actual = parse(actual.ok_or("missing observation outcome")?)?;
    let expected_decision = match number(&expected, "decision")? {
        0 => "allow",
        1 => "throttle",
        2 => "quarantine",
        3 => "escalate",
        _ => return Err("invalid expected decision".into()),
    };
    let decision = actual
        .get("decision")
        .and_then(Value::as_str)
        .ok_or("invalid observed decision")?;
    if !["allow", "throttle", "quarantine", "escalate"].contains(&decision) {
        return Err("invalid observed decision".into());
    }
    let same = decision == expected_decision
        && float(&expected, "score")? == float(&actual, "score")?
        && number(&expected, "reason")? == number(&actual, "reason")?;
    Ok(if same { "match" } else { "verdict_mismatch" })
}

#[cfg(test)]
mod tests {
    use super::compare;
    const WINDOW: &str = r#"{"count":0,"mean":0.0,"p99_us":0,"last_error":0}"#;
    const EVENT: &str = r#"{"timestamp_ns":1,"entity_id":2,"metric":0.0,"flags":0}"#;
    const EXPECTED: &str = r#"{"decision":1,"score":0.0,"reason":2}"#;
    #[test]
    fn distinguishes_exact_verdict_mismatch_worker_error_and_signed_zero() {
        let actual = r#"{"decision":"throttle","score":0.0,"reason":2}"#;
        let mismatch = r#"{"decision":"allow","score":0.0,"reason":2}"#;
        let negative_zero = r#"{"timestamp_ns":1,"entity_id":2,"metric":-0.0,"flags":0}"#;
        assert_eq!(
            compare(WINDOW, EVENT, EXPECTED, WINDOW, EVENT, Some(actual), None).unwrap(),
            "match"
        );
        assert_eq!(
            compare(WINDOW, EVENT, EXPECTED, WINDOW, EVENT, Some(mismatch), None).unwrap(),
            "verdict_mismatch"
        );
        assert_eq!(
            compare(
                WINDOW,
                EVENT,
                EXPECTED,
                WINDOW,
                EVENT,
                None,
                Some("worker failed")
            )
            .unwrap(),
            "worker_error"
        );
        assert_eq!(
            compare(
                WINDOW,
                EVENT,
                EXPECTED,
                WINDOW,
                negative_zero,
                Some(actual),
                None
            )
            .unwrap(),
            "input_mismatch"
        );
        assert!(compare(
            WINDOW,
            EVENT,
            EXPECTED,
            WINDOW,
            EVENT,
            Some(actual),
            Some("worker failed")
        )
        .is_err());
    }
}
