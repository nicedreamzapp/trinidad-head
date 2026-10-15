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

#[cfg(test)]
mod more_tests {
    use super::*;

    fn sample(m: &mut Meter, t0: Instant, ms: u64) {
        m.key(t0);
        m.frame(Some(t0), t0 + Duration::from_millis(ms));
    }

    #[test]
    fn no_samples_no_stats() {
        assert!(Meter::new(None).stats().is_none());
    }

    #[test]
    fn a_second_key_before_the_echo_does_not_restart_the_clock() {
        let mut m = Meter::new(None);
        let t0 = Instant::now();
        m.key(t0);
        m.key(t0 + Duration::from_millis(8));
        m.frame(Some(t0 + Duration::from_millis(9)), t0 + Duration::from_millis(12));
        let (p50, _, _) = m.stats().unwrap();
        assert!((p50 - 12.0).abs() < 0.01, "timed from the first key: {p50}");
    }

    #[test]
    fn output_from_before_the_key_is_not_its_echo() {
        let mut m = Meter::new(None);
        let t0 = Instant::now();
        m.key(t0 + Duration::from_millis(5));
        m.frame(Some(t0), t0 + Duration::from_millis(10));
        assert!(m.stats().is_none());
    }

    #[test]
    fn a_key_never_echoed_is_dropped_after_two_seconds() {
        let mut m = Meter::new(None);
        let t0 = Instant::now();
        m.key(t0);
        m.frame(None, t0 + Duration::from_millis(2001));
        // The next key starts its own clock rather than inheriting the stale one.
        let t1 = t0 + Duration::from_secs(3);
        m.key(t1);
        m.frame(Some(t1), t1 + Duration::from_millis(4));
        let (p50, _, n) = m.stats().unwrap();
        assert_eq!(n, 1);
        assert!((p50 - 4.0).abs() < 0.01);
    }

    #[test]
    fn median_and_95th_percentile() {
        let mut m = Meter::new(None);
        let t0 = Instant::now();
        for ms in 1..=100 {
            sample(&mut m, t0, ms);
        }
        let (p50, p95, n) = m.stats().unwrap();
        assert_eq!(n, 100);
        // Nearest rank on 0-based index round((n - 1) * p).
        assert!((p50 - 51.0).abs() < 0.01, "{p50}");
        assert!((p95 - 95.0).abs() < 0.01, "{p95}");
    }

    #[test]
    fn keeps_only_the_last_two_hundred_samples() {
        let mut m = Meter::new(None);
        let t0 = Instant::now();
        for _ in 0..50 {
            sample(&mut m, t0, 1000);
        }
        for _ in 0..200 {
            sample(&mut m, t0, 3);
        }
        let (p50, p95, n) = m.stats().unwrap();
        assert_eq!(n, 200);
        assert!((p50 - 3.0).abs() < 0.01 && (p95 - 3.0).abs() < 0.01, "the old slow samples are gone");
    }

    #[test]
    fn writes_each_sample_to_the_log() {
        let path = std::env::temp_dir().join(format!("th-latency-test-{}.log", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let mut m = Meter::new(Some(file));
        let t0 = Instant::now();
        sample(&mut m, t0, 7);
        sample(&mut m, t0, 12);
        drop(m);
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(text, "7.00\n12.00\n");
    }
}
