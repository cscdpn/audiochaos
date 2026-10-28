/// Time constant of the knob smoothing. Short enough to feel instant, long
/// enough to remove the click of a sudden jump.
const TIME_CONSTANT_SECONDS: f32 = 0.005;

/// Below this distance (in normalized knob travel) a glide jumps to its end.
/// f32 rounding stalls the glide once `gap * coeff` drops under half an ulp,
/// around 3e-5 at 96 kHz; 1e-4 clears that and is inaudible (under 2 cents
/// on the widest frequency knob).
const SNAP_DISTANCE: f32 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Linear,
    /// Equal knob travel multiplies the value by the same factor, which is how
    /// we hear frequencies and times. Requires `min > 0`.
    Exponential,
}

/// A knob: its range and how knob travel maps onto that range.
///
/// Knob positions and CV live in a normalized 0..1 space, so a ±1 CV sweeps
/// the whole range of any parameter, whatever its units.
pub struct ParamInfo {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub scale: Scale,
    pub unit: &'static str,
}

impl ParamInfo {
    pub fn to_normalized(&self, value: f32) -> f32 {
        let n = match self.scale {
            Scale::Linear => (value - self.min) / (self.max - self.min),
            Scale::Exponential => (value / self.min).ln() / (self.max / self.min).ln(),
        };
        n.clamp(0.0, 1.0)
    }

    /// Out-of-range positions are clamped, so heavy modulation saturates
    /// instead of producing nonsense.
    pub fn from_normalized(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        match self.scale {
            Scale::Linear => self.min + n * (self.max - self.min),
            Scale::Exponential => self.min * (self.max / self.min).powf(n),
        }
    }
}

/// A value that glides towards its target with a one-pole low-pass, so that
/// changes coming from the UI never produce clicks.
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

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Advances by one sample and returns the new value.
    pub fn tick(&mut self) -> f32 {
        let gap = self.target - self.value;
        if gap.abs() < SNAP_DISTANCE {
            self.value = self.target;
        } else {
            self.value += gap * self.coeff;
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn param(scale: Scale, min: f32, max: f32) -> ParamInfo {
        ParamInfo {
            name: "p",
            min,
            max,
            default: min,
            scale,
            unit: "",
        }
    }

    #[test]
    fn linear_mapping_is_proportional() {
        let p = param(Scale::Linear, -1.0, 3.0);
        assert_eq!(p.from_normalized(0.5), 1.0);
        assert_eq!(p.to_normalized(1.0), 0.5);
    }

    #[test]
    fn exponential_mapping_puts_the_geometric_mean_in_the_middle() {
        let p = param(Scale::Exponential, 10.0, 1000.0);
        assert!((p.from_normalized(0.5) - 100.0).abs() < 1e-3);
        assert!((p.to_normalized(100.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mapping_round_trips() {
        for scale in [Scale::Linear, Scale::Exponential] {
            let p = param(scale, 0.5, 20.0);
            for v in [0.5, 1.0, 7.0, 20.0] {
                assert!((p.from_normalized(p.to_normalized(v)) - v).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let p = param(Scale::Exponential, 1.0, 100.0);
        assert_eq!(p.from_normalized(2.0), 100.0);
        assert_eq!(p.from_normalized(-1.0), 1.0);
        assert_eq!(p.to_normalized(1e9), 1.0);
        assert_eq!(p.to_normalized(0.0), 0.0);
    }

    #[test]
    fn smoothed_holds_its_initial_value() {
        let mut s = Smoothed::new(3.0, SR);
        assert!((0..1000).all(|_| s.tick() == 3.0));
    }

    #[test]
    fn smoothed_does_not_jump_to_the_target() {
        let mut s = Smoothed::new(0.0, SR);
        s.set(1.0);
        assert!(s.tick() < 0.01);
    }

    #[test]
    fn smoothed_reaches_the_target_within_ten_time_constants() {
        let mut s = Smoothed::new(0.0, SR);
        s.set(1.0);
        let samples = (10.0 * TIME_CONSTANT_SECONDS * SR) as usize;
        let last = (0..samples).map(|_| s.tick()).last().unwrap();
        assert!((last - 1.0).abs() < 1e-3);
    }

    #[test]
    fn smoothed_lands_exactly_on_the_target() {
        let mut s = Smoothed::new(0.0, SR);
        s.set(0.55);
        let last = (0..SR as usize).map(|_| s.tick()).last().unwrap();
        assert_eq!(last, 0.55);
    }
}
