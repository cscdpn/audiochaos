//! The module library. Each module exposes its port and parameter indices as
//! constants, so patches read as `oscillator::OUT_SAW` rather than `1`.

pub mod crusher;
pub mod delay;
pub mod distortion;
pub mod filter;
pub mod lfo;
pub mod master;
pub mod noise;
pub mod oscillator;
pub mod sample_hold;
pub mod sequencer;
pub mod vca;

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
