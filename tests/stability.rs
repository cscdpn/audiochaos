//! Chaotic patches must never send anything but finite, bounded samples to
//! the speakers, and must never leave a module stuck on non-finite state.

use audiochaos::engine::graph::{Cable, Graph, NodeId};
use audiochaos::engine::module::BLOCK;
use audiochaos::modules::master;
use audiochaos::rack;

const SR: f32 = 48_000.0;

/// Small deterministic generator, so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }

    fn unit(&mut self) -> f32 {
        self.below(1001) as f32 / 1000.0
    }
}

fn random_cable(g: &Graph, nodes: &[NodeId], rng: &mut Rng) -> Option<Cable> {
    let from = nodes[rng.below(nodes.len())];
    let to = nodes[rng.below(nodes.len())];
    let (from_info, to_info) = (g.info(from), g.info(to));
    if from_info.outputs.is_empty() || to_info.num_input_ports() == 0 {
        return None;
    }
    Some(Cable {
        from,
        from_port: rng.below(from_info.outputs.len()),
        to,
        to_port: rng.below(to_info.num_input_ports()),
    })
}

/// Patches the rack at random, with every knob at a random position (so
/// resonance, feedback, drive and FM depth often end up maxed out).
fn random_patch(seed: u64) -> (Graph, NodeId) {
    let mut rng = Rng(seed);
    let (mut g, out) = rack::build(SR, rack::Preset::Chaos).unwrap();
    let nodes: Vec<NodeId> = g.nodes().collect();
    for _ in 0..40 {
        if let Some(cable) = random_cable(&g, &nodes, &mut rng) {
            // Duplicates are refused; skipping them is fine here.
            let _ = g.connect(cable);
        }
    }
    for &node in &nodes {
        for (p, info) in g.info(node).params.iter().enumerate() {
            g.set_param(node, p, info.from_normalized(rng.unit()));
        }
    }
    (g, out)
}

#[test]
fn random_patches_stay_finite_and_bounded() {
    for seed in 0..25 {
        let (mut g, out) = random_patch(seed);
        let nodes: Vec<NodeId> = g.nodes().collect();
        // Four seconds each.
        for _ in 0..4 * 48_000 / BLOCK {
            g.process();
            assert!(
                g.output(out, master::OUT).iter().all(|s| s.abs() <= 1.0),
                "seed {seed}: master out of bounds"
            );
            for &node in &nodes {
                let finite = g.outputs(node).iter().flatten().all(|s| s.is_finite());
                assert!(finite, "seed {seed}: {} went non-finite", g.name(node));
            }
        }
    }
}

#[test]
fn self_feeding_loops_through_every_processor_stay_finite() {
    let (mut g, out) = rack::build(SR, rack::Preset::Chaos).unwrap();
    let nodes: Vec<NodeId> = g.nodes().collect();
    // Every module's first output into every input of itself and of the next
    // module: the densest feedback the rack allows without random choice.
    for pair in nodes.windows(2) {
        for &(from, to) in &[(pair[0], pair[0]), (pair[1], pair[0])] {
            if g.info(from).outputs.is_empty() {
                continue;
            }
            for to_port in 0..g.info(to).num_input_ports() {
                let _ = g.connect(Cable {
                    from,
                    from_port: 0,
                    to,
                    to_port,
                });
            }
        }
    }
    for &node in &nodes {
        for (p, info) in g.info(node).params.iter().enumerate() {
            g.set_param(node, p, info.max);
        }
    }
    let audio = g.render(out, master::OUT, 5 * 48_000);
    assert!(audio.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    for &node in &nodes {
        assert!(g.outputs(node).iter().flatten().all(|s| s.is_finite()));
    }
}
