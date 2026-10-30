use super::param::ParamInfo;

/// Number of samples processed per call. Small enough for low latency,
/// large enough to amortise the per-call overhead.
pub const BLOCK: usize = 64;

pub type Buffer = [f32; BLOCK];

/// Static description of a module type: what can be patched and turned.
pub struct ModuleInfo {
    /// The kind of module, as patches name it, and the default name of its
    /// instances.
    pub name: &'static str,
    pub inputs: &'static [&'static str],
    pub outputs: &'static [&'static str],
    pub params: &'static [ParamInfo],
}

impl ModuleInfo {
    /// Every parameter also gets a CV input, numbered after the signal inputs.
    pub fn cv_port(&self, param: usize) -> usize {
        self.inputs.len() + param
    }

    pub fn num_input_ports(&self) -> usize {
        self.inputs.len() + self.params.len()
    }

    pub fn input_port_name(&self, port: usize) -> String {
        match self.inputs.get(port) {
            Some(name) => name.to_string(),
            None => format!("{} cv", self.params[port - self.inputs.len()].name),
        }
    }
}

/// A signal-processing brick. Audio and control signals share the same
/// `f32` buffers, as in a modular synth where everything is a voltage.
pub trait Module: Send {
    fn info(&self) -> &'static ModuleInfo;

    /// Must not allocate or block: it runs on the audio thread.
    ///
    /// `inputs` holds the signal inputs only (unplugged ones are zeros, several
    /// cables into one are summed). `params` holds one buffer per parameter,
    /// already smoothed and modulated by its CV input, in the parameter's units.
    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]);
}
