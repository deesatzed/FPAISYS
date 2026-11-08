//! Local rule proposal compiler. Compiling does not execute or authorize a candidate.

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FixtureProposalRecord {
    pub schema: String,
    pub generator: String,
    pub candidate_id: String,
    pub threshold_bits: String,
    pub fixture_version: String,
    pub fixture_digest: String,
    pub fixture_raw_sha256: String,
    pub fixture_json: String,
    pub training_cases: usize,
}

/// Generate an event-v1 module which throttles when metric > threshold.
pub fn threshold_artifact(threshold: f64) -> Result<Vec<u8>, String> {
    if !threshold.is_finite() || !(-1_000_000_000_000.0..=1_000_000_000_000.0).contains(&threshold)
    {
        return Err("invalid threshold".into());
    }
    let source = format!(
        r#"(module
      (func (export "evaluate_event_v1")
        (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32)
        local.get 6 f64.const {threshold:e} f64.gt
        if (result i32) i32.const 1 else i32.const 0 end
        local.get 6 local.get 7))"#
    );
    wat::parse_str(source).map_err(|e| e.to_string())
}

/// Infer one threshold from labeled allow/throttle cases without executing a guest.
pub fn threshold_from_fixture(pack: &crate::fixture_file::FixturePack) -> Result<f64, String> {
    use crate::event_abi::Decision;
    let mut allow: Option<f64> = None;
    let mut throttle: Option<f64> = None;
    for case in &pack.cases {
        let metric = case.event.metric;
        if !metric.is_finite()
            || !(-1_000_000_000_000.0..=1_000_000_000_000.0).contains(&metric)
            || case.expected.score.to_bits() != metric.to_bits()
            || case.expected.reason != case.event.flags
        {
            return Err("fixture cannot be represented by the threshold rule".into());
        }
        match case.expected.decision {
            Decision::Allow => {
                allow = Some(allow.map_or(metric, |v| {
                    if metric.total_cmp(&v).is_gt() {
                        metric
                    } else {
                        v
                    }
                }))
            }
            Decision::Throttle => {
                throttle = Some(throttle.map_or(metric, |v| {
                    if metric.total_cmp(&v).is_lt() {
                        metric
                    } else {
                        v
                    }
                }))
            }
            _ => return Err("fixture has unsupported decisions".into()),
        }
    }
    let (allow, throttle) = (
        allow.ok_or("fixture needs both allow and throttle examples")?,
        throttle.ok_or("fixture needs both allow and throttle examples")?,
    );
    if allow >= throttle {
        return Err("fixture is not separable by a threshold".into());
    }
    Ok(allow)
}
