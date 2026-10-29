//! Patches: knob settings and cables, saved as JSON.
//!
//! Modules, knobs and ports are referenced by name rather than by index, so a
//! patch file is readable, can be edited by hand, and stays valid when the
//! rack gains modules or a module gains knobs.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::engine::control::Controller;
use crate::engine::graph::{Cable, CableSet, Graph, GraphError, NodeId};
use crate::engine::module::ModuleInfo;

/// Bumped on incompatible format changes, so old files fail loudly instead of
/// loading wrong.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
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
    UnknownModule(String),
    UnknownKnob { module: String, knob: String },
    UnknownPort { module: String, port: String },
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

/// A patch translated to node indices: every knob of the rack (defaults filled
/// in) and every cable, checked against the rack's modules.
#[derive(Debug, PartialEq)]
pub struct Resolved {
    pub knobs: Vec<(NodeId, usize, f32)>,
    pub cables: Vec<Cable>,
}

/// The rack as a patch sees it: module names and descriptions, in node order.
pub type Modules<'a> = [(&'a str, &'static ModuleInfo)];

impl Patch {
    pub fn from_json(json: &str) -> Result<Patch, PatchError> {
        let patch: Patch = serde_json::from_str(json).map_err(PatchError::Json)?;
        if patch.version != VERSION {
            return Err(PatchError::UnsupportedVersion(patch.version));
        }
        Ok(patch)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a patch always serializes")
    }

    /// Records every knob and cable. `params` holds each module's knob values.
    pub fn capture(modules: &Modules, params: &[&[f32]], cables: &[Cable]) -> Patch {
        let knobs = modules
            .iter()
            .zip(params)
            .filter(|((_, info), _)| !info.params.is_empty())
            .map(|((name, info), values)| {
                let knobs = info
                    .params
                    .iter()
                    .zip(values.iter())
                    .map(|(p, &v)| (p.name.to_string(), v))
                    .collect();
                (name.to_string(), knobs)
            })
            .collect();
        let cables = cables
            .iter()
            .map(|c| {
                let (from, from_info) = modules[c.from.0];
                let (to, to_info) = modules[c.to.0];
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
            knobs,
            cables,
            positions: BTreeMap::new(),
        }
    }

    pub fn resolve(&self, modules: &Modules) -> Result<Resolved, PatchError> {
        let find = |name: &str| {
            modules
                .iter()
                .position(|(n, _)| *n == name)
                .ok_or_else(|| PatchError::UnknownModule(name.to_string()))
        };

        for (module, knobs) in &self.knobs {
            let info = modules[find(module)?].1;
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
        for (node, (name, info)) in modules.iter().enumerate() {
            let set = self.knobs.get(*name);
            for (p, param) in info.params.iter().enumerate() {
                let value = set.and_then(|s| s.get(param.name)).copied();
                knobs.push((NodeId(node), p, value.unwrap_or(param.default)));
            }
        }

        let unknown_port = |module: &str, port: &str| PatchError::UnknownPort {
            module: module.to_string(),
            port: port.to_string(),
        };
        let mut cables = CableSet::new();
        for c in &self.cables {
            let (from, to) = (find(&c.from)?, find(&c.to)?);
            let (from_info, to_info) = (modules[from].1, modules[to].1);
            let from_port = from_info
                .outputs
                .iter()
                .position(|o| *o == c.output)
                .ok_or_else(|| unknown_port(&c.from, &c.output))?;
            let to_port = (0..to_info.num_input_ports())
                .find(|&port| to_info.input_port_name(port) == c.input)
                .ok_or_else(|| unknown_port(&c.to, &c.input))?;
            let cable = Cable {
                from: NodeId(from),
                from_port,
                to: NodeId(to),
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

    /// Sets up a graph that is not playing yet. Its existing cables are kept.
    pub fn apply_to_graph(&self, graph: &mut Graph) -> Result<(), PatchError> {
        let resolved = self.resolve(&graph_modules(graph))?;
        for (node, param, value) in resolved.knobs {
            graph.set_param(node, param, value);
        }
        for cable in resolved.cables {
            graph.connect(cable).map_err(PatchError::Graph)?;
        }
        Ok(())
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

    /// Replaces the playing patch.
    pub fn load_into(&self, controller: &mut Controller) -> Result<(), Box<dyn Error>> {
        let resolved = self.resolve(&controller_modules(controller))?;
        controller.replace(&resolved.knobs, &resolved.cables)?;
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

fn graph_modules(graph: &Graph) -> Vec<(&str, &'static ModuleInfo)> {
    graph
        .nodes()
        .map(|n| (graph.name(n), graph.info(n)))
        .collect()
}

fn controller_modules(controller: &Controller) -> Vec<(&str, &'static ModuleInfo)> {
    controller
        .nodes()
        .iter()
        .map(|n| (n.name.as_str(), n.info))
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
        "version": 1,
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
    fn graph_round_trips_through_json() {
        let mut g = graph();
        patch(OSC_TO_MASTER).apply_to_graph(&mut g).unwrap();
        let saved = Patch::from_graph(&g);
        let reloaded = patch(&saved.to_json());
        assert_eq!(reloaded, saved);
        assert_eq!(reloaded.knobs["osc"]["freq"], 440.0);
        assert_eq!(reloaded.cables.len(), 2);
    }

    #[test]
    fn controller_round_trips_and_replaces_the_patch() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 64);
        patch(OSC_TO_MASTER).load_into(&mut controller).unwrap();
        assert_eq!(controller.cables().len(), 2);
        assert_eq!(controller.nodes()[0].params[oscillator::FREQ], 440.0);

        let empty = patch(r#"{ "version": 1 }"#);
        empty.load_into(&mut controller).unwrap();
        assert!(controller.cables().is_empty());
        // Knobs left out of a patch go back to their default.
        assert_eq!(controller.nodes()[0].params[oscillator::FREQ], 110.0);
        assert_eq!(Patch::from_controller(&controller).cables, []);
    }

    #[test]
    fn positions_and_description_are_kept() {
        let json = r#"{
            "version": 1,
            "description": "test",
            "positions": { "osc": [10.0, 20.0] }
        }"#;
        let p = patch(json);
        assert_eq!(p.description, "test");
        assert_eq!(patch(&p.to_json()).positions["osc"], [10.0, 20.0]);
    }

    fn resolve_error(json: &str) -> String {
        let g = graph();
        let p = Patch::from_json(json);
        match p.and_then(|p| p.resolve(&graph_modules(&g))) {
            Ok(_) => panic!("{json} should not resolve"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn mistakes_are_reported_by_name() {
        assert_eq!(
            resolve_error(r#"{ "version": 1, "knobs": { "osk": {} } }"#),
            r#"no module named "osk""#
        );
        assert_eq!(
            resolve_error(r#"{ "version": 1, "knobs": { "osc": { "frq": 1.0 } } }"#),
            r#"module "osc" has no knob "frq""#
        );
        assert_eq!(
            resolve_error(
                r#"{ "version": 1, "cables": [
                    { "from": "osc", "output": "tri", "to": "master", "input": "in" }
                ] }"#
            ),
            r#"module "osc" has no port "tri""#
        );
        assert_eq!(
            resolve_error(r#"{ "version": 2 }"#),
            "patch format version 2 is not supported (expected 1)"
        );
        assert!(resolve_error(r#"{ "version": 1, "colour": "red" }"#).contains("unknown field"));
    }

    #[test]
    fn duplicate_cables_are_refused() {
        let json = r#"{ "version": 1, "cables": [
            { "from": "osc", "output": "saw", "to": "master", "input": "in" },
            { "from": "osc", "output": "saw", "to": "master", "input": "in" }
        ] }"#;
        assert_eq!(resolve_error(json), "invalid cable: already connected");
    }
}
