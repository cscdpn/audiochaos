//! The fixed set of modules the UI patches together, and the presets it can
//! start from.

use crate::engine::graph::{Cable, Graph, GraphError, NodeId};
use crate::modules::{
    atten, atten::Atten, crusher, crusher::Crusher, delay, delay::Delay, distortion,
    distortion::Distortion, filter, filter::Filter, lfo, lfo::Lfo, master, master::Master, noise,
    noise::Noise, oscillator, oscillator::Oscillator, sample_hold, sample_hold::SampleHold,
    sequencer, sequencer::Sequencer, vca, vca::Vca,
};

/// One octave, in normalized travel of the oscillator's frequency knob
/// (four decades wide).
const OCTAVE: f32 = 0.0753;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Cross-modulated oscillators, folded, crushed and fed back through the
    /// delay: noisy, unstable and never quite repeating.
    Chaos,
    /// A plain sequenced bassline, a calm place to start patching from.
    Bass,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::Chaos, Preset::Bass];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Chaos => "chaos",
            Preset::Bass => "bass",
        }
    }

    pub fn from_name(name: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// The rack's modules, by role.
struct Rack {
    graph: Graph,
    seq: NodeId,
    lfo: NodeId,
    noise: NodeId,
    sh: NodeId,
    atten: NodeId,
    osc1: NodeId,
    osc2: NodeId,
    filter: NodeId,
    dist: NodeId,
    crush: NodeId,
    delay: NodeId,
    vca: NodeId,
    master: NodeId,
}

impl Rack {
    /// Modules are ordered from sources to the master: a cable going backwards
    /// in this order is a feedback cable and costs one block of delay.
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate;
        let mut g = Graph::new(sr);
        Self {
            seq: g.add("seq", Box::new(Sequencer::new(sr))),
            lfo: g.add("lfo", Box::new(Lfo::new(sr))),
            noise: g.add("noise", Box::new(Noise::new(0x5eed))),
            sh: g.add("s&h", Box::new(SampleHold::new())),
            atten: g.add("atten", Box::new(Atten)),
            osc1: g.add("osc 1", Box::new(Oscillator::new(sr))),
            osc2: g.add("osc 2", Box::new(Oscillator::new(sr))),
            filter: g.add("filter", Box::new(Filter::new(sr))),
            dist: g.add("dist", Box::new(Distortion)),
            crush: g.add("crush", Box::new(Crusher::new())),
            delay: g.add("delay", Box::new(Delay::new(sr))),
            vca: g.add("vca", Box::new(Vca)),
            master: g.add("master", Box::new(Master)),
            graph: g,
        }
    }

    fn set(&mut self, settings: &[(NodeId, usize, f32)]) {
        for &(node, param, value) in settings {
            self.graph.set_param(node, param, value);
        }
    }

    fn patch(&mut self, cables: &[(NodeId, usize, NodeId, usize)]) -> Result<(), GraphError> {
        for &(from, from_port, to, to_port) in cables {
            self.graph.connect(Cable {
                from,
                from_port,
                to,
                to_port,
            })?;
        }
        Ok(())
    }

    fn set_steps(&mut self, steps: [f32; sequencer::STEPS]) {
        for (n, value) in steps.into_iter().enumerate() {
            self.graph.set_param(self.seq, sequencer::step(n), value);
        }
    }

    fn bass(&mut self) -> Result<(), GraphError> {
        let o = OCTAVE;
        self.set_steps([0.0, 0.0, o, 0.0, 0.58 * o, 0.0, o, 1.25 * o]);
        self.set(&[
            (self.osc1, oscillator::FREQ, 55.0),
            (self.filter, filter::CUTOFF, 600.0),
            (self.filter, filter::RESONANCE, 0.6),
            (self.vca, vca::GAIN, 0.0),
        ]);
        let cv = |param| oscillator::INFO.cv_port(param);
        self.patch(&[
            (self.seq, sequencer::OUT_CV, self.osc1, cv(oscillator::FREQ)),
            (self.osc1, oscillator::OUT_SAW, self.filter, filter::IN),
            (self.filter, filter::OUT_LOWPASS, self.vca, vca::IN_SIGNAL),
            (self.seq, sequencer::OUT_GATE, self.vca, vca::IN_MOD),
            (self.vca, vca::OUT, self.master, master::IN),
        ])
    }

    /// Three intertwined loops make it chaotic: the two oscillators frequency-
    /// modulate each other, the delay output feeds back into osc 1's FM, and
    /// random voltages keep retuning osc 2, so the cross-modulation ratios
    /// never settle.
    fn chaos(&mut self) -> Result<(), GraphError> {
        self.set_steps([0.3, -0.2, 0.0, 0.5, -0.4, 0.2, 0.6, -0.1]);
        self.set(&[
            (self.seq, sequencer::RATE, 6.0),
            (self.lfo, lfo::RATE, 5.0),
            (self.atten, atten::A, 0.15),
            (self.atten, atten::B, 0.05),
            (self.osc1, oscillator::FREQ, 55.0),
            (self.osc1, oscillator::FM_DEPTH, 250.0),
            (self.osc2, oscillator::FREQ, 82.0),
            (self.osc2, oscillator::FM_DEPTH, 120.0),
            (self.dist, distortion::DRIVE, 10.0),
            (self.dist, distortion::FOLD, 0.6),
            (self.filter, filter::CUTOFF, 900.0),
            (self.filter, filter::RESONANCE, 0.8),
            (self.crush, crusher::BITS, 7.0),
            (self.crush, crusher::DOWNSAMPLE, 2.0),
            (self.delay, delay::TIME, 0.09),
            (self.delay, delay::FEEDBACK, 0.9),
            (self.delay, delay::MIX, 0.35),
            (self.vca, vca::GAIN, 0.25),
            (self.master, master::VOLUME, 0.6),
        ]);
        let osc_cv = |param| oscillator::INFO.cv_port(param);
        self.patch(&[
            // Random voltages, five times a second, gently retune osc 2.
            (self.noise, noise::OUT, self.sh, sample_hold::IN),
            (self.lfo, lfo::OUT_SQUARE, self.sh, sample_hold::IN_TRIGGER),
            (self.sh, sample_hold::OUT, self.atten, atten::A),
            (self.atten, atten::A, self.osc2, osc_cv(oscillator::FREQ)),
            // A slight wobble of the delay time bends the echoes' pitch.
            (self.lfo, lfo::OUT_SINE, self.atten, atten::B),
            (
                self.atten,
                atten::B,
                self.delay,
                delay::INFO.cv_port(delay::TIME),
            ),
            // The cross-FM and delay feedback loops.
            (
                self.osc2,
                oscillator::OUT_SINE,
                self.osc1,
                oscillator::IN_FM,
            ),
            (
                self.osc1,
                oscillator::OUT_SQUARE,
                self.osc2,
                oscillator::IN_FM,
            ),
            (self.delay, delay::OUT, self.osc1, oscillator::IN_FM),
            // The audio chain.
            (self.osc1, oscillator::OUT_SAW, self.dist, distortion::IN),
            (self.dist, distortion::OUT, self.filter, filter::IN),
            (self.filter, filter::OUT_LOWPASS, self.crush, crusher::IN),
            (self.crush, crusher::OUT, self.delay, delay::IN),
            (self.delay, delay::OUT, self.vca, vca::IN_SIGNAL),
            (self.vca, vca::OUT, self.master, master::IN),
            // Rhythm: the sequencer accents the drone and sweeps the filter.
            (self.seq, sequencer::OUT_GATE, self.vca, vca::IN_MOD),
            (
                self.seq,
                sequencer::OUT_CV,
                self.filter,
                filter::INFO.cv_port(filter::CUTOFF),
            ),
        ])
    }
}

/// Builds the rack with a preset and returns it with the node that feeds the
/// speakers.
pub fn build(sample_rate: f32, preset: Preset) -> Result<(Graph, NodeId), GraphError> {
    let mut rack = Rack::new(sample_rate);
    match preset {
        Preset::Chaos => rack.chaos()?,
        Preset::Bass => rack.bass()?,
    }
    Ok((rack.graph, rack.master))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::{rising_zero_crossings, rms};

    const SR: f32 = 48_000.0;

    fn render(preset: Preset, seconds: usize) -> Vec<f32> {
        let (mut g, out) = build(SR, preset).unwrap();
        g.render(out, master::OUT, seconds * SR as usize)
    }

    /// 100 ms windows.
    fn windows(audio: &[f32]) -> impl Iterator<Item = &[f32]> {
        audio.chunks_exact(SR as usize / 10)
    }

    fn spread(values: &[f32]) -> f32 {
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
        variance.sqrt() / mean
    }

    #[test]
    fn presets_are_found_by_name() {
        for preset in Preset::ALL {
            assert_eq!(Preset::from_name(preset.name()), Some(preset));
        }
        assert_eq!(Preset::from_name("nope"), None);
    }

    #[test]
    fn bass_makes_sound() {
        assert!(rms(&render(Preset::Bass, 1)) > 0.05);
    }

    #[test]
    fn chaos_is_loud_and_bounded() {
        let audio = render(Preset::Chaos, 10);
        assert!(audio.iter().all(|s| s.abs() <= 1.0));
        assert!(rms(&audio) > 0.2, "rms {}", rms(&audio));
    }

    #[test]
    fn chaos_keeps_changing() {
        let audio = render(Preset::Chaos, 10);
        // Loudness and brightness (zero crossings) both move from one 100 ms
        // window to the next, and the patch never falls silent.
        let loudness: Vec<f32> = windows(&audio).map(rms).collect();
        let brightness: Vec<f32> = windows(&audio)
            .map(|w| rising_zero_crossings(w) as f32)
            .collect();
        assert!(loudness.iter().all(|&l| l > 0.02), "a window went silent");
        assert!(
            spread(&loudness) > 0.1,
            "loudness spread {}",
            spread(&loudness)
        );
        assert!(
            spread(&brightness) > 0.1,
            "brightness spread {}",
            spread(&brightness)
        );
    }

    #[test]
    fn octave_constant_matches_the_frequency_knob() {
        let freq = &oscillator::INFO.params[oscillator::FREQ];
        let n = freq.to_normalized(110.0);
        assert!((freq.from_normalized(n + OCTAVE) - 220.0).abs() < 0.1);
    }
}
