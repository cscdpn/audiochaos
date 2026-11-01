use std::f32::consts::TAU;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::{Allpass, allpass_coefficient};

pub const IN: usize = 0;

pub const OUT: usize = 0;

pub const SPEED: usize = 0;
pub const INTENSITY: usize = 1;
pub const MIX: usize = 2;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "univibe",
    inputs: &["in"],
    outputs: &["out"],
    output_details: &["input + throb"],
    params: &[
        ParamInfo {
            name: "speed",
            min: 0.1,
            max: 15.0,
            default: 3.0,
            scale: Scale::Exponential,
            unit: "Hz",
        },
        ParamInfo {
            name: "intensity",
            min: 0.0,
            max: 1.0,
            default: 0.7,
            scale: Scale::Linear,
            unit: "",
        },
        ParamInfo {
            name: "mix",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            scale: Scale::Linear,
            unit: "",
        },
    ],
};

/// Staggered, unlike a phaser's stages, which is what spreads the notches and
/// keeps the sound from whooshing. Chosen by ear, not taken from a schematic.
const STAGE_FREQS: [f32; 4] = [80.0, 300.0, 1000.0, 3000.0];
/// How far full intensity moves the stages, either way.
const SWEEP_OCTAVES: f32 = 2.0;
/// A filament heats up faster than it cools down.
const LAMP_RISE_SECONDS: f32 = 0.02;
const LAMP_FALL_SECONDS: f32 = 0.06;

/// After the Uni-Vibe, where a pulsing lamp drives the phase stages through
/// light sensors: same idea as a phaser, but the lamp's lag gives the sweep its
/// lopsided throb. Set `mix` to 1 for pure vibrato, as the pedal's switch does.
pub struct Univibe {
    sample_rate: f32,
    stages: [Allpass; STAGE_FREQS.len()],
    lamp: f32,
    rise: f32,
    fall: f32,
    phase: f32,
}

impl Univibe {
    pub fn new(sample_rate: f32) -> Self {
        let pole = |seconds: f32| 1.0 - (-1.0 / (seconds * sample_rate)).exp();
        Self {
            sample_rate,
            stages: [Allpass::default(); STAGE_FREQS.len()],
            lamp: 0.0,
            rise: pole(LAMP_RISE_SECONDS),
            fall: pole(LAMP_FALL_SECONDS),
            phase: 0.0,
        }
    }

    /// The lamp's brightness, 0..1, after one more sample.
    fn tick_lamp(&mut self, speed: f32) -> f32 {
        // Brightness grows faster than the voltage, so the pulses are narrow.
        let drive = 0.5 + 0.5 * (TAU * self.phase).sin();
        let target = drive * drive;
        let pole = if target > self.lamp {
            self.rise
        } else {
            self.fall
        };
        self.lamp += (target - self.lamp) * pole;
        self.phase = (self.phase + speed / self.sample_rate).fract();
        self.lamp
    }
}

impl Module for Univibe {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, x) in inputs[IN].iter().enumerate() {
            let lamp = self.tick_lamp(params[SPEED][i]);
            let octaves = SWEEP_OCTAVES * params[INTENSITY][i] * (2.0 * lamp - 1.0);
            let shift = octaves.exp2();
            let mut wet = *x;
            for (stage, freq) in self.stages.iter_mut().zip(STAGE_FREQS) {
                let coefficient = allpass_coefficient(freq * shift, self.sample_rate);
                wet = stage.process(wet, coefficient);
            }
            let mix = params[MIX][i];
            outputs[OUT][i] = (1.0 - mix) * x + mix * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{gain_at, process_signal, sine, wobbles};

    const SR: f32 = 48_000.0;

    #[test]
    fn the_lamp_rises_faster_than_it_falls() {
        let mut univibe = Univibe::new(SR);
        // One second at 2 Hz, then the time spent rising and falling over the
        // next full cycle.
        (0..SR as usize).for_each(|_| {
            univibe.tick_lamp(2.0);
        });
        let lamps: Vec<f32> = (0..SR as usize / 2)
            .map(|_| univibe.tick_lamp(2.0))
            .collect();
        let rising = lamps.windows(2).filter(|w| w[1] > w[0]).count();
        assert!(
            rising * 2 < lamps.len(),
            "rising {rising} of {}",
            lamps.len()
        );
        assert!(lamps.iter().all(|l| (0.0..=1.0).contains(l)));
    }

    #[test]
    fn the_wet_signal_alone_keeps_its_level() {
        for freq in [100.0, 1000.0, 8000.0] {
            let gain = gain_at(&mut Univibe::new(SR), freq, &[3.0, 1.0, 1.0], OUT);
            assert!((gain - 1.0).abs() < 0.05, "{gain} at {freq} Hz");
        }
    }

    #[test]
    fn mix_zero_is_the_dry_input() {
        let dry = sine(440.0, SR, 4800);
        let out = process_signal(&mut Univibe::new(SR), &dry, &[3.0, 1.0, 0.0], OUT);
        assert_eq!(out, dry[..out.len()]);
    }

    #[test]
    fn without_intensity_nothing_moves() {
        let count = wobbles(&mut Univibe::new(SR), 1000.0, &[3.0, 0.0, 0.5], OUT);
        assert!(count <= 2, "{count}");
    }

    #[test]
    fn speed_sets_how_fast_the_sound_throbs() {
        let count = |speed| wobbles(&mut Univibe::new(SR), 1000.0, &[speed, 0.7, 0.5], OUT);
        let (slow, fast) = (count(1.0), count(2.0));
        assert!(slow >= 5, "{slow}");
        assert!(fast.abs_diff(2 * slow) <= 3, "{slow}, {fast}");
    }
}
