//! Typing-delay meter: time from a key press to the frame that shows the shell's echo.
//!
//! This measures key → frame handed to the GPU. It does not include the monitor's own delay,
//! so a high-speed camera will read a few ms higher.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct Meter {
    pending: Option<Instant>,
    samples: VecDeque<f64>,
    log: Option<std::fs::File>,
}

impl Meter {
    pub fn new(log: Option<std::fs::File>) -> Self {
        Meter { pending: None, samples: VecDeque::with_capacity(256), log }
    }

    /// A key was sent to the shell. Only the first unanswered key counts.
    pub fn key(&mut self, at: Instant) {
        if self.pending.is_none() {
            self.pending = Some(at);
        }
    }

    /// A frame was presented; `last_output` is when the shell last printed.
    pub fn frame(&mut self, last_output: Option<Instant>, now: Instant) {
        let Some(key) = self.pending else { return };
        match last_output {
            Some(out) if out >= key => {
                let ms = (now - key).as_secs_f64() * 1000.0;
                self.pending = None;
                if self.samples.len() == 200 {
                    self.samples.pop_front();
                }
                self.samples.push_back(ms);
                if let Some(f) = self.log.as_mut() {
                    use std::io::Write;
                    let _ = writeln!(f, "{ms:.2}");
                }
            }
            // Keys the shell never echoes (e.g. a bare Shift) shouldn't poison the next sample.
            _ if now - key > Duration::from_secs(2) => self.pending = None,
            _ => {}
        }
    }

    /// (median, 95th percentile, sample count) in ms.
    pub fn stats(&self) -> Option<(f64, f64, usize)> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v: Vec<f64> = self.samples.iter().copied().collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        Some((at(0.5), at(0.95), v.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_echoed_keys() {
        let mut m = Meter::new(None);
        let t0 = Instant::now();
        m.key(t0);
        m.frame(None, t0 + Duration::from_millis(5));
        assert!(m.stats().is_none());
        m.frame(Some(t0 + Duration::from_millis(6)), t0 + Duration::from_millis(10));
        let (p50, _, n) = m.stats().unwrap();
        assert_eq!(n, 1);
        assert!((p50 - 10.0).abs() < 0.5);
    }
}
