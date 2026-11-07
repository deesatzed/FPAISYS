//! Experimental synthetic host with separate scalar and window ABI paths.
use crate::{
    shadow::{compare, Observation},
    storage::Storage,
};
use std::{collections::VecDeque, time::Duration};

pub struct Host {
    capacity: usize,
    values: VecDeque<i32>,
}
impl Host {
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=4096).contains(&capacity) {
            return Err("window capacity must be 1..=4096".into());
        }
        Ok(Self {
            capacity,
            values: VecDeque::with_capacity(capacity),
        })
    }
    pub fn window(&self) -> Vec<i32> {
        self.values.iter().copied().collect()
    }
    /// Commit the event to the local window only after its comparison record is persisted.
    pub fn process(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        value: i32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let result = compare(db, worker, champion, shadow, value, timeout)?;
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
        Ok(result)
    }
    /// Synthetic window ABI comparison; stored separately from scalar review evidence.
    pub fn process_window(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        value: i32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let window = self.window();
        let mut result = evaluate_pair(db, worker, champion, shadow, &window, value, timeout)?;
        result.id = db.save_window_observation(champion, shadow, &window, value, &result)?;
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
        Ok(result)
    }
}

fn evaluate_pair(
    db: &Storage,
    worker: &str,
    champion: &str,
    shadow: &str,
    window: &[i32],
    value: i32,
    timeout: Duration,
) -> Result<Observation, String> {
    let champion_bytes = db.artifact_for_abi(champion, 2)?;
    let shadow_bytes = db.artifact_for_abi(shadow, 2)?;
    let start = std::time::Instant::now();
    let champion_result = crate::supervise_window(worker, &champion_bytes, window, value, timeout);
    let champion_us = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    let shadow_result = crate::supervise_window(worker, &shadow_bytes, window, value, timeout);
    let shadow_us = i64::try_from(start.elapsed().as_micros()).map_err(|e| e.to_string())?;
    let (c, ce) = match champion_result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    let (s, se) = match shadow_result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    let result = Observation {
        id: 0,
        champion: c,
        champion_error: ce,
        shadow: s,
        shadow_error: se,
        diverged: match (c, s) {
            (Some(a), Some(b)) => Some(a != b),
            _ => None,
        },
        champion_us,
        shadow_us,
    };
    Ok(result)
}

pub struct DurableHost {
    name: String,
    capacity: usize,
    generation: i64,
    values: Vec<i32>,
}
impl DurableHost {
    pub fn open(db: &mut Storage, name: &str, capacity: usize) -> Result<Self, String> {
        if name.trim().is_empty() || name.len() > 256 || !(1..=4096).contains(&capacity) {
            return Err("invalid stream".into());
        }
        let (generation, values) = db.load_window_stream(name, capacity)?;
        Ok(Self {
            name: name.into(),
            capacity,
            generation,
            values,
        })
    }
    pub fn window(&self) -> Vec<i32> {
        self.values.clone()
    }
    pub fn process(
        &mut self,
        db: &mut Storage,
        worker: &str,
        champion: &str,
        shadow: &str,
        value: i32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let mut result = evaluate_pair(db, worker, champion, shadow, &self.values, value, timeout)?;
        let mut next = self.values.clone();
        if next.len() == self.capacity {
            next.remove(0);
        }
        next.push(value);
        result.id = db.commit_window_stream(
            &self.name,
            self.generation,
            self.capacity,
            &self.values,
            &next,
            champion,
            shadow,
            value,
            &result,
            None,
        )?;
        self.values = next;
        self.generation += 1;
        Ok(result)
    }
    /// Evaluate the current synthetic domain champion; fail if its generation changes before commit.
    pub fn process_domain(
        &mut self,
        db: &mut Storage,
        worker: &str,
        domain: &str,
        shadow: &str,
        value: i32,
        timeout: Duration,
    ) -> Result<Observation, String> {
        let (champion, generation) = db.champion_snapshot(domain)?;
        let mut result =
            evaluate_pair(db, worker, &champion, shadow, &self.values, value, timeout)?;
        let mut next = self.values.clone();
        if next.len() == self.capacity {
            next.remove(0);
        }
        next.push(value);
        result.id = db.commit_window_stream(
            &self.name,
            self.generation,
            self.capacity,
            &self.values,
            &next,
            &champion,
            shadow,
            value,
            &result,
            Some((domain, &champion, generation)),
        )?;
        self.values = next;
        self.generation += 1;
        Ok(result)
    }
}
