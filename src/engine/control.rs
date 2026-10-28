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
    use crate::engine::module::{Buffer, Module};

    /// Outputs its only parameter, unsmoothed.
    struct Knob(f32);
    impl Module for Knob {
        fn num_inputs(&self) -> usize {
            0
        }
        fn num_outputs(&self) -> usize {
            1
        }
        fn process(&mut self, _: &[Buffer], outputs: &mut [Buffer]) {
            outputs[0].fill(self.0);
        }
        fn set_param(&mut self, _: usize, value: f32) {
            self.0 = value;
        }
    }

    fn knob_engine() -> (Engine, Controller, NodeId) {
        let mut graph = Graph::new();
        let knob = graph.add(Box::new(Knob(1.0)));
        let (engine, controller) = Engine::new(graph, knob, 8);
        (engine, controller, knob)
    }

    #[test]
    fn plays_the_output_node() {
        let (mut engine, _, _) = knob_engine();
        assert!((0..3 * BLOCK).all(|_| engine.next_sample() == 1.0));
    }

    #[test]
    fn param_changes_apply_at_the_next_block() {
        let (mut engine, mut controller, knob) = knob_engine();
        engine.next_sample();
        controller.set_param(knob, 0, 2.0).unwrap();
        assert!((1..BLOCK).all(|_| engine.next_sample() == 1.0));
        assert_eq!(engine.next_sample(), 2.0);
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
