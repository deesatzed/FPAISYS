//! Synthetic comparison; no champion selection, promotion or containment is implied.
use crate::{storage::Storage, supervise};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct Observation {
    pub id: i64,
    pub champion: Option<i32>,
    pub champion_error: Option<String>,
    pub shadow: Option<i32>,
    pub shadow_error: Option<String>,
    pub diverged: Option<bool>,
    pub champion_us: i64,
    pub shadow_us: i64,
}
pub fn compare(
    db: &mut Storage,
    worker: &str,
    champion: &str,
    shadow: &str,
    input: i32,
    timeout: Duration,
) -> Result<Observation, String> {
    let champion_bytes = db.artifact_for_abi(champion, 1)?;
    let shadow_bytes = db.artifact_for_abi(shadow, 1)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string();
    let start = Instant::now();
    let champion_result = supervise(worker, &champion_bytes, input, timeout);
    let champion_us = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let shadow_result = supervise(worker, &shadow_bytes, input, timeout);
    let shadow_us = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
    let (champion_value, champion_error) = match champion_result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    let (shadow_value, shadow_error) = match shadow_result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    let diverged = match (champion_value, shadow_value) {
        (Some(a), Some(b)) => Some(a != b),
        _ => None,
    };
    let mut result = Observation {
        id: 0,
        champion: champion_value,
        champion_error,
        shadow: shadow_value,
        shadow_error,
        diverged,
        champion_us,
        shadow_us,
    };
    result.id = db.save_shadow(champion, shadow, input, &result, &timestamp)?;
    Ok(result)
}
