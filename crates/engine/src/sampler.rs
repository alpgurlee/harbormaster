use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use harbormaster_platform::ProcessKey;

/// CPU use between consecutive scans, as a percentage of one core.
#[derive(Debug, Default)]
pub(crate) struct CpuSampler {
    last: HashMap<ProcessKey, (Duration, Instant)>,
}

impl CpuSampler {
    pub(crate) fn sample(
        &mut self,
        key: ProcessKey,
        cpu_time: Duration,
        now: Instant,
    ) -> Option<f64> {
        let (before, then) = self.last.insert(key, (cpu_time, now))?;
        let wall = now.checked_duration_since(then)?.as_secs_f64();
        if wall <= 0.0 || cpu_time < before {
            return None;
        }
        Some((cpu_time - before).as_secs_f64() / wall * 100.0)
    }

    /// Forget processes that no longer exist.
    pub(crate) fn retain(&mut self, alive: &HashSet<ProcessKey>) {
        self.last.retain(|key, _| alive.contains(key));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use harbormaster_platform::{Pid, ProcessKey, StartTime};

    use super::CpuSampler;

    const KEY: ProcessKey = ProcessKey {
        pid: Pid(42),
        start: StartTime(7),
        exec: 0,
    };

    #[test]
    fn needs_two_samples_before_reporting() {
        let mut sampler = CpuSampler::default();

        assert_eq!(
            sampler.sample(KEY, Duration::from_millis(10), Instant::now()),
            None
        );
    }

    #[test]
    fn reports_cpu_time_over_wall_time() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        sampler.sample(KEY, Duration::from_millis(100), start);

        let percent = sampler.sample(
            KEY,
            Duration::from_millis(600),
            start + Duration::from_secs(2),
        );

        assert_eq!(percent, Some(25.0));
    }

    #[test]
    fn does_not_mix_up_a_reused_pid() {
        let mut sampler = CpuSampler::default();
        let start = Instant::now();
        sampler.sample(KEY, Duration::from_secs(50), start);
        let reused = ProcessKey {
            pid: Pid(42),
            start: StartTime(8),
            exec: 0,
        };

        assert_eq!(
            sampler.sample(
                reused,
                Duration::from_millis(5),
                start + Duration::from_secs(2)
            ),
            None
        );
    }
}
