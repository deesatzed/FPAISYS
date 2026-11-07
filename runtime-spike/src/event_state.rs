//! Versioned synthetic state used by the measured event host and its durable streams.
use crate::event_abi::Window;
use std::collections::VecDeque;

const MAX_METRIC: f64 = 1_000_000_000_000.0;
#[derive(Clone, Debug)]
pub struct EventState {
    capacity: usize,
    total: u64,
    last_error: u32,
    samples: VecDeque<(f64, u64)>,
}
impl EventState {
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=4096).contains(&capacity) {
            return Err("invalid capacity".into());
        }
        Ok(Self {
            capacity,
            total: 0,
            last_error: 0,
            samples: VecDeque::with_capacity(capacity),
        })
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    /// Record one host observation. `duration_us` is the elapsed champion attempt, including failures.
    /// A nonzero error replaces the last error; successful observations do not erase it.
    pub fn record(&mut self, metric: f64, duration_us: u64, error: u32) -> Result<(), String> {
        if !metric.is_finite() || !(-MAX_METRIC..=MAX_METRIC).contains(&metric) {
            return Err("invalid metric".into());
        }
        let next = self.total.checked_add(1).ok_or("event count overflow")?;
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back((metric, duration_us));
        self.total = next;
        if error != 0 {
            self.last_error = error;
        }
        Ok(())
    }
    /// Count is lifetime observations. Mean and p99 use retained samples; p99 is the
    /// sorted value at floor(0.99*n), clamped to n-1, matching the source sketch.
    pub fn snapshot(&self) -> Window {
        let mean = if self.samples.is_empty() {
            0.0
        } else {
            self.samples.iter().map(|(metric, _)| metric).sum::<f64>() / self.samples.len() as f64
        };
        let mut latencies: Vec<u64> = self.samples.iter().map(|(_, latency)| *latency).collect();
        latencies.sort_unstable();
        let p99 = if latencies.is_empty() {
            0
        } else {
            latencies[(latencies.len() * 99 / 100).min(latencies.len() - 1)]
        };
        Window {
            count: self.total,
            mean,
            p99_us: p99,
            last_error: self.last_error,
        }
    }
    /// Big-endian SES2 format; historical SES1 metric-only streams are never decoded as durations.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24 + self.samples.len() * 16);
        bytes.extend_from_slice(b"SES2");
        bytes.extend_from_slice(&(self.capacity as u32).to_be_bytes());
        bytes.extend_from_slice(&self.total.to_be_bytes());
        bytes.extend_from_slice(&self.last_error.to_be_bytes());
        bytes.extend_from_slice(&(self.samples.len() as u32).to_be_bytes());
        for (metric, latency) in &self.samples {
            bytes.extend_from_slice(&metric.to_bits().to_be_bytes());
            bytes.extend_from_slice(&latency.to_be_bytes());
        }
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 24 || &bytes[..4] != b"SES2" {
            return Err("invalid state format".into());
        }
        let capacity = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let total = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
        let last_error = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let count = u32::from_be_bytes(bytes[20..24].try_into().unwrap()) as usize;
        if !(1..=4096).contains(&capacity)
            || count > capacity
            || total < count as u64
            || bytes.len() != 24 + count * 16
        {
            return Err("invalid state dimensions".into());
        }
        // Every observation contributes one retained sample, up to capacity.
        if count != (total.min(capacity as u64) as usize) {
            return Err("inconsistent state count".into());
        }
        let mut samples = VecDeque::with_capacity(capacity);
        for part in bytes[24..].chunks_exact(16) {
            let metric = f64::from_bits(u64::from_be_bytes(part[..8].try_into().unwrap()));
            let latency = u64::from_be_bytes(part[8..].try_into().unwrap());
            if !metric.is_finite() || !(-MAX_METRIC..=MAX_METRIC).contains(&metric) {
                return Err("invalid state metric".into());
            }
            samples.push_back((metric, latency));
        }
        Ok(Self {
            capacity,
            total,
            last_error,
            samples,
        })
    }
}
