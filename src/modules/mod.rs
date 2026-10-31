//! The module library. Each module exposes its port and parameter indices as
//! constants, so patches read as `oscillator::OUT_SAW` rather than `1`.

pub mod atten;
pub mod crusher;
pub mod delay;
pub mod distortion;
pub mod filter;
pub mod lfo;
pub mod master;
pub mod midi;
pub mod noise;
pub mod oscillator;
pub mod sample_hold;
pub mod sampler;
pub mod sequencer;
pub mod vca;

use std::sync::Arc;

use crate::engine::module::{Module, ModuleInfo};
use crate::midi::State;

/// What building a module takes besides its kind.
#[derive(Clone)]
pub struct Context {
    pub sample_rate: f32,
    /// What the MIDI controllers play; `midi` modules read it.
    pub midi: Arc<State>,
}

impl Context {
    /// With no MIDI controller playing.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            midi: Arc::default(),
        }
    }
}

/// Every kind of module, as patches and the editor's palette name them.
pub const KINDS: [&ModuleInfo; 14] = [
    &midi::INFO,
    &sequencer::INFO,
    &lfo::INFO,
    &noise::INFO,
    &sample_hold::INFO,
    &sampler::INFO,
    &atten::INFO,
    &oscillator::INFO,
    &filter::INFO,
    &distortion::INFO,
    &crusher::INFO,
    &delay::INFO,
    &vca::INFO,
    &master::INFO,
];

/// A new module of the given kind, or `None` if no kind has that name.
/// `seed` makes each noise source different, yet reproducible.
pub fn build(kind: &str, context: &Context, seed: u32) -> Option<Box<dyn Module>> {
    let sr = context.sample_rate;
    let module: Box<dyn Module> = match kind {
        "midi" => Box::new(midi::Midi::new(Arc::clone(&context.midi), sr)),
        "seq" => Box::new(sequencer::Sequencer::new(sr)),
        "lfo" => Box::new(lfo::Lfo::new(sr)),
        "noise" => Box::new(noise::Noise::new(seed)),
        "s&h" => Box::new(sample_hold::SampleHold::new()),
        "sampler" => Box::new(sampler::Sampler::new(sr)),
        "atten" => Box::new(atten::Atten),
        "osc" => Box::new(oscillator::Oscillator::new(sr)),
        "filter" => Box::new(filter::Filter::new(sr)),
        "dist" => Box::new(distortion::Distortion),
        "crush" => Box::new(crusher::Crusher::new()),
        "delay" => Box::new(delay::Delay::new(sr)),
        "vca" => Box::new(vca::Vca),
        "master" => Box::new(master::Master),
        _ => return None,
    };
    Some(module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_builds_a_module_of_that_kind() {
        for info in KINDS {
            let module = build(info.name, &Context::new(48_000.0), 1).unwrap();
            assert_eq!(module.info().name, info.name);
        }
        assert!(build("theremin", &Context::new(48_000.0), 1).is_none());
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

    /// Feeds a whole signal through a module with a single input.
    pub fn process_signal(
        module: &mut dyn Module,
        signal: &[f32],
        params: &[f32],
        port: usize,
    ) -> Vec<f32> {
        let params: Vec<Buffer> = params.iter().map(|&v| [v; BLOCK]).collect();
        let mut outputs = vec![[0.0; BLOCK]; module.info().outputs.len()];
        let mut out = Vec::with_capacity(signal.len());
        for chunk in signal.chunks_exact(BLOCK) {
            let input: Buffer = chunk.try_into().unwrap();
            module.process(&[input], &params, &mut outputs);
            out.extend_from_slice(&outputs[port]);
        }
        out
    }

    pub fn sine(freq: f32, sample_rate: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / sample_rate).sin())
            .collect()
    }
}
