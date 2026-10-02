//! A damped spring for UI motion: every animated value in WayStow is one of
//! these, so motion keeps its velocity when a target changes mid-flight.

/// A 1-D spring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// Current value.
    pub value: f64,
    /// Current velocity (units per second).
    pub velocity: f64,
    /// Where it is heading.
    pub target: f64,
    /// Stiffness (higher is faster).
    pub stiffness: f64,
    /// Damping ratio: 1 = critically damped, below 1 overshoots.
    pub damping_ratio: f64,
}

impl Spring {
    /// The default feel: quick with a slight overshoot.
    #[must_use]
    pub const fn new(value: f64) -> Self {
        Self { value, velocity: 0.0, target: value, stiffness: 320.0, damping_ratio: 0.82 }
    }

    /// Same, with custom stiffness and damping.
    #[must_use]
    pub const fn with(value: f64, stiffness: f64, damping_ratio: f64) -> Self {
        Self { value, velocity: 0.0, target: value, stiffness, damping_ratio }
    }

    /// Jump straight to `v` (reduced motion, first layout).
    pub const fn snap(&mut self, v: f64) {
        self.value = v;
        self.target = v;
        self.velocity = 0.0;
    }

    /// Advance by `dt` seconds. Large steps are subdivided for stability.
    pub fn step(&mut self, dt: f64) {
        let dt = dt.clamp(0.0, 0.25);
        let steps = (dt * 240.0).ceil().max(1.0) as u32;
        let h = dt / f64::from(steps);
        let damping = 2.0 * self.damping_ratio * self.stiffness.sqrt();
        for _ in 0..steps {
            let accel = self.stiffness.mul_add(self.target - self.value, -damping * self.velocity);
            self.velocity = accel.mul_add(h, self.velocity);
            self.value = self.velocity.mul_add(h, self.value);
        }
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    /// Close enough to rest.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        (self.target - self.value).abs() < 1e-3 && self.velocity.abs() < 1e-2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settles_on_target_with_a_small_overshoot() {
        let mut s = Spring::new(0.0);
        s.target = 1.0;
        let mut peak: f64 = 0.0;
        for _ in 0..120 {
            s.step(1.0 / 60.0);
            peak = peak.max(s.value);
        }
        assert!(s.is_settled(), "{s:?}");
        assert!((s.value - 1.0).abs() < f64::EPSILON);
        assert!(peak > 1.0 && peak < 1.1, "peak {peak}");
    }

    #[test]
    fn critically_damped_never_overshoots() {
        let mut s = Spring::with(0.0, 300.0, 1.0);
        s.target = 1.0;
        for _ in 0..200 {
            s.step(1.0 / 120.0);
            assert!(s.value <= 1.0 + 1e-9);
        }
    }

    #[test]
    fn huge_frame_gaps_stay_stable() {
        let mut s = Spring::new(0.0);
        s.target = 100.0;
        s.step(5.0);
        assert!(s.value.is_finite() && s.value.abs() < 1000.0);
    }
}
