use std::error::Error;
use std::fmt;
use std::mem;

use rtrb::{Consumer, Producer, RingBuffer};

use super::graph::{Cable, CableSet, Graph, GraphError, MAX_MODULES, Node, NodeId, free_slot};
use super::module::{BLOCK, Module, ModuleInfo};

enum Command {
    SetParam {
        node: NodeId,
        param: usize,
        value: f32,
    },
    Connect(Cable),
    Disconnect(Cable),
    Add(NodeId, Box<Node>),
    Remove(NodeId),
    Replace(Box<Graph>, NodeId),
}

/// What the audio thread hands back to be freed on the UI thread: freeing
/// memory is as unpredictable as allocating it.
enum Retired {
    // Never read: these are only ever dropped.
    #[allow(dead_code)]
    Node(Box<Node>),
    #[allow(dead_code)]
    Graph(Box<Graph>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ControlError {
    /// The audio thread has fallen behind; the change was not applied.
    QueueFull,
    TooManyModules,
    /// Patches refer to modules by name, so names are unique.
    NameTaken(String),
    /// The module feeding the speakers cannot be removed.
    Output,
    Graph(GraphError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlError::QueueFull => write!(f, "audio thread is not keeping up"),
            ControlError::TooManyModules => write!(f, "too many modules (max {MAX_MODULES})"),
            ControlError::NameTaken(name) => write!(f, "a module is already named {name:?}"),
            ControlError::Output => write!(f, "the output module cannot be removed"),
            ControlError::Graph(e) => e.fmt(f),
        }
    }
}

impl Error for ControlError {}

/// What the UI knows about a module.
pub struct NodeState {
    pub id: NodeId,
    pub name: String,
    pub info: &'static ModuleInfo,
    /// Knob values, in the parameters' units.
    pub params: Vec<f32>,
}

/// The UI side. The graph itself lives on the audio thread, so the controller
/// keeps a mirror of the patch for display, and only updates it once the
/// matching command is queued, so the two never disagree.
pub struct Controller {
    sample_rate: f32,
    output: NodeId,
    commands: Producer<Command>,
    retired: Consumer<Retired>,
    /// In processing order.
    nodes: Vec<NodeState>,
    cables: CableSet,
}

impl Controller {
    /// The module feeding the speakers.
    pub fn output(&self) -> NodeId {
        self.output
    }

    /// The modules, in processing order.
    pub fn nodes(&self) -> &[NodeState] {
        &self.nodes
    }

    /// Panics if no module has this id.
    pub fn node(&self, id: NodeId) -> &NodeState {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .expect("no module with this id")
    }

    pub fn cables(&self) -> &[Cable] {
        self.cables.as_slice()
    }

    /// Values outside the parameter's range are clamped.
    pub fn set_param(
        &mut self,
        node: NodeId,
        param: usize,
        value: f32,
    ) -> Result<(), ControlError> {
        let value = self.node(node).info.params[param].clamp(value);
        self.push(Command::SetParam { node, param, value })?;
        let index = self.index(node);
        self.nodes[index].params[param] = value;
        Ok(())
    }

    pub fn connect(&mut self, cable: Cable) -> Result<(), ControlError> {
        let (from, to) = (self.node(cable.from).info, self.node(cable.to).info);
        self.cables
            .insert(cable, from, to)
            .map_err(ControlError::Graph)?;
        if let Err(e) = self.push(Command::Connect(cable)) {
            self.cables.remove(cable);
            return Err(e);
        }
        Ok(())
    }

    /// Returns whether the cable was there.
    pub fn disconnect(&mut self, cable: Cable) -> Result<bool, ControlError> {
        if !self.cables.contains(cable) {
            return Ok(false);
        }
        self.push(Command::Disconnect(cable))?;
        self.cables.remove(cable);
        Ok(true)
    }

    /// Connects the cable, or removes it if it is already there. Returns
    /// whether it is now connected.
    pub fn toggle_cable(&mut self, cable: Cable) -> Result<bool, ControlError> {
        if self.disconnect(cable)? {
            Ok(false)
        } else {
            self.connect(cable).map(|()| true)
        }
    }

    /// Adds a module, processed after all the others. Its knobs start at
    /// their defaults.
    pub fn add(&mut self, name: String, module: Box<dyn Module>) -> Result<NodeId, ControlError> {
        if self.nodes.iter().any(|n| n.name == name) {
            return Err(ControlError::NameTaken(name));
        }
        let id = free_slot(self.nodes.iter().map(|n| n.id)).ok_or(ControlError::TooManyModules)?;
        let info = module.info();
        let node = Box::new(Node::new(&name, module, self.sample_rate));
        self.push(Command::Add(id, node))?;
        self.nodes.push(NodeState {
            id,
            name,
            info,
            params: info.params.iter().map(|p| p.default).collect(),
        });
        Ok(id)
    }

    /// Removes a module and its cables.
    pub fn remove(&mut self, id: NodeId) -> Result<(), ControlError> {
        if id == self.output {
            return Err(ControlError::Output);
        }
        self.push(Command::Remove(id))?;
        let index = self.index(id);
        self.nodes.remove(index);
        self.cables.remove_node(id);
        Ok(())
    }

    /// Swaps in a whole new graph, built on this thread, with `output`
    /// feeding the speakers.
    pub fn replace(&mut self, graph: Graph, output: NodeId) -> Result<(), ControlError> {
        let (nodes, cables) = mirror(&graph);
        self.push(Command::Replace(Box::new(graph), output))?;
        self.output = output;
        self.nodes = nodes;
        self.cables = cables;
        Ok(())
    }

    fn index(&self, id: NodeId) -> usize {
        self.nodes
            .iter()
            .position(|n| n.id == id)
            .expect("no module with this id")
    }

    /// `base` if no module has that name yet, else the first free `base 2`,
    /// `base 3`…
    pub fn free_name(&self, base: &str) -> String {
        let taken = |name: &str| self.nodes.iter().any(|n| n.name == name);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !taken(name))
            .expect("some number is free")
    }

    fn push(&mut self, command: Command) -> Result<(), ControlError> {
        // Free whatever the audio thread is done with. Doing it on every push
        // means the retired queue, as large as the command queue, never fills.
        while self.retired.pop().is_ok() {}
        self.commands
            .push(command)
            .map_err(|_| ControlError::QueueFull)
    }
}

/// What the controller mirrors of a graph.
fn mirror(graph: &Graph) -> (Vec<NodeState>, CableSet) {
    let nodes = graph
        .nodes()
        .map(|id| {
            let info = graph.info(id);
            NodeState {
                id,
                name: graph.name(id).to_string(),
                info,
                params: (0..info.params.len()).map(|p| graph.param(id, p)).collect(),
            }
        })
        .collect();
    let mut cables = CableSet::new();
    for &cable in graph.cables() {
        cables
            .insert(cable, graph.info(cable.from), graph.info(cable.to))
            .expect("the graph only holds valid cables");
    }
    (nodes, cables)
}

/// The audio side: owns the graph and hands out samples one at a time.
///
/// The graph lives on the audio thread only, so it needs no lock; the UI
/// reaches it through a lock-free queue, drained once per block.
pub struct Engine {
    /// Boxed so a replacement graph moves in and the old one out without
    /// allocating or freeing here.
    graph: Box<Graph>,
    output: NodeId,
    commands: Consumer<Command>,
    retired: Producer<Retired>,
    position: usize,
}

impl Engine {
    /// `output` is the node whose port 0 goes to the speakers.
    pub fn new(graph: Graph, output: NodeId, queue_capacity: usize) -> (Self, Controller) {
        let (producer, consumer) = RingBuffer::new(queue_capacity);
        let (retired_producer, retired_consumer) = RingBuffer::new(queue_capacity);
        let (nodes, cables) = mirror(&graph);
        let controller = Controller {
            sample_rate: graph.sample_rate(),
            output,
            commands: producer,
            retired: retired_consumer,
            nodes,
            cables,
        };
        let engine = Self {
            graph: Box::new(graph),
            output,
            commands: consumer,
            retired: retired_producer,
            position: BLOCK,
        };
        (engine, controller)
    }

    pub fn next_sample(&mut self) -> f32 {
        if self.position == BLOCK {
            self.apply_commands();
            self.graph.process();
            self.position = 0;
        }
        let sample = self.graph.output(self.output, 0)[self.position];
        self.position += 1;
        sample
    }

    fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            // The controller checked every command against the same rules
            // before sending it, so none of them can fail here.
            match command {
                Command::SetParam { node, param, value } => {
                    self.graph.set_param(node, param, value)
                }
                Command::Connect(cable) => {
                    let _ = self.graph.connect(cable);
                }
                Command::Disconnect(cable) => {
                    self.graph.disconnect(cable);
                }
                Command::Add(id, node) => self.graph.insert(id, node),
                Command::Remove(id) => {
                    if let Some(node) = self.graph.remove(id) {
                        self.retire(Retired::Node(node));
                    }
                }
                Command::Replace(graph, output) => {
                    let old = mem::replace(&mut self.graph, graph);
                    self.output = output;
                    self.retire(Retired::Graph(old));
                }
            }
        }
    }

    fn retire(&mut self, item: Retired) {
        // Cannot fail while the controller keeps draining (see
        // `Controller::push`); if it ever did, freeing here beats leaking.
        let _ = self.retired.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::master::{self, Master};
    use crate::modules::oscillator::{self, Oscillator};
    use crate::modules::vca::{self, Vca};

    const SR: f32 = 48_000.0;

    /// A VCA (gain 1, no input) feeding the master.
    fn engine(queue_capacity: usize) -> (Engine, Controller, NodeId, NodeId) {
        let mut graph = Graph::new(SR);
        let amp = graph.add("vca", Box::new(Vca));
        let out = graph.add("master", Box::new(Master));
        let (engine, controller) = Engine::new(graph, out, queue_capacity);
        (engine, controller, amp, out)
    }

    fn vca_to_master_mod(amp: NodeId, out: NodeId) -> Cable {
        Cable {
            from: amp,
            from_port: vca::OUT,
            to: out,
            to_port: master::IN,
        }
    }

    fn run_block(engine: &mut Engine) {
        for _ in 0..BLOCK {
            engine.next_sample();
        }
    }

    #[test]
    fn mirrors_the_graph_it_was_built_from() {
        let (_, controller, amp, _) = engine(8);
        let names: Vec<_> = controller.nodes().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["vca", "master"]);
        assert_eq!(controller.node(amp).params, [1.0]);
    }

    #[test]
    fn param_changes_apply_at_the_next_block() {
        let (mut engine, mut controller, _, out) = engine(8);
        engine.next_sample();
        controller.set_param(out, master::VOLUME, 0.1).unwrap();
        for _ in 1..BLOCK {
            engine.next_sample();
            assert_eq!(engine.graph.param(out, master::VOLUME), 0.5);
        }
        engine.next_sample();
        assert!((engine.graph.param(out, master::VOLUME) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn param_values_are_clamped_in_the_mirror_too() {
        let (_, mut controller, _, out) = engine(8);
        controller.set_param(out, master::VOLUME, 7.0).unwrap();
        assert_eq!(controller.node(out).params[master::VOLUME], 1.0);
    }

    #[test]
    fn toggling_a_cable_connects_then_disconnects_it() {
        let (mut engine, mut controller, amp, out) = engine(8);
        let cable = vca_to_master_mod(amp, out);
        assert_eq!(controller.toggle_cable(cable), Ok(true));
        run_block(&mut engine);
        assert_eq!(engine.graph.cables(), [cable]);
        assert_eq!(controller.cables(), [cable]);

        assert_eq!(controller.toggle_cable(cable), Ok(false));
        run_block(&mut engine);
        assert!(engine.graph.cables().is_empty());
        assert!(controller.cables().is_empty());
    }

    #[test]
    fn replace_swaps_the_whole_graph() {
        let (mut engine, mut controller, amp, out) = engine(16);
        controller.connect(vca_to_master_mod(amp, out)).unwrap();
        run_block(&mut engine);

        let mut graph = Graph::new(SR);
        let new_out = graph.add("speakers", Box::new(Master));
        graph.set_param(new_out, master::VOLUME, 0.2);
        controller.replace(graph, new_out).unwrap();
        assert_eq!(controller.output(), new_out);
        assert_eq!(controller.nodes().len(), 1);
        assert_eq!(controller.node(new_out).name, "speakers");
        assert_eq!(controller.node(new_out).params, [0.2]);
        assert!(controller.cables().is_empty());

        run_block(&mut engine);
        assert_eq!(engine.output, new_out);
        assert!(engine.graph.cables().is_empty());
        // The old graph comes back to be freed off the audio thread.
        assert_eq!(controller.retired.slots(), 1);
    }

    #[test]
    fn added_modules_play_after_the_others() {
        let (mut engine, mut controller, _, out) = engine(16);
        let osc = controller
            .add("osc".to_string(), Box::new(Oscillator::new(SR)))
            .unwrap();
        assert_eq!(controller.nodes().last().unwrap().name, "osc");
        assert_eq!(controller.node(osc).params, [110.0, 100.0]);
        controller
            .connect(Cable {
                from: osc,
                from_port: oscillator::OUT_SAW,
                to: out,
                to_port: master::IN,
            })
            .unwrap();
        let samples: Vec<f32> = (0..4 * BLOCK).map(|_| engine.next_sample()).collect();
        assert!(
            samples.iter().any(|&s| s.abs() > 0.1),
            "the new oscillator is heard"
        );
    }

    #[test]
    fn names_stay_unique() {
        let (_, mut controller, _, _) = engine(16);
        assert_eq!(controller.free_name("vca"), "vca 2");
        assert_eq!(controller.free_name("osc"), "osc");
        assert_eq!(
            controller.add("vca".to_string(), Box::new(Vca)),
            Err(ControlError::NameTaken("vca".to_string()))
        );
        controller.add("vca 2".to_string(), Box::new(Vca)).unwrap();
        assert_eq!(controller.free_name("vca"), "vca 3");
    }

    #[test]
    fn removing_a_module_takes_its_cables_and_frees_it_off_the_audio_thread() {
        let (mut engine, mut controller, amp, out) = engine(16);
        controller.connect(vca_to_master_mod(amp, out)).unwrap();
        controller.remove(amp).unwrap();
        assert!(controller.cables().is_empty());
        assert_eq!(controller.nodes().len(), 1);

        run_block(&mut engine);
        assert!(engine.graph.cables().is_empty());
        assert_eq!(engine.graph.nodes().collect::<Vec<_>>(), [out]);
        assert_eq!(controller.retired.slots(), 1);
        // The next command frees it.
        controller.set_param(out, master::VOLUME, 0.3).unwrap();
        assert_eq!(controller.retired.slots(), 0);
    }

    #[test]
    fn the_output_cannot_be_removed() {
        let (_, mut controller, _, out) = engine(16);
        assert_eq!(controller.remove(out), Err(ControlError::Output));
        assert_eq!(controller.nodes().len(), 2);
    }

    #[test]
    fn a_removed_module_frees_its_slot() {
        let (mut engine, mut controller, amp, _) = engine(16);
        controller.remove(amp).unwrap();
        let again = controller.add("vca".to_string(), Box::new(Vca)).unwrap();
        assert_eq!(again, amp);
        run_block(&mut engine);
        assert_eq!(engine.graph.name(again), "vca");
    }

    #[test]
    fn module_count_is_capped() {
        let (_, mut controller, _, _) = engine(256);
        for n in 2..MAX_MODULES {
            controller.add(format!("vca {n}"), Box::new(Vca)).unwrap();
        }
        assert_eq!(
            controller.add("one more".to_string(), Box::new(Vca)),
            Err(ControlError::TooManyModules)
        );
    }

    #[test]
    fn invalid_cables_are_refused_before_reaching_the_audio_thread() {
        let (_, mut controller, amp, out) = engine(8);
        let bad = Cable {
            to_port: 99,
            ..vca_to_master_mod(amp, out)
        };
        assert_eq!(
            controller.toggle_cable(bad),
            Err(ControlError::Graph(GraphError::NoSuchPort))
        );
        assert!(controller.cables().is_empty());
    }

    #[test]
    fn a_full_queue_leaves_the_mirror_unchanged() {
        let (_engine, mut controller, amp, out) = engine(2);
        controller.set_param(out, master::VOLUME, 0.2).unwrap();
        controller.set_param(out, master::VOLUME, 0.3).unwrap();
        assert_eq!(
            controller.set_param(out, master::VOLUME, 0.9),
            Err(ControlError::QueueFull)
        );
        assert_eq!(controller.node(out).params[master::VOLUME], 0.3);
        let cable = vca_to_master_mod(amp, out);
        assert_eq!(controller.toggle_cable(cable), Err(ControlError::QueueFull));
        assert!(controller.cables().is_empty());
        assert_eq!(
            controller.add("vca 2".to_string(), Box::new(Vca)),
            Err(ControlError::QueueFull)
        );
        assert_eq!(controller.remove(amp), Err(ControlError::QueueFull));
        assert_eq!(controller.nodes().len(), 2);
    }
}
