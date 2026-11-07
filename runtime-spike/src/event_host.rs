//! Synthetic latency window host. Not an OS containment boundary or production host.
#[cfg(unix)]
pub use crate::container_observation::{compare_container_observation, ContainerObservationReport};
#[cfg(unix)]
pub use crate::container_shadow::{ContainerShadowHost, ContainerShadowObservation};
use crate::{
    event_abi::{Decision, Event, Verdict, Window},
    storage::Storage,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Observation {
    pub id: i64,
    pub prior: Window,
    pub champion: Option<Verdict>,
    pub champion_error: Option<String>,
    pub shadow: Option<Verdict>,
    pub shadow_error: Option<String>,
}
pub struct EventHost {
    capacity: usize,
    latencies: VecDeque<f64>,
    stream: Option<(String, i64)>,
    scope: Option<String>,
}
fn verdict_json(v: Verdict) -> String {
    let code = match v.decision {
        Decision::Allow => "allow",
        Decision::Throttle => "throttle",
        Decision::Quarantine => "quarantine",
        Decision::Escalate => "escalate",
    };
    serde_json::json!({"decision":code,"score":v.score,"reason":v.reason}).to_string()
}
impl EventHost {
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=4096).contains(&capacity) {
            return Err("invalid capacity".into());
        }
        Ok(Self {
            capacity,
            latencies: VecDeque::with_capacity(capacity),
            stream: None,
            scope: None,
        })
    }
    /// Open a persistent synthetic stream. Concurrent stale writers fail without recording an observation.
    pub fn open(db: &mut Storage, name: &str, capacity: usize) -> Result<Self, String> {
        if name.trim().is_empty() || name.len() > 256 || !(1..=4096).contains(&capacity) {
            return Err("invalid event stream".into());
        }
        let (generation, values, scope) = db.load_event_stream(name, capacity)?;
        Ok(Self {
            capacity,
            latencies: values.into(),
            stream: Some((name.into(), generation)),
            scope,
        })
    }
    /// Snapshot covers only prior events; metric is synthetic nonnegative latency in microseconds.
    pub fn snapshot(&self, last_error: u32) -> Window {
        let mut sorted: Vec<f64> = self.latencies.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let mean = if sorted.is_empty() {
            0.0
        } else {
            sorted.iter().sum::<f64>() / (sorted.len() as f64)
        };
        let p99 = if sorted.is_empty() {
            0
        } else {
            let rank = (sorted.len() * 99).div_ceil(100);
            sorted[rank - 1].ceil() as u64
        };
        Window {
            count: sorted.len() as u64,
            mean,
            p99_us: p99,
            last_error,
        }
    }
    pub fn process(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        event: &Event,
        last_error: u32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        self.process_inner(
            db, worker, champion, shadow, event, last_error, timeout, None,
        )
    }
    /// Run the current synthetic domain champion and reject a transition during execution.
    pub fn process_domain(
        &mut self,
        db: &mut Storage,
        worker: &str,
        domain: &str,
        shadow: &str,
        event: &Event,
        last_error: u32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let (champion, generation, semantics, selected_worker) =
            db.event_champion_snapshot(domain)?;
        let selected_worker = selected_worker.ok_or("fixture worker provenance missing")?;
        if semantics != "legacy-v1" {
            return Err("domain requires measured state".into());
        }
        self.process_inner(
            db,
            worker,
            &champion,
            shadow,
            event,
            last_error,
            timeout,
            Some((domain, generation, &selected_worker)),
        )
    }
    /// Reject a changed champion before running either guest; the existing generation guard also applies at commit.
    pub fn process_domain_expected(
        &mut self,
        db: &mut Storage,
        worker: &str,
        domain: &str,
        expected: &str,
        shadow: &str,
        event: &Event,
        last_error: u32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let (champion, generation, semantics, selected_worker) =
            db.event_champion_snapshot(domain)?;
        if champion != expected {
            return Err("unexpected domain champion".into());
        }
        let selected_worker = selected_worker.ok_or("fixture worker provenance missing")?;
        if semantics != "legacy-v1" {
            return Err("domain requires measured state".into());
        }
        self.process_inner(
            db,
            worker,
            &champion,
            shadow,
            event,
            last_error,
            timeout,
            Some((domain, generation, &selected_worker)),
        )
    }
    fn process_inner(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        event: &Event,
        last_error: u32,
        timeout: Duration,
        guard: Option<(&str, i64, &str)>,
    ) -> Result<Observation, String> {
        let desired = guard.map(|(domain, _, _)| domain).unwrap_or("");
        if self.scope.as_deref().is_some_and(|s| s != desired) {
            return Err("event stream domain mismatch".into());
        }
        if !event.metric.is_finite() || !(0.0..=1_000_000_000_000.0).contains(&event.metric) {
            return Err("invalid synthetic latency".into());
        }
        let c = db.artifact_for_abi(champion, 3)?;
        let s = db.artifact_for_abi(shadow, 3)?;
        let executor = crate::event_executor::LocalEventExecutor::capture(worker)?;
        if guard.is_some_and(|(_, _, selected)| selected != executor.identity.sha256) {
            return Err("worker differs from fixture selection".into());
        }
        let prior = self.snapshot(last_error);
        let start = Instant::now();
        let c = executor.execute(&c, &prior, event, timeout);
        let cu = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
        let start = Instant::now();
        let s = executor.execute(&s, &prior, event, timeout);
        let su = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
        executor.verify_unchanged()?;
        let (c, ce) = match c {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        let (s, se) = match s {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        let p=serde_json::json!({"count":prior.count,"mean":prior.mean,"p99_us":prior.p99_us,"last_error":prior.last_error}).to_string();
        let e=serde_json::json!({"timestamp_ns":event.timestamp_ns,"entity_id":event.entity_id,"metric":event.metric,"flags":event.flags}).to_string();
        let old: Vec<f64> = self.latencies.iter().copied().collect();
        let mut next = old.clone();
        if next.len() == self.capacity {
            next.remove(0);
        }
        next.push(event.metric);
        let stream = self.stream.as_ref().map(|(name, generation)| {
            (
                name.as_str(),
                *generation,
                self.capacity,
                old.as_slice(),
                next.as_slice(),
            )
        });
        let id = db.save_event_observation(
            champion,
            shadow,
            &p,
            &e,
            &c.map(verdict_json),
            &ce,
            &s.map(verdict_json),
            &se,
            cu,
            su,
            stream,
            guard.map(|(domain, generation, selected)| (domain, champion, generation, selected)),
            None,
            &executor.identity,
        )?;
        self.scope = Some(desired.to_owned());
        self.latencies = next.into();
        if let Some((_, generation)) = self.stream.as_mut() {
            *generation += 1;
        }
        Ok(Observation {
            id,
            prior,
            champion: c,
            champion_error: ce,
            shadow: s,
            shadow_error: se,
        })
    }
}

/// Experimental measured state path for synthetic candidates.
pub struct MeasuredEventHost {
    state: crate::event_state::EventState,
    stream: Option<(String, i64)>,
    scope: Option<String>,
}
impl MeasuredEventHost {
    pub fn new(capacity: usize) -> Result<Self, String> {
        Ok(Self {
            state: crate::event_state::EventState::new(capacity)?,
            stream: None,
            scope: None,
        })
    }
    pub fn open(db: &mut Storage, name: &str, capacity: usize) -> Result<Self, String> {
        let (generation, state, scope) = db.load_measured_stream(name, capacity)?;
        Ok(Self {
            state,
            stream: Some((name.into(), generation)),
            scope,
        })
    }
    pub fn snapshot(&self) -> Window {
        self.state.snapshot()
    }
    pub fn process(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        event: &Event,
        timeout: Duration,
    ) -> Result<Observation, String> {
        self.process_inner(db, worker, champion, shadow, event, timeout, None)
    }
    /// Use the verified measured-domain champion; reject a transition during execution.
    pub fn process_domain(
        &mut self,
        db: &mut Storage,
        worker: &str,
        domain: &str,
        shadow: &str,
        event: &Event,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let (champion, generation, semantics, selected_worker) =
            db.event_champion_snapshot(domain)?;
        let selected_worker = selected_worker.ok_or("fixture worker provenance missing")?;
        if semantics != "measured-v2" {
            return Err("domain requires legacy state".into());
        }
        self.process_inner(
            db,
            worker,
            &champion,
            shadow,
            event,
            timeout,
            Some((domain, generation, &selected_worker)),
        )
    }
    /// Reject a changed champion before running either guest; the existing generation guard also applies at commit.
    pub fn process_domain_expected(
        &mut self,
        db: &mut Storage,
        worker: &str,
        domain: &str,
        expected: &str,
        shadow: &str,
        event: &Event,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let (champion, generation, semantics, selected_worker) =
            db.event_champion_snapshot(domain)?;
        if champion != expected {
            return Err("unexpected domain champion".into());
        }
        let selected_worker = selected_worker.ok_or("fixture worker provenance missing")?;
        if semantics != "measured-v2" {
            return Err("domain requires legacy state".into());
        }
        self.process_inner(
            db,
            worker,
            &champion,
            shadow,
            event,
            timeout,
            Some((domain, generation, &selected_worker)),
        )
    }
    fn process_inner(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        event: &Event,
        timeout: Duration,
        guard: Option<(&str, i64, &str)>,
    ) -> Result<Observation, String> {
        let desired = guard.map(|(domain, _, _)| domain).unwrap_or("");
        if self.scope.as_deref().is_some_and(|s| s != desired) {
            return Err("event stream domain mismatch".into());
        }
        if !event.metric.is_finite()
            || !(-1_000_000_000_000.0..=1_000_000_000_000.0).contains(&event.metric)
        {
            return Err("invalid metric".into());
        }
        let champion_bytes = db.artifact_for_abi(champion, 3)?;
        let shadow_bytes = db.artifact_for_abi(shadow, 3)?;
        let executor = crate::event_executor::LocalEventExecutor::capture(worker)?;
        if guard.is_some_and(|(_, _, selected)| selected != executor.identity.sha256) {
            return Err("worker differs from fixture selection".into());
        }
        let prior = self.state.snapshot();
        let start = Instant::now();
        let champion_result = executor.execute(&champion_bytes, &prior, event, timeout);
        let champion_us = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
        let start = Instant::now();
        let shadow_result = executor.execute(&shadow_bytes, &prior, event, timeout);
        let shadow_us = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
        executor.verify_unchanged()?;
        let (c, ce) = match champion_result {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        let (s, se) = match shadow_result {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        let mut next = self.state.clone();
        next.record(
            event.metric,
            champion_us as u64,
            if ce.is_some() { 500 } else { 0 },
        )?;
        let p=serde_json::json!({"state_semantics":"measured-v2","count":prior.count,"mean":prior.mean,"p99_us":prior.p99_us,"last_error":prior.last_error}).to_string();
        let e=serde_json::json!({"timestamp_ns":event.timestamp_ns,"entity_id":event.entity_id,"metric":event.metric,"flags":event.flags}).to_string();
        let old = self.state.encode();
        let updated = next.encode();
        let stream = self.stream.as_ref().map(|(name, generation)| {
            (
                name.as_str(),
                *generation,
                old.as_slice(),
                updated.as_slice(),
            )
        });
        let id = db.save_event_observation(
            champion,
            shadow,
            &p,
            &e,
            &c.map(verdict_json),
            &ce,
            &s.map(verdict_json),
            &se,
            champion_us,
            shadow_us,
            None,
            guard.map(|(domain, generation, selected)| (domain, champion, generation, selected)),
            stream,
            &executor.identity,
        )?;
        self.scope = Some(desired.to_owned());
        self.state = next;
        if let Some((_, generation)) = self.stream.as_mut() {
            *generation += 1;
        }
        Ok(Observation {
            id,
            prior,
            champion: c,
            champion_error: ce,
            shadow: s,
            shadow_error: se,
        })
    }
}
