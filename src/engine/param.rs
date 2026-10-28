/// Time constant of the smoothing. Short enough to feel instant when turning
/// a knob, long enough to remove the click of a sudden jump.
const TIME_CONSTANT_SECONDS: f32 = 0.005;

/// A parameter that glides towards its target with a one-pole low-pass,
/// so that changes coming from the UI never produce clicks.
pub struct Smoothed {
    value: f32,
    target: f32,
    coeff: f32,
}

impl Smoothed {
    pub fn new(value: f32, sample_rate: f32) -> Self {
        Self {
            value,
            target: value,
            coeff: 1.0 - (-1.0 / (TIME_CONSTANT_SECONDS * sample_rate)).exp(),
        }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Advances by one sample and returns the new value.
    pub fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coeff;
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn holds_its_initial_value() {
        let mut p = Smoothed::new(3.0, SR);
        assert!((0..1000).all(|_| p.tick() == 3.0));
    }

    #[test]
    fn does_not_jump_to_the_target() {
        let mut p = Smoothed::new(0.0, SR);
        p.set(1.0);
        assert!(p.tick() < 0.01);
    }

    #[test]
    fn reaches_the_target_within_ten_time_constants() {
        let mut p = Smoothed::new(0.0, SR);
        p.set(1.0);
        let samples = (10.0 * TIME_CONSTANT_SECONDS * SR) as usize;
        let last = (0..samples).map(|_| p.tick()).last().unwrap();
        assert!((last - 1.0).abs() < 1e-3);
    }
}
