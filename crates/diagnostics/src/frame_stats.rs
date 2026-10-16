use std::time::Duration;

/// Fixed-size ring of recent frame durations. Never allocates after construction.
#[derive(Debug, Clone)]
pub struct FrameStats {
    samples: Box<[Duration]>,
    next: usize,
    filled: usize,
    total_frames: u64,
}

/// Aggregate view over the samples currently held by [`FrameStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSummary {
    pub total_frames: u64,
    pub samples: usize,
    pub min: Duration,
    pub max: Duration,
    pub mean: Duration,
    /// 95th percentile (nearest-rank) of the held samples.
    pub p95: Duration,
}

impl FrameStats {
    /// Creates a ring holding up to `capacity` samples (at least one).
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: vec![Duration::ZERO; capacity.max(1)].into_boxed_slice(),
            next: 0,
            filled: 0,
            total_frames: 0,
        }
    }

    pub fn record(&mut self, frame_time: Duration) {
        self.samples[self.next] = frame_time;
        self.next = (self.next + 1) % self.samples.len();
        self.filled = (self.filled + 1).min(self.samples.len());
        self.total_frames += 1;
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Returns `None` until at least one frame has been recorded.
    pub fn summary(&self) -> Option<FrameSummary> {
        if self.filled == 0 {
            return None;
        }
        let mut sorted = self.samples[..self.filled].to_vec();
        sorted.sort_unstable();
        let sum: Duration = sorted.iter().sum();
        // Nearest-rank percentile: ceil(0.95 * n) as a 1-based rank.
        let rank = (self.filled * 95).div_ceil(100).max(1);
        Some(FrameSummary {
            total_frames: self.total_frames,
            samples: self.filled,
            min: sorted[0],
            max: sorted[self.filled - 1],
            mean: sum / self.filled as u32,
            p95: sorted[rank - 1],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn empty_has_no_summary() {
        assert_eq!(FrameStats::new(4).summary(), None);
    }

    #[test]
    fn summary_over_samples() {
        let mut stats = FrameStats::new(100);
        for n in 1..=20 {
            stats.record(ms(n));
        }
        let s = stats.summary().unwrap();
        assert_eq!(s.samples, 20);
        assert_eq!(s.min, ms(1));
        assert_eq!(s.max, ms(20));
        assert_eq!(s.p95, ms(19));
        assert_eq!(s.mean, Duration::from_micros(10_500));
    }

    #[test]
    fn ring_evicts_oldest() {
        let mut stats = FrameStats::new(2);
        stats.record(ms(100));
        stats.record(ms(1));
        stats.record(ms(2));
        let s = stats.summary().unwrap();
        assert_eq!(s.total_frames, 3);
        assert_eq!(s.samples, 2);
        assert_eq!(s.max, ms(2));
    }
}
