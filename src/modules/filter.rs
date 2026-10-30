use std::f32::consts::PI;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN: usize = 0;

pub const OUT_LOWPASS: usize = 0;
pub const OUT_BANDPASS: usize = 1;
pub const OUT_HIGHPASS: usize = 2;

pub const CUTOFF: usize = 0;
pub const RESONANCE: usize = 1;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "filter",
    inputs: &["in"],
    outputs: &["lowpass", "bandpass", "highpass"],
    params: &[
        ParamInfo {
            name: "cutoff",
            min: 20.0,
            max: 18_000.0,
            default: 1000.0,
            scale: Scale::Exponential,
            unit: "Hz",
        },
        ParamInfo {
            name: "resonance",
            min: 0.0,
            max: 1.0,
            default: 0.2,
            scale: Scale::Linear,
            unit: "",
        },
    ],
};

/// Lowest damping, reached at full resonance: the filter rings almost forever
/// and whistles on its own, but stays stable.
const MIN_DAMPING: f32 = 0.01;

/// State-variable filter in its trapezoidal (TPT) form, after Andrew Simper's
/// "Linear Trajectory SVF". Unlike the classic Chamberlin SVF it stays stable
/// at any cutoff and resonance, which matters when both are modulated hard.
pub struct Filter {
    sample_rate: f32,
    ic1eq: f32,
    ic2eq: f32,
}

impl Filter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            ic1eq: 0.0,
            ic2eq: 0.0,
        }
    }
}

impl Module for Filter {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [low, band, high] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        let nyquist_guard = 0.49 * self.sample_rate;
        for (i, v0) in inputs[IN].iter().enumerate() {
            let cutoff = params[CUTOFF][i].min(nyquist_guard);
            let g = (PI * cutoff / self.sample_rate).tan();
            let k = 2.0 * (1.0 - params[RESONANCE][i]).max(MIN_DAMPING);
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;
            let v3 = v0 - self.ic2eq;
            let v1 = a1 * self.ic1eq + a2 * v3;
            let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
            self.ic1eq = 2.0 * v1 - self.ic1eq;
            self.ic2eq = 2.0 * v2 - self.ic2eq;
            low[i] = v2;
            // Scaled by k so the band peaks at unity whatever the resonance.
            band[i] = k * v1;
            high[i] = v0 - k * v1 - v2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::noise::Noise;
    use crate::modules::test_util::{process_signal, rms, run, sine};

    const SR: f32 = 48_000.0;

    fn gain(freq: f32, port: usize, cutoff: f32) -> f32 {
        let input = sine(freq, SR, 48_000);
        let out = process_signal(&mut Filter::new(SR), &input, &[cutoff, 0.0], port);
        // Skip the first half, where the filter settles.
        rms(&out[24_000..]) / rms(&input[24_000..])
    }

    #[test]
    fn lowpass_keeps_lows_and_cuts_highs() {
        assert!(gain(100.0, OUT_LOWPASS, 1000.0) > 0.95);
        assert!(gain(10_000.0, OUT_LOWPASS, 1000.0) < 0.02);
    }

    #[test]
    fn highpass_keeps_highs_and_cuts_lows() {
        assert!(gain(10_000.0, OUT_HIGHPASS, 1000.0) > 0.95);
        assert!(gain(100.0, OUT_HIGHPASS, 1000.0) < 0.02);
    }

    #[test]
    fn bandpass_peaks_at_the_cutoff() {
        assert!(gain(1000.0, OUT_BANDPASS, 1000.0) > 0.9);
        // At zero resonance the band is wide (Q = 0.5): about -14 dB a decade away.
        assert!(gain(100.0, OUT_BANDPASS, 1000.0) < 0.25);
        assert!(gain(10_000.0, OUT_BANDPASS, 1000.0) < 0.25);
    }

    #[test]
    fn stays_finite_at_full_resonance_and_extreme_cutoff() {
        let noise = run(&mut Noise::new(7), &[], &[], 0, 1000);
        for cutoff in [20.0, 1000.0, 18_000.0, 1e6] {
            let out = process_signal(&mut Filter::new(SR), &noise, &[cutoff, 1.0], OUT_LOWPASS);
            assert!(out.iter().all(|s| s.is_finite()), "cutoff {cutoff}");
        }
    }
}
