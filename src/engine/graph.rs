use std::error::Error;
use std::fmt;

use super::module::{BLOCK, Buffer, Module, ModuleInfo};
use super::param::Smoothed;

/// Cables are stored in a vector allocated once with this capacity, so that
/// connecting from the audio thread never allocates.
pub const MAX_CABLES: usize = 256;

/// Every input is clamped to ±RAIL, like the power rails of a hardware
/// modular, and NaN reads as 0. Feedback loops through VCAs multiply without
/// bound, and a single non-finite sample reaching a filter would poison its
/// state forever; with the rails every module only ever sees finite input.
pub const RAIL: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(pub(crate) usize);

/// `to_port` counts signal inputs first, then one CV input per parameter
/// (see [`ModuleInfo::cv_port`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cable {
    pub from: NodeId,
    pub from_port: usize,
    pub to: NodeId,
    pub to_port: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GraphError {
    NoSuchPort,
    AlreadyConnected,
    TooManyCables,
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::NoSuchPort => write!(f, "no such port"),
            GraphError::AlreadyConnected => write!(f, "already connected"),
            GraphError::TooManyCables => write!(f, "too many cables (max {MAX_CABLES})"),
        }
    }
}

impl Error for GraphError {}

/// The set of cables, with the rules shared by the graph (audio side) and its
/// mirror in the controller (UI side), so both always agree.
pub struct CableSet(Vec<Cable>);

impl CableSet {
    pub fn new() -> Self {
        Self(Vec::with_capacity(MAX_CABLES))
    }

    pub fn insert(
        &mut self,
        cable: Cable,
        from: &ModuleInfo,
        to: &ModuleInfo,
    ) -> Result<(), GraphError> {
        if cable.from_port >= from.outputs.len() || cable.to_port >= to.num_input_ports() {
            Err(GraphError::NoSuchPort)
        } else if self.0.contains(&cable) {
            Err(GraphError::AlreadyConnected)
        } else if self.0.len() == MAX_CABLES {
            Err(GraphError::TooManyCables)
        } else {
            self.0.push(cable);
            Ok(())
        }
    }

    /// Returns whether the cable was there.
    pub fn remove(&mut self, cable: Cable) -> bool {
        match self.0.iter().position(|&c| c == cable) {
            Some(i) => {
                self.0.swap_remove(i);
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, cable: Cable) -> bool {
        self.0.contains(&cable)
    }

    pub fn as_slice(&self) -> &[Cable] {
        &self.0
    }
}

impl Default for CableSet {
    fn default() -> Self {
        Self::new()
    }
}

struct Node {
    name: String,
    module: Box<dyn Module>,
    /// Signal inputs followed by parameter CV inputs.
    inputs: Vec<Buffer>,
    /// Knob positions, normalized.
    knobs: Vec<Smoothed>,
    /// Knob values as set, in the parameters' units. Kept apart from the
    /// normalized positions so reading a knob back gives exactly what was set.
    settings: Vec<f32>,
    params: Vec<Buffer>,
    outputs: Vec<Buffer>,
}

/// Modules are processed in insertion order. A cable coming from a module
/// processed later (or from the module itself) reads that module's previous
/// block, so feedback loops are legal and get exactly one block of delay.
pub struct Graph {
    sample_rate: f32,
    nodes: Vec<Node>,
    cables: CableSet,
}

impl Graph {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            nodes: Vec::new(),
            cables: CableSet::new(),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn add(&mut self, name: &str, module: Box<dyn Module>) -> NodeId {
        let info = module.info();
        let knobs = info
            .params
            .iter()
            .map(|p| Smoothed::new(p.to_normalized(p.default), self.sample_rate))
            .collect();
        self.nodes.push(Node {
            name: name.to_string(),
            inputs: vec![[0.0; BLOCK]; info.num_input_ports()],
            knobs,
            settings: info.params.iter().map(|p| p.default).collect(),
            params: vec![[0.0; BLOCK]; info.params.len()],
            outputs: vec![[0.0; BLOCK]; info.outputs.len()],
            module,
        });
        NodeId(self.nodes.len() - 1)
    }

    pub fn connect(&mut self, cable: Cable) -> Result<(), GraphError> {
        let from = self.info(cable.from);
        let to = self.info(cable.to);
        self.cables.insert(cable, from, to)
    }

    pub fn disconnect(&mut self, cable: Cable) -> bool {
        self.cables.remove(cable)
    }

    /// Values outside the parameter's range are clamped.
    pub fn set_param(&mut self, node: NodeId, param: usize, value: f32) {
        let node = &mut self.nodes[node.0];
        let info = &node.module.info().params[param];
        node.settings[param] = info.clamp(value);
        node.knobs[param].set(info.to_normalized(value));
    }

    /// The value the knob is set to (not the modulated value).
    pub fn param(&self, node: NodeId, param: usize) -> f32 {
        self.nodes[node.0].settings[param]
    }

    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + use<> {
        (0..self.nodes.len()).map(NodeId)
    }

    pub fn name(&self, node: NodeId) -> &str {
        &self.nodes[node.0].name
    }

    pub fn info(&self, node: NodeId) -> &'static ModuleInfo {
        self.nodes[node.0].module.info()
    }

    pub fn cables(&self) -> &[Cable] {
        self.cables.as_slice()
    }

    pub fn process(&mut self) {
        for i in 0..self.nodes.len() {
            for input in &mut self.nodes[i].inputs {
                input.fill(0.0);
            }
            for cable in self.cables.as_slice().iter().filter(|c| c.to.0 == i) {
                let source = self.nodes[cable.from.0].outputs[cable.from_port];
                let target = &mut self.nodes[i].inputs[cable.to_port];
                for (t, s) in target.iter_mut().zip(source) {
                    *t += s;
                }
            }
            for input in &mut self.nodes[i].inputs {
                for s in input.iter_mut() {
                    *s = if s.is_nan() {
                        0.0
                    } else {
                        s.clamp(-RAIL, RAIL)
                    };
                }
            }

            let Node {
                module,
                inputs,
                knobs,
                params,
                outputs,
                ..
            } = &mut self.nodes[i];
            let info = module.info();
            let (signals, cvs) = inputs.split_at(info.inputs.len());
            for (p, ((buffer, knob), cv)) in params.iter_mut().zip(knobs).zip(cvs).enumerate() {
                for (out, cv) in buffer.iter_mut().zip(cv) {
                    *out = info.params[p].from_normalized(knob.tick() + cv);
                }
            }
            module.process(signals, params, outputs);
        }
    }

    pub fn output(&self, node: NodeId, port: usize) -> &Buffer {
        &self.nodes[node.0].outputs[port]
    }

    /// Every output buffer of a node, for inspecting the whole graph.
    pub fn outputs(&self, node: NodeId) -> &[Buffer] {
        &self.nodes[node.0].outputs
    }

    /// Renders at least `samples` samples of one output port, offline.
    pub fn render(&mut self, node: NodeId, port: usize, samples: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(samples + BLOCK);
        while out.len() < samples {
            self.process();
            out.extend_from_slice(self.output(node, port));
        }
        out.truncate(samples);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::param::{ParamInfo, Scale};

    const SR: f32 = 48_000.0;

    /// Outputs its input plus its `offset` knob.
    struct Offset;
    static OFFSET: ModuleInfo = ModuleInfo {
        inputs: &["in"],
        outputs: &["out"],
        params: &[ParamInfo {
            name: "offset",
            min: -10.0,
            max: 10.0,
            default: 1.0,
            scale: Scale::Linear,
            unit: "",
        }],
    };
    impl Module for Offset {
        fn info(&self) -> &'static ModuleInfo {
            &OFFSET
        }
        fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
            for ((o, i), p) in outputs[0].iter_mut().zip(inputs[0]).zip(params[0]) {
                *o = i + p;
            }
        }
    }

    /// Outputs a fixed value, whatever it is.
    struct Emit(f32);
    static EMIT: ModuleInfo = ModuleInfo {
        inputs: &[],
        outputs: &["out"],
        params: &[],
    };
    impl Module for Emit {
        fn info(&self) -> &'static ModuleInfo {
            &EMIT
        }
        fn process(&mut self, _: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
            outputs[0].fill(self.0);
        }
    }

    fn cable(from: NodeId, to: NodeId) -> Cable {
        Cable {
            from,
            from_port: 0,
            to,
            to_port: 0,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn signal_flows_through_a_chain() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        let b = g.add("b", Box::new(Offset));
        g.connect(cable(a, b)).unwrap();
        assert!(g.render(b, 0, 100).iter().all(|&s| close(s, 2.0)));
    }

    #[test]
    fn several_cables_into_one_input_are_summed() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        let b = g.add("b", Box::new(Offset));
        let sum = g.add("sum", Box::new(Offset));
        g.connect(cable(a, sum)).unwrap();
        g.connect(cable(b, sum)).unwrap();
        assert!(g.render(sum, 0, 100).iter().all(|&s| close(s, 3.0)));
    }

    #[test]
    fn unconnected_input_is_silent() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        assert!(g.render(a, 0, 100).iter().all(|&s| close(s, 1.0)));
    }

    #[test]
    fn inputs_are_clamped_to_the_rails() {
        for (value, expected) in [
            (1e9, RAIL),
            (f32::INFINITY, RAIL),
            (f32::NEG_INFINITY, -RAIL),
            (f32::NAN, 0.0),
        ] {
            let mut g = Graph::new(SR);
            let src = g.add("src", Box::new(Emit(value)));
            let a = g.add("a", Box::new(Offset));
            g.set_param(a, 0, 0.0);
            g.connect(cable(src, a)).unwrap();
            let out = g.render(a, 0, 4800);
            assert!(close(out[4799], expected), "{value} gave {}", out[4799]);
        }
    }

    #[test]
    fn feedback_cable_delays_by_one_block() {
        let mut g = Graph::new(SR);
        let acc = g.add("acc", Box::new(Offset));
        g.connect(cable(acc, acc)).unwrap();
        // Each block reads the previous block's output and adds one.
        let out = g.render(acc, 0, 3 * BLOCK);
        assert!(close(out[0], 1.0));
        assert!(close(out[BLOCK], 2.0));
        assert!(close(out[2 * BLOCK], 3.0));
    }

    #[test]
    fn knob_changes_glide_to_the_new_value() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        g.set_param(a, 0, 5.0);
        assert_eq!(g.param(a, 0), 5.0);
        let out = g.render(a, 0, 4800);
        assert!(out[0] < 2.0);
        assert!(close(out[4799], 5.0));
    }

    #[test]
    fn cv_input_moves_the_knob_in_normalized_space() {
        let mut g = Graph::new(SR);
        let src = g.add("src", Box::new(Offset));
        let a = g.add("a", Box::new(Offset));
        g.set_param(src, 0, 0.25);
        g.set_param(a, 0, 0.0);
        g.connect(Cable {
            to_port: OFFSET.cv_port(0),
            ..cable(src, a)
        })
        .unwrap();
        // The knob sits mid-range; +0.25 of travel over a range of 20 is +5.
        let out = g.render(a, 0, 4800);
        assert!(close(out[4799], 5.0));
    }

    #[test]
    fn cv_saturates_at_the_end_of_the_range() {
        let mut g = Graph::new(SR);
        let src = g.add("src", Box::new(Offset));
        let a = g.add("a", Box::new(Offset));
        g.set_param(src, 0, 10.0);
        g.connect(Cable {
            to_port: OFFSET.cv_port(0),
            ..cable(src, a)
        })
        .unwrap();
        let out = g.render(a, 0, 4800);
        assert!(close(out[4799], 10.0));
    }

    #[test]
    fn invalid_cables_are_rejected() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        let b = g.add("b", Box::new(Offset));
        let bad_port = Cable {
            to_port: 2,
            ..cable(a, b)
        };
        assert_eq!(g.connect(bad_port), Err(GraphError::NoSuchPort));
        g.connect(cable(a, b)).unwrap();
        assert_eq!(g.connect(cable(a, b)), Err(GraphError::AlreadyConnected));
    }

    #[test]
    fn disconnect_removes_the_cable() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        let b = g.add("b", Box::new(Offset));
        g.connect(cable(a, b)).unwrap();
        assert!(g.disconnect(cable(a, b)));
        assert!(!g.disconnect(cable(a, b)));
        assert!(g.cables().is_empty());
    }

    #[test]
    fn cable_count_is_capped() {
        let mut set = CableSet::new();
        for i in 0..MAX_CABLES {
            set.insert(cable(NodeId(i), NodeId(0)), &OFFSET, &OFFSET)
                .unwrap();
        }
        let extra = cable(NodeId(MAX_CABLES), NodeId(0));
        assert_eq!(
            set.insert(extra, &OFFSET, &OFFSET),
            Err(GraphError::TooManyCables)
        );
    }
}
