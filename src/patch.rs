//! Patches: which modules, their knob settings and cables, saved as JSON.
//!
//! Modules, knobs and ports are referenced by name rather than by index, so a
//! patch file is readable, can be edited by hand, and stays valid when a
//! module gains knobs.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::engine::control::Controller;
use crate::engine::graph::{Cable, CableSet, Graph, GraphError, MAX_MODULES, NodeId};
use crate::engine::module::ModuleInfo;
use crate::modules::{self, master};

/// Bumped on incompatible format changes, so old files fail loudly instead of
/// loading wrong.
pub const VERSION: u32 = 2;

/// Version 1 patches had no module list: they all used this fixed rack.
const RACK_V1: [(&str, &str); 13] = [
    ("seq", "seq"),
    ("lfo", "lfo"),
    ("noise", "noise"),
    ("s&h", "s&h"),
    ("atten", "atten"),
    ("osc 1", "osc"),
    ("osc 2", "osc"),
    ("filter", "filter"),
    ("dist", "dist"),
    ("crush", "crush"),
    ("delay", "delay"),
    ("vca", "vca"),
    ("master", "master"),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// In processing order: a cable going backwards in this list is a
    /// feedback cable and costs one block of delay.
    #[serde(default)]
    pub modules: Vec<PatchModule>,
    /// Module name, then knob name, to value. Knobs left out are at their
    /// default, so a patch always fully determines the sound.
    #[serde(default)]
    pub knobs: BTreeMap<String, BTreeMap<String, f32>>,
    #[serde(default)]
    pub cables: Vec<PatchCable>,
    /// Module name to block position in the graphical editor. Only the GUI
    /// uses it; other front ends keep it as is.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<String, [f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchModule {
    pub name: String,
    /// One of [`modules::KINDS`].
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchCable {
    pub from: String,
    pub output: String,
    pub to: String,
    pub input: String,
}

#[derive(Debug)]
pub enum PatchError {
    Json(serde_json::Error),
    UnsupportedVersion(u32),
    UnknownType {
        module: String,
        kind: String,
    },
    DuplicateModule(String),
    TooManyModules,
    /// A patch needs exactly one master; holds how many it has.
    Masters(usize),
    UnknownModule(String),
    UnknownKnob {
        module: String,
        knob: String,
    },
    UnknownPort {
        module: String,
        port: String,
    },
    Graph(GraphError),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Json(e) => write!(f, "invalid patch file: {e}"),
            PatchError::UnsupportedVersion(v) => {
                write!(
                    f,
                    "patch format version {v} is not supported (expected {VERSION})"
                )
            }
            PatchError::UnknownType { module, kind } => {
                write!(f, "module {module:?} has unknown type {kind:?}")
            }
            PatchError::DuplicateModule(m) => write!(f, "two modules are named {m:?}"),
            PatchError::TooManyModules => write!(f, "too many modules (max {MAX_MODULES})"),
            PatchError::Masters(n) => {
                write!(f, "a patch needs exactly one master module, not {n}")
            }
            PatchError::UnknownModule(m) => write!(f, "no module named {m:?}"),
            PatchError::UnknownKnob { module, knob } => {
                write!(f, "module {module:?} has no knob {knob:?}")
            }
            PatchError::UnknownPort { module, port } => {
                write!(f, "module {module:?} has no port {port:?}")
            }
            PatchError::Graph(e) => write!(f, "invalid cable: {e}"),
        }
    }
}

impl Error for PatchError {}

/// A patch translated to node ids: every knob of the graph (defaults filled
/// in) and every cable, checked against the graph's modules.
#[derive(Debug, PartialEq)]
pub struct Resolved {
    pub knobs: Vec<(NodeId, usize, f32)>,
    pub cables: Vec<Cable>,
}

/// A graph's modules as a patch sees them, in processing order.
pub type Modules<'a> = [(NodeId, &'a str, &'static ModuleInfo)];

impl Patch {
    pub fn from_json(json: &str) -> Result<Patch, PatchError> {
        let mut patch: Patch = serde_json::from_str(json).map_err(PatchError::Json)?;
        match patch.version {
            VERSION => {}
            1 if patch.modules.is_empty() => {
                patch.modules = RACK_V1
                    .iter()
                    .map(|(name, kind)| PatchModule {
                        name: name.to_string(),
                        kind: kind.to_string(),
                    })
                    .collect();
                patch.version = VERSION;
            }
            v => return Err(PatchError::UnsupportedVersion(v)),
        }
        Ok(patch)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a patch always serializes")
    }

    /// Records every module, knob and cable. `params` holds each module's
    /// knob values, in the order of `modules`.
    pub fn capture(modules: &Modules, params: &[&[f32]], cables: &[Cable]) -> Patch {
        let knobs = modules
            .iter()
            .zip(params)
            .filter(|((_, _, info), _)| !info.params.is_empty())
            .map(|((_, name, info), values)| {
                let knobs = info
                    .params
                    .iter()
                    .zip(values.iter())
                    .map(|(p, &v)| (p.name.to_string(), v))
                    .collect();
                (name.to_string(), knobs)
            })
            .collect();
        let module = |id: NodeId| {
            let (_, name, info) = modules
                .iter()
                .find(|(n, _, _)| *n == id)
                .expect("cables join listed modules");
            (*name, *info)
        };
        let cables = cables
            .iter()
            .map(|c| {
                let (from, from_info) = module(c.from);
                let (to, to_info) = module(c.to);
                PatchCable {
                    from: from.to_string(),
                    output: from_info.outputs[c.from_port].to_string(),
                    to: to.to_string(),
                    input: to_info.input_port_name(c.to_port),
                }
            })
            .collect();
        Patch {
            version: VERSION,
            description: String::new(),
            modules: modules
                .iter()
                .map(|(_, name, info)| PatchModule {
                    name: name.to_string(),
                    kind: info.name.to_string(),
                })
                .collect(),
            knobs,
            cables,
            positions: BTreeMap::new(),
        }
    }

    pub fn resolve(&self, modules: &Modules) -> Result<Resolved, PatchError> {
        let find = |name: &str| {
            modules
                .iter()
                .find(|(_, n, _)| *n == name)
                .map(|&(id, _, info)| (id, info))
                .ok_or_else(|| PatchError::UnknownModule(name.to_string()))
        };

        for (module, knobs) in &self.knobs {
            let (_, info) = find(module)?;
            if let Some(knob) = knobs
                .keys()
                .find(|k| !info.params.iter().any(|p| &p.name == k))
            {
                return Err(PatchError::UnknownKnob {
                    module: module.clone(),
                    knob: knob.clone(),
                });
            }
        }
        let mut knobs = Vec::new();
        for &(id, name, info) in modules {
            let set = self.knobs.get(name);
            for (p, param) in info.params.iter().enumerate() {
                let value = set.and_then(|s| s.get(param.name)).copied();
                knobs.push((id, p, value.unwrap_or(param.default)));
            }
        }

        let unknown_port = |module: &str, port: &str| PatchError::UnknownPort {
            module: module.to_string(),
            port: port.to_string(),
        };
        let mut cables = CableSet::new();
        for c in &self.cables {
            let ((from, from_info), (to, to_info)) = (find(&c.from)?, find(&c.to)?);
            let from_port = from_info
                .outputs
                .iter()
                .position(|o| *o == c.output)
                .ok_or_else(|| unknown_port(&c.from, &c.output))?;
            let to_port = (0..to_info.num_input_ports())
                .find(|&port| to_info.input_port_name(port) == c.input)
                .ok_or_else(|| unknown_port(&c.to, &c.input))?;
            let cable = Cable {
                from,
                from_port,
                to,
                to_port,
            };
            cables
                .insert(cable, from_info, to_info)
                .map_err(PatchError::Graph)?;
        }
        Ok(Resolved {
            knobs,
            cables: cables.as_slice().to_vec(),
        })
    }

    /// Builds the patch's graph, and returns it with the master, which feeds
    /// the speakers.
    pub fn build(&self, sample_rate: f32) -> Result<(Graph, NodeId), PatchError> {
        if self.modules.len() > MAX_MODULES {
            return Err(PatchError::TooManyModules);
        }
        let mut graph = Graph::new(sample_rate);
        let mut masters = Vec::new();
        for m in &self.modules {
            if graph.nodes().any(|id| graph.name(id) == m.name) {
                return Err(PatchError::DuplicateModule(m.name.clone()));
            }
            let module = modules::build(&m.kind, sample_rate, seed(&m.name)).ok_or_else(|| {
                PatchError::UnknownType {
                    module: m.name.clone(),
                    kind: m.kind.clone(),
                }
            })?;
            let id = graph.add(&m.name, module);
            if m.kind == master::INFO.name {
                masters.push(id);
            }
        }
        let [output] = masters[..] else {
            return Err(PatchError::Masters(masters.len()));
        };
        let resolved = self.resolve(&graph_modules(&graph))?;
        for (node, param, value) in resolved.knobs {
            graph.set_param(node, param, value);
        }
        for cable in resolved.cables {
            graph.connect(cable).map_err(PatchError::Graph)?;
        }
        Ok((graph, output))
    }

    pub fn from_graph(graph: &Graph) -> Patch {
        let params: Vec<Vec<f32>> = graph
            .nodes()
            .map(|n| {
                (0..graph.info(n).params.len())
                    .map(|p| graph.param(n, p))
                    .collect()
            })
            .collect();
        let params: Vec<&[f32]> = params.iter().map(Vec::as_slice).collect();
        Patch::capture(&graph_modules(graph), &params, graph.cables())
    }

    /// Replaces the playing patch, modules included.
    pub fn load_into(&self, controller: &mut Controller) -> Result<(), Box<dyn Error>> {
        let (graph, output) = self.build(controller.sample_rate())?;
        controller.replace(graph, output)?;
        Ok(())
    }

    pub fn from_controller(controller: &Controller) -> Patch {
        let params: Vec<&[f32]> = controller
            .nodes()
            .iter()
            .map(|n| n.params.as_slice())
            .collect();
        Patch::capture(
            &controller_modules(controller),
            &params,
            controller.cables(),
        )
    }
}

/// Adds a module of the given kind to the playing patch, named after its kind
/// (`filter`, `filter 2`…) and set up exactly as loading it from a patch file
/// would.
pub fn add_module(controller: &mut Controller, kind: &str) -> Result<NodeId, Box<dyn Error>> {
    let name = controller.free_name(kind);
    let module = modules::build(kind, controller.sample_rate(), seed(&name)).ok_or_else(|| {
        PatchError::UnknownType {
            module: name.clone(),
            kind: kind.to_string(),
        }
    })?;
    Ok(controller.add(name, module)?)
}

/// Derived from the module's name, so each noise source sounds different but
/// a patch renders the same every time. FNV-1a.
fn seed(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn graph_modules(graph: &Graph) -> Vec<(NodeId, &str, &'static ModuleInfo)> {
    graph
        .nodes()
        .map(|n| (n, graph.name(n), graph.info(n)))
        .collect()
}

fn controller_modules(controller: &Controller) -> Vec<(NodeId, &str, &'static ModuleInfo)> {
    controller
        .nodes()
        .iter()
        .map(|n| (n.id, n.name.as_str(), n.info))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::control::Engine;
    use crate::modules::{
        master::{self, Master},
        oscillator::{self, Oscillator},
    };

    const SR: f32 = 48_000.0;

    /// An oscillator into the master.
    fn graph() -> Graph {
        let mut g = Graph::new(SR);
        g.add("osc", Box::new(Oscillator::new(SR)));
        g.add("master", Box::new(Master));
        g
    }

    fn patch(json: &str) -> Patch {
        Patch::from_json(json).unwrap()
    }

    const OSC_TO_MASTER: &str = r#"{
        "version": 2,
        "modules": [
            { "name": "osc", "type": "osc" },
            { "name": "master", "type": "master" }
        ],
        "knobs": { "osc": { "freq": 440.0 } },
        "cables": [
            { "from": "osc", "output": "saw", "to": "master", "input": "in" },
            { "from": "osc", "output": "sine", "to": "osc", "input": "fm depth cv" }
        ]
    }"#;

    #[test]
    fn resolves_names_to_ports_and_fills_in_defaults() {
        let g = graph();
        let resolved = patch(OSC_TO_MASTER).resolve(&graph_modules(&g)).unwrap();
        let (osc, out) = (NodeId(0), NodeId(1));
        assert_eq!(
            resolved.knobs,
            [
                (osc, oscillator::FREQ, 440.0),
                (osc, oscillator::FM_DEPTH, 100.0),
                (out, master::VOLUME, 0.5),
            ]
        );
        assert_eq!(
            resolved.cables,
            [
                Cable {
                    from: osc,
                    from_port: oscillator::OUT_SAW,
                    to: out,
                    to_port: master::IN,
                },
                Cable {
                    from: osc,
                    from_port: oscillator::OUT_SINE,
                    to: osc,
                    to_port: oscillator::INFO.cv_port(oscillator::FM_DEPTH),
                },
            ]
        );
    }

    #[test]
    fn builds_the_modules_it_lists_and_finds_the_master() {
        let (g, out) = patch(OSC_TO_MASTER).build(SR).unwrap();
        let names: Vec<_> = g.nodes().map(|n| g.name(n)).collect();
        assert_eq!(names, ["osc", "master"]);
        assert_eq!(g.name(out), "master");
        assert_eq!(g.param(NodeId(0), oscillator::FREQ), 440.0);
        assert_eq!(g.cables().len(), 2);
    }

    #[test]
    fn graph_round_trips_through_json() {
        let (g, _) = patch(OSC_TO_MASTER).build(SR).unwrap();
        let saved = Patch::from_graph(&g);
        let reloaded = patch(&saved.to_json());
        assert_eq!(reloaded, saved);
        assert_eq!(reloaded.modules, patch(OSC_TO_MASTER).modules);
        assert_eq!(reloaded.knobs["osc"]["freq"], 440.0);
        assert_eq!(reloaded.cables.len(), 2);
    }

    #[test]
    fn loading_replaces_modules_knobs_and_cables() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 64);
        patch(OSC_TO_MASTER).load_into(&mut controller).unwrap();
        assert_eq!(controller.cables().len(), 2);
        assert_eq!(controller.nodes()[0].params[oscillator::FREQ], 440.0);

        let alone = patch(r#"{ "version": 2, "modules": [{ "name": "out", "type": "master" }] }"#);
        alone.load_into(&mut controller).unwrap();
        assert_eq!(controller.nodes().len(), 1);
        assert_eq!(controller.node(controller.output()).name, "out");
        assert_eq!(Patch::from_controller(&controller).modules, alone.modules);
    }

    #[test]
    fn version_1_patches_load_into_the_old_fixed_rack() {
        let v1 = patch(
            r#"{ "version": 1, "cables": [
                { "from": "osc 2", "output": "saw", "to": "master", "input": "in" }
            ] }"#,
        );
        assert_eq!(v1.version, VERSION);
        assert_eq!(v1.modules.len(), RACK_V1.len());
        let (g, _) = v1.build(SR).unwrap();
        assert_eq!(g.cables().len(), 1);
    }

    #[test]
    fn added_modules_get_free_names_and_save_like_the_others() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 64);
        let first = add_module(&mut controller, "noise").unwrap();
        let second = add_module(&mut controller, "noise").unwrap();
        assert_eq!(controller.node(first).name, "noise");
        assert_eq!(controller.node(second).name, "noise 2");
        let saved = Patch::from_controller(&controller);
        assert_eq!(saved.modules[3].kind, "noise");
        assert!(patch(&saved.to_json()).build(SR).is_ok());

        let error = add_module(&mut controller, "theremin").unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"module "theremin" has unknown type "theremin""#
        );
    }

    #[test]
    fn noise_sources_differ_by_name() {
        assert_ne!(seed("noise"), seed("noise 2"));
        assert_eq!(seed("noise"), seed("noise"));
    }

    fn error(json: &str) -> String {
        match Patch::from_json(json).and_then(|p| p.build(SR)) {
            Ok(_) => panic!("{json} should not build"),
            Err(e) => e.to_string(),
        }
    }

    /// `OSC_TO_MASTER`'s modules with other fields.
    fn with_osc(rest: &str) -> String {
        format!(
            r#"{{ "version": 2, "modules": [
                {{ "name": "osc", "type": "osc" }},
                {{ "name": "master", "type": "master" }}
            ]{rest} }}"#
        )
    }

    #[test]
    fn mistakes_are_reported_by_name() {
        assert_eq!(
            error(&with_osc(r#", "knobs": { "osk": {} }"#)),
            r#"no module named "osk""#
        );
        assert_eq!(
            error(&with_osc(r#", "knobs": { "osc": { "frq": 1.0 } }"#)),
            r#"module "osc" has no knob "frq""#
        );
        assert_eq!(
            error(&with_osc(
                r#", "cables": [
                    { "from": "osc", "output": "tri", "to": "master", "input": "in" }
                ]"#
            )),
            r#"module "osc" has no port "tri""#
        );
        assert_eq!(
            error(r#"{ "version": 3 }"#),
            "patch format version 3 is not supported (expected 2)"
        );
        assert!(error(r#"{ "version": 2, "colour": "red" }"#).contains("unknown field"));
    }

    #[test]
    fn module_list_mistakes() {
        let modules = |list: &str| format!(r#"{{ "version": 2, "modules": [{list}] }}"#);
        let master = r#"{ "name": "master", "type": "master" }"#;
        assert_eq!(
            error(&modules(&format!(
                r#"{master}, {{ "name": "x", "type": "kazoo" }}"#
            ))),
            r#"module "x" has unknown type "kazoo""#
        );
        assert_eq!(
            error(&modules(&format!(r#"{master}, {master}"#))),
            r#"two modules are named "master""#
        );
        assert_eq!(
            error(&modules("")),
            "a patch needs exactly one master module, not 0"
        );
        assert_eq!(
            error(&modules(&format!(
                r#"{master}, {{ "name": "out 2", "type": "master" }}"#
            ))),
            "a patch needs exactly one master module, not 2"
        );
        let many: Vec<String> = (0..=MAX_MODULES)
            .map(|n| format!(r#"{{ "name": "vca {n}", "type": "vca" }}"#))
            .collect();
        assert_eq!(
            error(&modules(&many.join(","))),
            "too many modules (max 64)"
        );
    }

    #[test]
    fn duplicate_cables_are_refused() {
        let json = with_osc(
            r#", "cables": [
            { "from": "osc", "output": "saw", "to": "master", "input": "in" },
            { "from": "osc", "output": "saw", "to": "master", "input": "in" }
        ]"#,
        );
        assert_eq!(error(&json), "invalid cable: already connected");
    }

    #[test]
    fn positions_and_description_are_kept() {
        let p = patch(&with_osc(
            r#", "description": "test", "positions": { "osc": [10.0, 20.0] }"#,
        ));
        assert_eq!(p.description, "test");
        assert_eq!(patch(&p.to_json()).positions["osc"], [10.0, 20.0]);
    }
}
