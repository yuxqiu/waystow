//! Reading progress out of tool output.

/// Accumulates progress from output lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tracker {
    duration_us: Option<f64>,
    fraction: Option<f32>,
}

impl Tracker {
    /// The latest fraction, 0–1.
    #[must_use]
    pub const fn fraction(&self) -> Option<f32> {
        self.fraction
    }

    /// Feed one line in ffmpeg mode (stderr header and `-progress` stdout).
    /// Returns true when the fraction changed.
    pub fn ffmpeg_line(&mut self, line: &str) -> bool {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Duration:") {
            // Only the first input's duration counts (later inputs are filters' inputs).
            if self.duration_us.is_none() {
                self.duration_us = rest.split(',').next().and_then(|t| parse_clock(t.trim())).map(|s| s * 1e6);
            }
            return false;
        }
        if line == "progress=end" {
            return self.set(1.0);
        }
        let value = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms="));
        match (value.and_then(|v| v.parse::<f64>().ok()), self.duration_us) {
            (Some(us), Some(total)) if total > 0.0 && us >= 0.0 => self.set((us / total) as f32),
            _ => false,
        }
    }

    /// Feed one line in percent mode: the last `NN%` or `NN.N%` on the line.
    pub fn percent_line(&mut self, line: &str) -> bool {
        let mut found = None;
        for (i, _) in line.match_indices('%') {
            let digits: String = line[..i]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(v) = digits.parse::<f32>() {
                found = Some(v / 100.0);
            }
        }
        found.is_some_and(|v| self.set(v))
    }

    fn set(&mut self, v: f32) -> bool {
        let v = v.clamp(0.0, 1.0);
        // Only move forward, and only report steps of at least 0.5%.
        if self.fraction.is_some_and(|old| v < old || v - old < 0.005 && v < 1.0) {
            return false;
        }
        self.fraction = Some(v);
        true
    }
}

/// `01:02:03.45` → seconds.
fn parse_clock(s: &str) -> Option<f64> {
    let mut parts = s.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    parts.next().is_none().then(|| h.mul_add(3600.0, m * 60.0) + sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg() {
        let mut t = Tracker::default();
        assert!(!t.ffmpeg_line("out_time_us=1000000"), "no duration yet");
        t.ffmpeg_line("  Duration: 00:00:10.00, start: 0.000000, bitrate: 1 kb/s");
        t.ffmpeg_line("  Duration: 00:05:00.00, start: 0.000000");
        assert!(t.ffmpeg_line("out_time_us=5000000"));
        assert!((t.fraction().unwrap() - 0.5).abs() < 1e-6);
        assert!(!t.ffmpeg_line("out_time_us=4000000"), "never goes backwards");
        assert!(!t.ffmpeg_line("out_time_us=N/A"));
        assert!(t.ffmpeg_line("progress=end"));
        assert!((t.fraction().unwrap() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ffmpeg_without_duration_stays_indeterminate() {
        let mut t = Tracker::default();
        t.ffmpeg_line("  Duration: N/A, bitrate: N/A");
        assert!(!t.ffmpeg_line("out_time_us=1"));
        assert_eq!(t.fraction(), None);
    }

    #[test]
    fn percent() {
        let mut t = Tracker::default();
        assert!(t.percent_line("page 3 of 10 (12.5%) done 30%"));
        assert!((t.fraction().unwrap() - 0.3).abs() < 1e-6);
        assert!(!t.percent_line("no numbers %"));
        assert!(!t.percent_line("back to 10%"), "never goes backwards");
        assert!(t.percent_line("100% done"));
        assert!((t.fraction().unwrap() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn clock() {
        assert_eq!(parse_clock("01:02:03.5"), Some(3723.5));
        assert_eq!(parse_clock("N/A"), None);
    }
}
