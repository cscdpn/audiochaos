//! The module library. Each module exposes its port and parameter indices as
//! constants, so patches read as `oscillator::OUT_SAW` rather than `1`.

pub mod master;
pub mod oscillator;
pub mod vca;

#[cfg(test)]
pub(crate) mod test_util {
    use crate::engine::module::{BLOCK, Buffer, Module};

    /// Runs a module on constant inputs for `blocks` blocks and returns one
    /// output port, concatenated.
    pub fn run(module: &mut dyn Module, inputs: &[f32], port: usize, blocks: usize) -> Vec<f32> {
        let inputs: Vec<Buffer> = inputs.iter().map(|&v| [v; BLOCK]).collect();
        let mut outputs = vec![[0.0; BLOCK]; module.num_outputs()];
        let mut out = Vec::with_capacity(blocks * BLOCK);
        for _ in 0..blocks {
            module.process(&inputs, &mut outputs);
            out.extend_from_slice(&outputs[port]);
        }
        out
    }
}
