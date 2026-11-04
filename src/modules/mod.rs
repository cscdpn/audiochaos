//! The module library. Each module exposes its port and parameter indices as
//! constants, so patches read as `oscillator::OUT_SAW` rather than `1`.

pub mod adsr;
pub mod atten;
pub mod chorus;
pub mod clock;
pub mod crusher;
pub mod delay;
pub mod distortion;
mod dsp;
pub mod filter;
pub mod flanger;
pub mod input;
pub mod invert;
pub mod lfo;
pub mod logic;
pub mod looper;
pub mod master;
pub mod midi;
pub mod mixer;
pub mod noise;
pub mod oscillator;
pub mod phaser;
pub mod poly;
pub mod sample_hold;
pub mod sampler;
pub mod sequencer;
pub mod univibe;
pub mod vca;

use std::sync::Arc;

use crate::engine::input::LiveInput;
use crate::engine::module::{Module, ModuleInfo};
use crate::engine::transport::Transport;
use crate::midi::State;

/// What building a module takes besides its kind.
#[derive(Clone)]
pub struct Context {
    pub sample_rate: f32,
    /// What the MIDI controllers play; `midi` modules read it.
    pub midi: Arc<State>,
    /// What the sound card's inputs play; `input` modules read it.
    pub input: Arc<LiveInput>,
    /// The DAW's tempo and position, in the plugin; `clock` modules follow
    /// it.
    pub transport: Arc<Transport>,
}

impl Context {
    /// With no MIDI controller playing, and silent inputs.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            midi: Arc::default(),
            input: Arc::default(),
            transport: Arc::default(),
        }
    }
}

/// A kind of module: what it is, and how to build one. `seed` makes each
/// noise source different, yet reproducible.
pub struct Kind {
    pub info: &'static ModuleInfo,
    build: fn(context: &Context, seed: u32) -> Box<dyn Module>,
}

/// Every kind of module, in the editor's palette order. Adding a module is
/// adding it here.
pub const KINDS: [Kind; 26] = [
    Kind {
        info: &input::INFO,
        build: |c, _| Box::new(input::Input::new(Arc::clone(&c.input))),
    },
    Kind {
        info: &midi::INFO,
        build: |c, _| Box::new(midi::Midi::new(Arc::clone(&c.midi), c.sample_rate)),
    },
    Kind {
        info: &poly::INFO,
        build: |c, _| Box::new(poly::Poly::new(Arc::clone(&c.midi), c.sample_rate)),
    },
    Kind {
        info: &clock::INFO,
        build: |c, _| Box::new(clock::Clock::new(c.sample_rate, Arc::clone(&c.transport))),
    },
    Kind {
        info: &sequencer::INFO,
        build: |c, _| Box::new(sequencer::Sequencer::new(c.sample_rate)),
    },
    Kind {
        info: &lfo::INFO,
        build: |c, _| Box::new(lfo::Lfo::new(c.sample_rate)),
    },
    Kind {
        info: &adsr::INFO,
        build: |c, _| Box::new(adsr::Adsr::new(c.sample_rate)),
    },
    Kind {
        info: &noise::INFO,
        build: |_, seed| Box::new(noise::Noise::new(seed)),
    },
    Kind {
        info: &sample_hold::INFO,
        build: |_, _| Box::new(sample_hold::SampleHold::new()),
    },
    Kind {
        info: &sampler::INFO,
        build: |c, _| Box::new(sampler::Sampler::new(c.sample_rate)),
    },
    Kind {
        info: &looper::INFO,
        build: |c, _| Box::new(looper::Looper::new(c.sample_rate)),
    },
    Kind {
        info: &atten::INFO,
        build: |_, _| Box::new(atten::Atten),
    },
    Kind {
        info: &invert::INFO,
        build: |_, _| Box::new(invert::Invert),
    },
    Kind {
        info: &logic::INFO,
        build: |_, _| Box::new(logic::Logic::default()),
    },
    Kind {
        info: &oscillator::INFO,
        build: |c, _| Box::new(oscillator::Oscillator::new(c.sample_rate)),
    },
    Kind {
        info: &filter::INFO,
        build: |c, _| Box::new(filter::Filter::new(c.sample_rate)),
    },
    Kind {
        info: &distortion::INFO,
        build: |_, _| Box::new(distortion::Distortion),
    },
    Kind {
        info: &crusher::INFO,
        build: |_, _| Box::new(crusher::Crusher::new()),
    },
    Kind {
        info: &delay::INFO,
        build: |c, _| Box::new(delay::Delay::new(c.sample_rate)),
    },
    Kind {
        info: &chorus::INFO,
        build: |c, _| Box::new(chorus::Chorus::new(c.sample_rate)),
    },
    Kind {
        info: &flanger::INFO,
        build: |c, _| Box::new(flanger::Flanger::new(c.sample_rate)),
    },
    Kind {
        info: &phaser::INFO,
        build: |c, _| Box::new(phaser::Phaser::new(c.sample_rate)),
    },
    Kind {
        info: &univibe::INFO,
        build: |c, _| Box::new(univibe::Univibe::new(c.sample_rate)),
    },
    Kind {
        info: &vca::INFO,
        build: |_, _| Box::new(vca::Vca),
    },
    Kind {
        info: &mixer::INFO,
        build: |_, _| Box::new(mixer::Mixer),
    },
    Kind {
        info: &master::INFO,
        build: |_, _| Box::new(master::Master),
    },
];

/// The descriptions of every kind of module, in palette order.
pub fn infos() -> impl Iterator<Item = &'static ModuleInfo> {
    KINDS.iter().map(|kind| kind.info)
}

/// A new module of the given kind, or `None` if no kind has that name.
/// `seed` makes each noise source different, yet reproducible.
pub fn build(kind: &str, context: &Context, seed: u32) -> Option<Box<dyn Module>> {
    let kind = KINDS.iter().find(|k| k.info.name == kind)?;
    Some((kind.build)(context, seed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::graph::MAX_PARAMS;

    #[test]
    fn every_kind_builds_a_module_of_that_kind() {
        for info in infos() {
            let module = build(info.name, &Context::new(48_000.0), 1).unwrap();
            assert_eq!(module.info().name, info.name);
        }
        assert!(build("theremin", &Context::new(48_000.0), 1).is_none());
    }

    #[test]
    fn no_two_kinds_share_a_name() {
        let mut names: Vec<_> = infos().map(|info| info.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KINDS.len());
    }

    #[test]
    fn every_module_and_knob_says_what_it_does() {
        for info in infos() {
            assert!(!info.description.is_empty(), "{}", info.name);
            for param in info.params {
                assert!(!param.help.is_empty(), "{} {}", info.name, param.name);
            }
        }
    }

    #[test]
    fn every_knob_fits_in_the_table_of_modulated_values() {
        for info in infos() {
            assert!(info.params.len() <= MAX_PARAMS, "{}", info.name);
        }
    }

    #[test]
    fn every_output_has_details() {
        for info in infos() {
            assert_eq!(
                info.output_details.len(),
                info.outputs.len(),
                "{}",
                info.name
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use crate::engine::module::{BLOCK, Buffer, Module};

    /// Runs a module on constant inputs and parameters for `blocks` blocks
    /// and returns one output port, concatenated.
    pub fn run(
        module: &mut dyn Module,
        inputs: &[f32],
        params: &[f32],
        port: usize,
        blocks: usize,
    ) -> Vec<f32> {
        let inputs: Vec<Buffer> = inputs.iter().map(|&v| [v; BLOCK]).collect();
        run_signal(module, &inputs, params, port, blocks)
    }

    /// Like [`run`], with arbitrary input blocks repeated every block.
    pub fn run_signal(
        module: &mut dyn Module,
        inputs: &[Buffer],
        params: &[f32],
        port: usize,
        blocks: usize,
    ) -> Vec<f32> {
        let info = module.info();
        assert_eq!(inputs.len(), info.inputs.len());
        assert_eq!(params.len(), info.params.len());
        let params: Vec<Buffer> = params.iter().map(|&v| [v; BLOCK]).collect();
        let mut outputs = vec![[0.0; BLOCK]; info.outputs.len()];
        let mut out = Vec::with_capacity(blocks * BLOCK);
        for _ in 0..blocks {
            module.process(inputs, &params, &mut outputs);
            out.extend_from_slice(&outputs[port]);
        }
        out
    }

    pub fn rising_zero_crossings(signal: &[f32]) -> usize {
        signal
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    pub fn rms(signal: &[f32]) -> f32 {
        (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt()
    }

    /// Feeds a whole signal through a module's first input, its other inputs
    /// silent.
    pub fn process_signal(
        module: &mut dyn Module,
        signal: &[f32],
        params: &[f32],
        port: usize,
    ) -> Vec<f32> {
        let params: Vec<Buffer> = params.iter().map(|&v| [v; BLOCK]).collect();
        let mut inputs = vec![[0.0; BLOCK]; module.info().inputs.len()];
        let mut outputs = vec![[0.0; BLOCK]; module.info().outputs.len()];
        let mut out = Vec::with_capacity(signal.len());
        for chunk in signal.as_chunks::<BLOCK>().0 {
            inputs[0] = *chunk;
            module.process(&inputs, &params, &mut outputs);
            out.extend_from_slice(&outputs[port]);
        }
        out
    }

    /// The level of a quiet sine after a module, relative to the level going
    /// in, once the module has settled. Quiet, to stay clear of `tanh`.
    pub fn gain_at(module: &mut dyn Module, freq: f32, params: &[f32], port: usize) -> f32 {
        const LEVEL: f32 = 0.1;
        let tone: Vec<f32> = sine(freq, 48_000.0, 48_000)
            .iter()
            .map(|s| s * LEVEL)
            .collect();
        let out = process_signal(module, &tone, params, port);
        rms(&out[out.len() / 2..]) / (LEVEL / std::f32::consts::SQRT_2)
    }

    /// How many times the level of a steady sine rises through its average
    /// over ten seconds after a module: how often the module moves the sound.
    /// `freq` should be a multiple of 100 Hz, so that each 10 ms window holds
    /// whole periods and only the module makes the level change.
    pub fn wobbles(module: &mut dyn Module, freq: f32, params: &[f32], port: usize) -> usize {
        let out = process_signal(module, &sine(freq, 48_000.0, 480_000), params, port);
        let levels: Vec<f32> = out.as_chunks::<480>().0.iter().map(|w| rms(w)).collect();
        let average = levels.iter().sum::<f32>() / levels.len() as f32;
        // Not counting rounding noise around a level that does not move.
        let quiet = 0.01 * average;
        let mut above = levels[0] > average;
        let mut count = 0;
        for level in levels {
            if !above && level > average + quiet {
                count += 1;
                above = true;
            } else if above && level < average - quiet {
                above = false;
            }
        }
        count
    }

    pub fn sine(freq: f32, sample_rate: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / sample_rate).sin())
            .collect()
    }
}
