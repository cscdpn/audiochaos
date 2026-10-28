use std::error::Error;
use std::fmt;

use rtrb::{Consumer, Producer, RingBuffer};

use super::graph::{Cable, CableSet, Graph, GraphError, NodeId};
use super::module::{BLOCK, ModuleInfo};

enum Command {
    SetParam {
        node: NodeId,
        param: usize,
        value: f32,
    },
    Connect(Cable),
    Disconnect(Cable),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ControlError {
    /// The audio thread has fallen behind; the change was not applied.
    QueueFull,
    Graph(GraphError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlError::QueueFull => write!(f, "audio thread is not keeping up"),
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
    commands: Producer<Command>,
    nodes: Vec<NodeState>,
    cables: CableSet,
}

impl Controller {
    pub fn nodes(&self) -> &[NodeState] {
        &self.nodes
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
        let info = &self.nodes[node.0].info.params[param];
        let value = info.from_normalized(info.to_normalized(value));
        self.push(Command::SetParam { node, param, value })?;
        self.nodes[node.0].params[param] = value;
        Ok(())
    }

    /// Connects the cable, or removes it if it is already there. Returns
    /// whether it is now connected.
    pub fn toggle_cable(&mut self, cable: Cable) -> Result<bool, ControlError> {
        if self.cables.contains(cable) {
            self.push(Command::Disconnect(cable))?;
            self.cables.remove(cable);
            return Ok(false);
        }
        let (from, to) = (self.nodes[cable.from.0].info, self.nodes[cable.to.0].info);
        self.cables
            .insert(cable, from, to)
            .map_err(ControlError::Graph)?;
        if let Err(e) = self.push(Command::Connect(cable)) {
            self.cables.remove(cable);
            return Err(e);
        }
        Ok(true)
    }

    fn push(&mut self, command: Command) -> Result<(), ControlError> {
        self.commands
            .push(command)
            .map_err(|_| ControlError::QueueFull)
    }
}

/// The audio side: owns the graph and hands out samples one at a time.
///
/// The graph lives on the audio thread only, so it needs no lock; the UI
/// reaches it through a lock-free queue, drained once per block.
pub struct Engine {
    graph: Graph,
    output: NodeId,
    commands: Consumer<Command>,
    position: usize,
}

impl Engine {
    /// `output` is the node whose port 0 goes to the speakers.
    pub fn new(graph: Graph, output: NodeId, queue_capacity: usize) -> (Self, Controller) {
        let (producer, consumer) = RingBuffer::new(queue_capacity);
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
        let controller = Controller {
            commands: producer,
            nodes,
            cables,
        };
        let engine = Self {
            graph,
            output,
            commands: consumer,
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
            match command {
                Command::SetParam { node, param, value } => {
                    self.graph.set_param(node, param, value)
                }
                // The controller checked the cable against the same rules
                // before sending it, so this cannot fail.
                Command::Connect(cable) => {
                    let _ = self.graph.connect(cable);
                }
                Command::Disconnect(cable) => {
                    self.graph.disconnect(cable);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::master::{self, Master};
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
        assert_eq!(controller.nodes()[amp.0].params, [1.0]);
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
        assert_eq!(controller.nodes()[out.0].params[master::VOLUME], 1.0);
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
        assert_eq!(controller.nodes()[out.0].params[master::VOLUME], 0.3);
        let cable = vca_to_master_mod(amp, out);
        assert_eq!(controller.toggle_cable(cable), Err(ControlError::QueueFull));
        assert!(controller.cables().is_empty());
    }
}
