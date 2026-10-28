//! The fixed set of modules the TUI patches together, and the patch it starts
//! with.

use crate::engine::graph::{Cable, Graph, GraphError, NodeId};
use crate::modules::{
    crusher::Crusher, delay::Delay, distortion::Distortion, filter, filter::Filter, lfo::Lfo,
    master, master::Master, noise::Noise, oscillator, oscillator::Oscillator,
    sample_hold::SampleHold, sequencer, sequencer::Sequencer, vca, vca::Vca,
};

/// One octave, in normalized travel of the oscillator's frequency knob
/// (four decades wide).
const OCTAVE: f32 = 0.0753;

/// Builds the rack and returns it with the node that feeds the speakers.
///
/// Modules are ordered from sources to the master: a cable going backwards in
/// this order is a feedback cable and costs one block of delay.
pub fn build(sample_rate: f32) -> Result<(Graph, NodeId), GraphError> {
    let sr = sample_rate;
    let mut g = Graph::new(sr);
    let seq = g.add("seq", Box::new(Sequencer::new(sr)));
    g.add("lfo", Box::new(Lfo::new(sr)));
    g.add("noise", Box::new(Noise::new(0x5eed)));
    g.add("s&h", Box::new(SampleHold::new()));
    let osc1 = g.add("osc 1", Box::new(Oscillator::new(sr)));
    g.add("osc 2", Box::new(Oscillator::new(sr)));
    let filt = g.add("filter", Box::new(Filter::new(sr)));
    g.add("dist", Box::new(Distortion));
    g.add("crush", Box::new(Crusher::new()));
    g.add("delay", Box::new(Delay::new(sr)));
    let amp = g.add("vca", Box::new(Vca));
    let out = g.add("master", Box::new(Master));

    // A plain sequenced bassline to start from; the patching is up to you.
    let melody = [
        0.0,
        0.0,
        OCTAVE,
        0.0,
        0.58 * OCTAVE,
        0.0,
        OCTAVE,
        1.25 * OCTAVE,
    ];
    for (n, value) in melody.into_iter().enumerate() {
        g.set_param(seq, sequencer::step(n), value);
    }
    g.set_param(osc1, oscillator::FREQ, 55.0);
    g.set_param(filt, filter::CUTOFF, 600.0);
    g.set_param(filt, filter::RESONANCE, 0.6);
    g.set_param(amp, vca::GAIN, 0.0);

    let cables = [
        (
            seq,
            sequencer::OUT_CV,
            osc1,
            oscillator::INFO.cv_port(oscillator::FREQ),
        ),
        (osc1, oscillator::OUT_SAW, filt, filter::IN),
        (filt, filter::OUT_LOWPASS, amp, vca::IN_SIGNAL),
        (seq, sequencer::OUT_GATE, amp, vca::IN_MOD),
        (amp, vca::OUT, out, master::IN),
    ];
    for (from, from_port, to, to_port) in cables {
        g.connect(Cable {
            from,
            from_port,
            to,
            to_port,
        })?;
    }
    Ok((g, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_patch_makes_sound() {
        let (mut g, out) = build(48_000.0).unwrap();
        let audio = g.render(out, master::OUT, 48_000);
        let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
        assert!(rms > 0.05, "rms {rms}");
    }

    #[test]
    fn octave_constant_matches_the_frequency_knob() {
        let freq = &oscillator::INFO.params[oscillator::FREQ];
        let n = freq.to_normalized(110.0);
        assert!((freq.from_normalized(n + OCTAVE) - 220.0).abs() < 0.1);
    }
}
