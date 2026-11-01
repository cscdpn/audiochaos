//! Chaotic patches must never send anything but finite, bounded samples to
//! the speakers, and must never leave a module stuck on non-finite state.

use audiochaos::engine::control::Engine;
use audiochaos::engine::graph::{Cable, Graph, NodeId};
use audiochaos::engine::module::BLOCK;
use audiochaos::modules::{self, Context, master};
use audiochaos::{patch, presets};

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
    let (mut g, out) = presets::default().build(&Context::new(SR)).unwrap();
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

/// Every kind of module, any knob position, its inputs all fed noise.
#[test]
fn every_module_stays_finite_with_any_knobs() {
    let context = Context::new(SR);
    for info in modules::KINDS {
        for seed in 0..8 {
            let mut rng = Rng(seed);
            let mut g = Graph::new(SR);
            let noise = g.add("noise", modules::build("noise", &context, 7).unwrap());
            let node = g.add(info.name, modules::build(info.name, &context, 1).unwrap());
            for p in 0..info.params.len() {
                // The ends of the range, half of the time.
                let n = match rng.below(4) {
                    0 => 0.0,
                    1 => 1.0,
                    _ => rng.unit(),
                };
                g.init_param(node, p, info.params[p].from_normalized(n));
            }
            for to_port in 0..info.inputs.len() {
                let cable = Cable {
                    from: noise,
                    from_port: 0,
                    to: node,
                    to_port,
                };
                g.connect(cable).unwrap();
            }
            for _ in 0..48_000 / BLOCK {
                g.process();
                let finite = g.outputs(node).iter().flatten().all(|s| s.is_finite());
                assert!(finite, "{} went non-finite, seed {seed}", info.name);
            }
        }
    }
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
    let (mut g, out) = presets::default().build(&Context::new(SR)).unwrap();
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

/// Adds, removes and patches modules at random while the engine plays, the
/// way someone hammering the editor would, and checks the sound stays finite
/// and bounded and the controller's mirror stays a valid patch.
#[test]
fn editing_the_patch_while_it_plays_stays_consistent() {
    let mut rng = Rng(7);
    let context = Context::new(SR);
    let (graph, out) = presets::default().build(&context).unwrap();
    let (mut engine, mut controller) = Engine::new(graph, out, 1024);
    for _ in 0..2_000 {
        match rng.below(4) {
            0 => {
                let kinds: Vec<_> = modules::KINDS
                    .iter()
                    .filter(|k| k.name != master::INFO.name)
                    .collect();
                let kind = kinds[rng.below(kinds.len())].name;
                // Refused once the patch is full, which is fine here.
                let _ = patch::add_module(&mut controller, &context, kind);
            }
            1 => {
                let nodes = controller.nodes();
                let id = nodes[rng.below(nodes.len())].id;
                if id != controller.output() {
                    controller.remove(id).unwrap();
                }
            }
            _ => {
                let nodes = controller.nodes();
                let (from, to) = (
                    &nodes[rng.below(nodes.len())],
                    &nodes[rng.below(nodes.len())],
                );
                if !from.info.outputs.is_empty() && to.info.num_input_ports() > 0 {
                    let cable = Cable {
                        from: from.id,
                        from_port: rng.below(from.info.outputs.len()),
                        to: to.id,
                        to_port: rng.below(to.info.num_input_ports()),
                    };
                    let _ = controller.toggle_cable(cable);
                }
            }
        }
        for _ in 0..rng.below(3) * BLOCK {
            let sample = engine.next_sample();
            assert!(sample.is_finite() && sample.abs() <= 1.0);
        }
    }
    let saved = patch::Patch::from_controller(&controller);
    let (rebuilt, _) = saved
        .build(&Context::new(SR))
        .expect("the mirror is a valid patch");
    assert_eq!(patch::Patch::from_graph(&rebuilt), saved);
}
