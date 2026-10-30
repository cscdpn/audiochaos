use std::error::Error;
use std::fmt;

use super::module::{BLOCK, Buffer, Module, ModuleInfo};
use super::param::Smoothed;

/// Cables are stored in a vector allocated once with this capacity, so that
/// connecting from the audio thread never allocates.
pub const MAX_CABLES: usize = 256;

/// Modules live in this many slots, allocated once for the same reason.
pub const MAX_MODULES: usize = 64;

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

    /// Removes every cable from or to `node`.
    pub fn remove_node(&mut self, node: NodeId) {
        self.0.retain(|c| c.from != node && c.to != node);
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

/// The lowest slot not used by `taken`, if any is left.
pub fn free_slot(taken: impl Iterator<Item = NodeId>) -> Option<NodeId> {
    let mut used = [false; MAX_MODULES];
    for id in taken {
        used[id.0] = true;
    }
    used.iter().position(|&u| !u).map(NodeId)
}

/// A module with its buffers. Built on the UI thread, where allocating is
/// fine, and moved to the audio thread whole.
pub struct Node {
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

impl Node {
    /// Knobs start at their defaults.
    pub fn new(name: &str, module: Box<dyn Module>, sample_rate: f32) -> Self {
        let info = module.info();
        let knobs = info
            .params
            .iter()
            .map(|p| Smoothed::new(p.to_normalized(p.default), sample_rate))
            .collect();
        Self {
            name: name.to_string(),
            inputs: vec![[0.0; BLOCK]; info.num_input_ports()],
            knobs,
            settings: info.params.iter().map(|p| p.default).collect(),
            params: vec![[0.0; BLOCK]; info.params.len()],
            outputs: vec![[0.0; BLOCK]; info.outputs.len()],
            module,
        }
    }
}

/// Modules are processed in the order they were added. A cable coming from a
/// module processed later (or from the module itself) reads that module's
/// previous block, so feedback loops are legal and get exactly one block of
/// delay.
///
/// Each module sits in a fixed slot, its [`NodeId`], so removing one leaves
/// the others' ids and cables alone. Slots and the processing order are
/// allocated up front: adding and removing modules never allocates.
pub struct Graph {
    sample_rate: f32,
    slots: Vec<Option<Box<Node>>>,
    order: Vec<NodeId>,
    cables: CableSet,
}

impl Graph {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            slots: (0..MAX_MODULES).map(|_| None).collect(),
            order: Vec::with_capacity(MAX_MODULES),
            cables: CableSet::new(),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn is_full(&self) -> bool {
        self.order.len() == MAX_MODULES
    }

    /// Adds a module in the first free slot.
    ///
    /// Panics if the graph [is full](Self::is_full).
    pub fn add(&mut self, name: &str, module: Box<dyn Module>) -> NodeId {
        let id = free_slot(self.nodes()).expect("the graph is full");
        self.insert(id, Box::new(Node::new(name, module, self.sample_rate)));
        id
    }

    /// Puts a node in the given slot, last in processing order. Does not
    /// allocate, so the audio thread can call it.
    ///
    /// Panics if the slot is taken.
    pub fn insert(&mut self, id: NodeId, node: Box<Node>) {
        assert!(self.slots[id.0].is_none(), "slot {} is taken", id.0);
        self.slots[id.0] = Some(node);
        self.order.push(id);
    }

    /// Takes a module out with its cables, and returns it so the caller
    /// chooses where it is freed. Does not allocate or free.
    pub fn remove(&mut self, id: NodeId) -> Option<Box<Node>> {
        let node = self.slots[id.0].take()?;
        self.order.retain(|&n| n != id);
        self.cables.remove_node(id);
        Some(node)
    }

    pub fn connect(&mut self, cable: Cable) -> Result<(), GraphError> {
        let from = self.info(cable.from);
        let to = self.info(cable.to);
        self.cables.insert(cable, from, to)
    }

    pub fn disconnect(&mut self, cable: Cable) -> bool {
        self.cables.remove(cable)
    }

    fn node(&self, id: NodeId) -> &Node {
        self.slots[id.0].as_ref().expect("no module in this slot")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots[id.0].as_mut().expect("no module in this slot")
    }

    /// Values outside the parameter's range are clamped.
    pub fn set_param(&mut self, node: NodeId, param: usize, value: f32) {
        let node = self.node_mut(node);
        let info = &node.module.info().params[param];
        node.settings[param] = info.clamp(value);
        node.knobs[param].set(info.to_normalized(value));
    }

    /// The value the knob is set to (not the modulated value).
    pub fn param(&self, node: NodeId, param: usize) -> f32 {
        self.node(node).settings[param]
    }

    /// The modules, in processing order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.order.iter().copied()
    }

    pub fn name(&self, node: NodeId) -> &str {
        &self.node(node).name
    }

    pub fn info(&self, node: NodeId) -> &'static ModuleInfo {
        self.node(node).module.info()
    }

    pub fn cables(&self) -> &[Cable] {
        self.cables.as_slice()
    }

    pub fn process(&mut self) {
        for k in 0..self.order.len() {
            let id = self.order[k];
            let mut node = self.slots[id.0].take().expect("ordered modules exist");
            for input in &mut node.inputs {
                input.fill(0.0);
            }
            for cable in self.cables.as_slice().iter().filter(|c| c.to == id) {
                // A module's own output is still in `node`, out of its slot.
                let source = match &self.slots[cable.from.0] {
                    Some(from) => from.outputs[cable.from_port],
                    None => node.outputs[cable.from_port],
                };
                for (t, s) in node.inputs[cable.to_port].iter_mut().zip(source) {
                    *t += s;
                }
            }
            for input in &mut node.inputs {
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
            } = &mut *node;
            let info = module.info();
            let (signals, cvs) = inputs.split_at(info.inputs.len());
            for (p, ((buffer, knob), cv)) in params.iter_mut().zip(knobs).zip(cvs).enumerate() {
                for (out, cv) in buffer.iter_mut().zip(cv) {
                    *out = info.params[p].from_normalized(knob.tick() + cv);
                }
            }
            module.process(signals, params, outputs);
            self.slots[id.0] = Some(node);
        }
    }

    pub fn output(&self, node: NodeId, port: usize) -> &Buffer {
        &self.node(node).outputs[port]
    }

    /// Every output buffer of a node, for inspecting the whole graph.
    pub fn outputs(&self, node: NodeId) -> &[Buffer] {
        &self.node(node).outputs
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
        name: "offset",
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
        name: "emit",
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
    fn removing_a_module_takes_its_cables_and_keeps_the_others_ids() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        let b = g.add("b", Box::new(Offset));
        let c = g.add("c", Box::new(Offset));
        g.connect(cable(a, b)).unwrap();
        g.connect(cable(a, c)).unwrap();
        let removed = g.remove(b).unwrap();
        assert_eq!(removed.name, "b");
        assert_eq!(g.cables(), [cable(a, c)]);
        assert_eq!(g.nodes().collect::<Vec<_>>(), [a, c]);
        assert_eq!(g.name(c), "c");
        assert!(g.render(c, 0, 100).iter().all(|&s| close(s, 2.0)));
        assert!(g.remove(b).is_none());
    }

    #[test]
    fn a_freed_slot_is_reused_but_processed_last() {
        let mut g = Graph::new(SR);
        let a = g.add("a", Box::new(Offset));
        let b = g.add("b", Box::new(Offset));
        g.remove(a);
        let c = g.add("c", Box::new(Offset));
        assert_eq!(c, a);
        assert_eq!(g.nodes().collect::<Vec<_>>(), [b, c]);
        // c now runs after b, so b -> c costs no delay.
        g.connect(cable(b, c)).unwrap();
        let out = g.render(c, 0, BLOCK);
        assert!(close(out[0], 2.0));
    }

    #[test]
    fn slots_run_out_at_the_cap() {
        let mut g = Graph::new(SR);
        for _ in 0..MAX_MODULES {
            assert!(!g.is_full());
            g.add("x", Box::new(Emit(0.0)));
        }
        assert!(g.is_full());
        assert_eq!(free_slot(g.nodes()), None);
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
