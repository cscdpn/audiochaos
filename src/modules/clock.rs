use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::modules::dsp::flash;

pub const OUT_BAR: usize = 0;
pub const OUT_BEAT: usize = 1;
pub const OUT_EIGHTH: usize = 2;
pub const OUT_DOTTED_EIGHTH: usize = 3;
pub const OUT_SIXTEENTH: usize = 4;

pub const TEMPO: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "clock",
    inputs: &[],
    outputs: &["bar", "beat", "8th", "dotted 8th", "16th"],
    output_details: &[
        "gate, 4 beats",
        "gate",
        "gate, ½ beat",
        "gate, ¾ beat",
        "gate, ¼ beat",
    ],
    params: &[ParamInfo {
        name: "tempo",
        min: 20.0,
        max: 300.0,
        default: 120.0,
        scale: Scale::Linear,
        unit: "BPM",
    }],
};

/// Each output's period, in beats.
const PERIODS: [f64; 5] = [4.0, 1.0, 0.5, 0.75, 0.25];
/// Beats after which every output starts a period at once.
const CYCLE: f64 = 12.0;

/// A tempo as gates, high for the first half of each period, to drive the
/// `clock` inputs of delays, LFOs and sequencers, or anything triggered. The
/// editor's Tap button sets the tempo.
pub struct Clock {
    sample_rate: f32,
    /// In f64: in f32, adding a sample's worth of beats would lose about a
    /// percent to rounding, and the tempo would drift.
    beats: f64,
}

impl Clock {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            beats: 0.0,
        }
    }
}

impl Module for Clock {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, &tempo) in params[TEMPO].iter().enumerate() {
            for (out, period) in outputs.iter_mut().zip(PERIODS) {
                out[i] = if self.beats % period < period / 2.0 {
                    1.0
                } else {
                    0.0
                };
            }
            let step = f64::from(tempo) / 60.0 / f64::from(self.sample_rate);
            self.beats = (self.beats + step) % CYCLE;
        }
    }

    fn led(&self) -> Option<f32> {
        Some(flash(self.beats.fract() as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::run;

    const SR: f32 = 48_000.0;

    fn rising_edges(signal: &[f32]) -> Vec<usize> {
        (1..signal.len())
            .filter(|&n| signal[n - 1] <= 0.0 && signal[n] > 0.0)
            .collect()
    }

    #[test]
    fn each_output_ticks_at_its_division_of_the_tempo() {
        // 120 BPM: a beat every 24,000 samples. Twelve seconds, a whole cycle.
        for (port, beats) in [
            (OUT_BAR, 4.0),
            (OUT_BEAT, 1.0),
            (OUT_EIGHTH, 0.5),
            (OUT_DOTTED_EIGHTH, 0.75),
            (OUT_SIXTEENTH, 0.25),
        ] {
            let out = run(
                &mut Clock::new(SR),
                &[],
                &[120.0],
                port,
                12 * 48_000 / BLOCK,
            );
            let edges = rising_edges(&out);
            let period = (24_000.0 * beats) as usize;
            for pair in edges.windows(2) {
                assert!((pair[1] - pair[0]).abs_diff(period) <= 1, "port {port}");
            }
            assert_eq!(edges.len(), (24.0 / beats) as usize - 1, "port {port}");
        }
    }

    #[test]
    fn gates_are_high_for_half_their_period() {
        let out = run(
            &mut Clock::new(SR),
            &[],
            &[120.0],
            OUT_BEAT,
            2 * 48_000 / BLOCK,
        );
        let high = out.iter().filter(|&&s| s == 1.0).count();
        assert!(high.abs_diff(out.len() / 2) <= 2, "{high}");
    }

    #[test]
    fn the_light_flashes_on_each_beat() {
        let mut clock = Clock::new(SR);
        // Just past a beat, then just before the next one.
        run(&mut clock, &[], &[120.0], OUT_BEAT, 24_000 / BLOCK + 1);
        assert!(clock.led().unwrap() > 0.9);
        run(&mut clock, &[], &[120.0], OUT_BEAT, 24_000 / BLOCK - 2);
        assert!(clock.led().unwrap() < 0.1);
    }
}
