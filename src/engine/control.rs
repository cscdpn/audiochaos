use rtrb::{Consumer, Producer, PushError, RingBuffer};

use super::graph::{Graph, NodeId};
use super::module::BLOCK;

pub enum Command {
    SetParam {
        node: NodeId,
        param: usize,
        value: f32,
    },
}

/// The UI side: sends commands to the audio thread without ever blocking it.
pub struct Controller {
    commands: Producer<Command>,
}

impl Controller {
    /// Fails only if the audio thread has fallen behind and the queue is full.
    pub fn set_param(
        &mut self,
        node: NodeId,
        param: usize,
        value: f32,
    ) -> Result<(), PushError<Command>> {
        self.commands.push(Command::SetParam { node, param, value })
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
        let engine = Self {
            graph,
            output,
            commands: consumer,
            position: BLOCK,
        };
        (engine, Controller { commands: producer })
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
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::master::{self, Master};

    fn knob_engine() -> (Engine, Controller, NodeId) {
        let mut graph = Graph::new(48_000.0);
        let knob = graph.add("master", Box::new(Master));
        let (engine, controller) = Engine::new(graph, knob, 8);
        (engine, controller, knob)
    }

    #[test]
    fn param_changes_apply_at_the_next_block() {
        let (mut engine, mut controller, knob) = knob_engine();
        engine.next_sample();
        controller.set_param(knob, master::VOLUME, 0.1).unwrap();
        // The master is silent without input, so check the knob, not the sound.
        for _ in 1..BLOCK {
            engine.next_sample();
            assert_eq!(engine.graph.param(knob, master::VOLUME), 0.5);
        }
        engine.next_sample();
        assert!((engine.graph.param(knob, master::VOLUME) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn full_queue_reports_an_error_instead_of_blocking() {
        let (_engine, mut controller, knob) = knob_engine();
        for _ in 0..8 {
            controller.set_param(knob, 0, 0.0).unwrap();
        }
        assert!(controller.set_param(knob, 0, 0.0).is_err());
    }
}
