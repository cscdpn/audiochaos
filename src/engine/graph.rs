use std::error::Error;
use std::fmt;

use super::module::{BLOCK, Buffer, Module};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(usize);

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
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::NoSuchPort => write!(f, "no such port"),
        }
    }
}

impl Error for GraphError {}

struct Node {
    module: Box<dyn Module>,
    inputs: Vec<Buffer>,
    outputs: Vec<Buffer>,
}

/// Modules are processed in insertion order. A cable coming from a module
/// processed later (or from the module itself) reads that module's previous
/// block, so feedback loops are legal and get exactly one block of delay.
#[derive(Default)]
pub struct Graph {
    nodes: Vec<Node>,
    cables: Vec<Cable>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, module: Box<dyn Module>) -> NodeId {
        let inputs = vec![[0.0; BLOCK]; module.num_inputs()];
        let outputs = vec![[0.0; BLOCK]; module.num_outputs()];
        self.nodes.push(Node {
            module,
            inputs,
            outputs,
        });
        NodeId(self.nodes.len() - 1)
    }

    pub fn connect(&mut self, cable: Cable) -> Result<(), GraphError> {
        if cable.from_port >= self.nodes[cable.from.0].outputs.len()
            || cable.to_port >= self.nodes[cable.to.0].inputs.len()
        {
            return Err(GraphError::NoSuchPort);
        }
        self.cables.push(cable);
        Ok(())
    }

    pub fn set_param(&mut self, node: NodeId, param: usize, value: f32) {
        self.nodes[node.0].module.set_param(param, value);
    }

    pub fn process(&mut self) {
        for i in 0..self.nodes.len() {
            for input in &mut self.nodes[i].inputs {
                input.fill(0.0);
            }
            for cable in self.cables.iter().filter(|c| c.to.0 == i) {
                let source = self.nodes[cable.from.0].outputs[cable.from_port];
                let target = &mut self.nodes[i].inputs[cable.to_port];
                for (t, s) in target.iter_mut().zip(source) {
                    *t += s;
                }
            }
            let node = &mut self.nodes[i];
            node.module.process(&node.inputs, &mut node.outputs);
        }
    }

    pub fn output(&self, node: NodeId, port: usize) -> &Buffer {
        &self.nodes[node.0].outputs[port]
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

    /// Outputs a constant on its single output.
    struct Constant(f32);
    impl Module for Constant {
        fn num_inputs(&self) -> usize {
            0
        }
        fn num_outputs(&self) -> usize {
            1
        }
        fn process(&mut self, _: &[Buffer], outputs: &mut [Buffer]) {
            outputs[0].fill(self.0);
        }
    }

    /// Adds `step` to its input.
    struct AddStep(f32);
    impl Module for AddStep {
        fn num_inputs(&self) -> usize {
            1
        }
        fn num_outputs(&self) -> usize {
            1
        }
        fn process(&mut self, inputs: &[Buffer], outputs: &mut [Buffer]) {
            for (o, i) in outputs[0].iter_mut().zip(inputs[0]) {
                *o = i + self.0;
            }
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

    #[test]
    fn signal_flows_through_a_chain() {
        let mut g = Graph::new();
        let a = g.add(Box::new(Constant(1.0)));
        let b = g.add(Box::new(AddStep(2.0)));
        g.connect(cable(a, b)).unwrap();
        assert!(g.render(b, 0, 100).iter().all(|&s| s == 3.0));
    }

    #[test]
    fn several_cables_into_one_input_are_summed() {
        let mut g = Graph::new();
        let a = g.add(Box::new(Constant(1.0)));
        let b = g.add(Box::new(Constant(10.0)));
        let sum = g.add(Box::new(AddStep(0.0)));
        g.connect(cable(a, sum)).unwrap();
        g.connect(cable(b, sum)).unwrap();
        assert!(g.render(sum, 0, 100).iter().all(|&s| s == 11.0));
    }

    #[test]
    fn unconnected_input_is_silent() {
        let mut g = Graph::new();
        let b = g.add(Box::new(AddStep(0.5)));
        assert!(g.render(b, 0, 100).iter().all(|&s| s == 0.5));
    }

    #[test]
    fn feedback_cable_delays_by_one_block() {
        let mut g = Graph::new();
        let acc = g.add(Box::new(AddStep(1.0)));
        g.connect(cable(acc, acc)).unwrap();
        // Each block reads the previous block's output and adds one.
        let out = g.render(acc, 0, 3 * BLOCK);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[BLOCK], 2.0);
        assert_eq!(out[2 * BLOCK], 3.0);
    }

    #[test]
    fn connecting_a_missing_port_fails() {
        let mut g = Graph::new();
        let a = g.add(Box::new(Constant(1.0)));
        let b = g.add(Box::new(AddStep(0.0)));
        let bad = Cable {
            to_port: 5,
            ..cable(a, b)
        };
        assert_eq!(g.connect(bad), Err(GraphError::NoSuchPort));
    }
}
