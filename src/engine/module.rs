/// Number of samples processed per call. Small enough for low latency,
/// large enough to amortise the per-call overhead.
pub const BLOCK: usize = 64;

pub type Buffer = [f32; BLOCK];

/// A signal-processing brick. Audio and control signals share the same
/// `f32` buffers, as in a modular synth where everything is a voltage.
pub trait Module: Send {
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;

    /// Must not allocate or block: it runs on the audio thread.
    /// An input with no cable plugged in is a buffer of zeros; several cables
    /// into one input are summed by the graph.
    fn process(&mut self, inputs: &[Buffer], outputs: &mut [Buffer]);
}
