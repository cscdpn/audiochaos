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
use crate::engine::sample::Sample;
use crate::engine::score::{
    self, Arrangement, Key, MAX_BARS, MAX_SECTIONS, PATTERNS, Patterns, Score, Section, TRACKS,
};
use crate::modules::{self, Context, master, notes, sampler, song};

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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchModule {
    pub name: String,
    /// One of [`modules::KINDS`], by its name.
    #[serde(rename = "type")]
    pub kind: String,
    /// The sound a sampler plays, as a path, relative to the directory the
    /// program runs in unless absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// A song's sections, in the order they play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<PatchSection>>,
    /// A song's tracks, each a cell per section: `A` to `D` for the pattern
    /// it plays there, `.` for silence. Missing ones are silent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<String>>,
    /// The key a `notes` module's editor offers notes in: `A minor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// A `notes` module's patterns by letter, `A` to `D`: notes by name,
    /// `-` holding the note before, `.` for a rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patterns: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchSection {
    pub name: String,
    pub bars: u32,
}

impl PatchModule {
    /// The score these fields describe, if any; the module keeps its own
    /// otherwise.
    fn score(&self) -> Result<Option<Score>, PatchError> {
        let problem = |problem: String| PatchError::Score {
            module: self.name.clone(),
            problem,
        };
        let is_song = self.sections.is_some() || self.tracks.is_some();
        let is_notes = self.key.is_some() || self.patterns.is_some();
        if is_song && self.kind != song::INFO.name {
            return Err(problem("only a song has sections and tracks".into()));
        }
        if is_notes && self.kind != notes::INFO.name {
            return Err(problem("only a notes module has a key and patterns".into()));
        }
        if is_song {
            let mut arrangement = Arrangement::default();
            if let Some(sections) = &self.sections {
                if sections.len() > MAX_SECTIONS {
                    return Err(problem(format!("the most sections is {MAX_SECTIONS}")));
                }
                if let Some(s) = sections.iter().find(|s| s.bars > MAX_BARS) {
                    return Err(problem(format!(
                        "section {:?} is over {MAX_BARS} bars",
                        s.name
                    )));
                }
                arrangement.sections = sections
                    .iter()
                    .map(|s| Section {
                        name: s.name.clone(),
                        bars: s.bars,
                        tracks: [None; TRACKS],
                    })
                    .collect();
            }
            let tracks = self.tracks.as_deref().unwrap_or_default();
            if tracks.len() > TRACKS {
                return Err(problem(format!("the most tracks is {TRACKS}")));
            }
            let sections = arrangement.sections.len();
            for (t, text) in tracks.iter().enumerate() {
                let cells = score::parse_cells(text, sections)
                    .map_err(|e| problem(format!("track {}: {e}", t + 1)))?;
                for (section, cell) in arrangement.sections.iter_mut().zip(cells) {
                    section.tracks[t] = cell;
                }
            }
            return Ok(Some(Score::Arrangement(arrangement)));
        }
        if is_notes {
            let mut patterns = Patterns::default();
            if let Some(key) = &self.key {
                patterns.key = Key::parse(key).map_err(problem)?;
            }
            if let Some(texts) = &self.patterns {
                patterns.patterns = Default::default();
                for (letter, text) in texts {
                    let index = ["A", "B", "C", "D"]
                        .iter()
                        .position(|l| l == letter)
                        .ok_or_else(|| problem(format!("unknown pattern {letter:?}: A to D")))?;
                    patterns.patterns[index] = score::parse_steps(text)
                        .map_err(|e| problem(format!("pattern {letter}: {e}")))?;
                }
            }
            return Ok(Some(Score::Patterns(patterns)));
        }
        Ok(None)
    }

    /// Writes a score into these fields.
    fn set_score(&mut self, score: &Score) {
        match score {
            Score::Arrangement(arrangement) => {
                self.sections = Some(
                    arrangement
                        .sections
                        .iter()
                        .map(|s| PatchSection {
                            name: s.name.clone(),
                            bars: s.bars,
                        })
                        .collect(),
                );
                let mut tracks: Vec<String> = (0..TRACKS)
                    .map(|t| score::cells_text(arrangement.sections.iter().map(|s| s.tracks[t])))
                    .collect();
                let silent = |track: &String| !track.contains(|c: char| c.is_ascii_uppercase());
                while tracks.last().is_some_and(silent) {
                    tracks.pop();
                }
                self.tracks = (!tracks.is_empty()).then_some(tracks);
            }
            Score::Patterns(patterns) => {
                self.key = Some(patterns.key.name());
                self.patterns = Some(
                    (0..PATTERNS)
                        .filter(|&p| !patterns.patterns[p].is_empty())
                        .map(|p| {
                            let letter = score::pattern_letter(p as u8).to_string();
                            (letter, score::steps_text(&patterns.patterns[p]))
                        })
                        .collect(),
                );
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchCable {
    pub from: String,
    pub output: String,
    pub to: String,
    pub input: String,
}

impl PatchCable {
    /// This cable between modules `from` and `to`, by port index.
    fn ports(
        &self,
        from: NodeId,
        from_info: &ModuleInfo,
        to: NodeId,
        to_info: &ModuleInfo,
    ) -> Result<Cable, PatchError> {
        let unknown_port = |module: &str, port: &str| PatchError::UnknownPort {
            module: module.to_string(),
            port: port.to_string(),
        };
        let from_port = from_info
            .outputs
            .iter()
            .position(|o| *o == self.output)
            .ok_or_else(|| unknown_port(&self.from, &self.output))?;
        let to_port = (0..to_info.num_input_ports())
            .find(|&port| to_info.input_port_name(port) == self.input)
            .ok_or_else(|| unknown_port(&self.to, &self.input))?;
        Ok(Cable {
            from,
            from_port,
            to,
            to_port,
        })
    }
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
    /// A module other than a sampler was given a file.
    NotASampler(String),
    /// A song's arrangement or a `notes` module's patterns are wrong.
    Score {
        module: String,
        problem: String,
    },
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
            PatchError::NotASampler(m) => write!(f, "module {m:?} is no sampler: it takes no file"),
            PatchError::Score { module, problem } => write!(f, "module {module:?}: {problem}"),
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
                        ..Default::default()
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
    /// knob values, `files` its sound file and `scores` its score if it has
    /// them, all in the order of `modules`.
    pub fn capture(
        modules: &Modules,
        params: &[&[f32]],
        files: &[Option<&str>],
        scores: &[Option<&Score>],
        cables: &[Cable],
    ) -> Patch {
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
                .zip(files)
                .zip(scores)
                .map(|(((_, name, info), file), score)| {
                    let mut module = PatchModule {
                        name: name.to_string(),
                        kind: info.name.to_string(),
                        file: file.map(str::to_string),
                        ..Default::default()
                    };
                    if let Some(score) = score {
                        module.set_score(score);
                    }
                    module
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

        let mut cables = CableSet::new();
        for c in &self.cables {
            let ((from, from_info), (to, to_info)) = (find(&c.from)?, find(&c.to)?);
            let cable = c.ports(from, from_info, to, to_info)?;
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
    pub fn build(&self, context: &Context) -> Result<(Graph, NodeId), PatchError> {
        if self.modules.len() > MAX_MODULES {
            return Err(PatchError::TooManyModules);
        }
        let mut graph = Graph::new(context.sample_rate);
        let mut masters = Vec::new();
        for m in &self.modules {
            if graph.nodes().any(|id| graph.name(id) == m.name) {
                return Err(PatchError::DuplicateModule(m.name.clone()));
            }
            let module = modules::build(&m.kind, context, seed(&m.name)).ok_or_else(|| {
                PatchError::UnknownType {
                    module: m.name.clone(),
                    kind: m.kind.clone(),
                }
            })?;
            let id = graph.add(&m.name, module);
            if let Some(file) = &m.file {
                if m.kind != sampler::INFO.name {
                    return Err(PatchError::NotASampler(m.name.clone()));
                }
                graph.set_sample(id, Box::new(Sample::load_or_silence(file)));
            }
            if let Some(score) = m.score()? {
                graph.set_score(id, Box::new(score));
            }
            if m.kind == master::INFO.name {
                masters.push(id);
            }
        }
        let [output] = masters[..] else {
            return Err(PatchError::Masters(masters.len()));
        };
        let resolved = self.resolve(&graph_modules(&graph))?;
        for (node, param, value) in resolved.knobs {
            graph.init_param(node, param, value);
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
        let files: Vec<_> = graph
            .nodes()
            .map(|n| graph.sample(n).map(|s| s.path.as_str()))
            .collect();
        let scores: Vec<_> = graph.nodes().map(|n| graph.score(n)).collect();
        Patch::capture(
            &graph_modules(graph),
            &params,
            &files,
            &scores,
            graph.cables(),
        )
    }

    /// Replaces the playing patch, modules included. Returns what went wrong
    /// with its sound files, which leaves their samplers silent.
    pub fn load_into(
        &self,
        controller: &mut Controller,
        context: &Context,
    ) -> Result<Vec<String>, Box<dyn Error>> {
        let (graph, output) = self.build(context)?;
        let problems = sample_problems(&graph);
        controller.replace(graph, output)?;
        Ok(problems)
    }

    pub fn from_controller(controller: &Controller) -> Patch {
        let params: Vec<&[f32]> = controller
            .nodes()
            .iter()
            .map(|n| n.params.as_slice())
            .collect();
        let files: Vec<_> = controller
            .nodes()
            .iter()
            .map(|n| n.sample.as_ref().map(|s| s.path.as_str()))
            .collect();
        let scores: Vec<_> = controller
            .nodes()
            .iter()
            .map(|n| n.score.as_ref())
            .collect();
        Patch::capture(
            &controller_modules(controller),
            &params,
            &files,
            &scores,
            controller.cables(),
        )
    }
}

impl Patch {
    /// The named modules, with their knobs, sound files and positions, and
    /// the cables between them: a group of blocks to duplicate or keep, as a
    /// patch without a master. Cables to modules left out are dropped.
    pub fn group(&self, names: &[&str]) -> Patch {
        let kept = |name: &String| {
            names.contains(&name.as_str())
                && self
                    .modules
                    .iter()
                    .any(|m| &m.name == name && m.kind != master::INFO.name)
        };
        Patch {
            version: VERSION,
            description: String::new(),
            modules: self
                .modules
                .iter()
                .filter(|m| kept(&m.name))
                .cloned()
                .collect(),
            knobs: self
                .knobs
                .iter()
                .filter(|(name, _)| kept(name))
                .map(|(name, knobs)| (name.clone(), knobs.clone()))
                .collect(),
            cables: self
                .cables
                .iter()
                .filter(|c| kept(&c.from) && kept(&c.to))
                .cloned()
                .collect(),
            positions: self
                .positions
                .iter()
                .filter(|(name, _)| kept(name))
                .map(|(name, pos)| (name.clone(), *pos))
                .collect(),
        }
    }

    /// Adds this patch's modules to the playing one, with their knobs, sound
    /// files and the cables between them, after the modules already there. A
    /// master is left out, the playing patch has one. Each module gets a free
    /// name of its own: a copy of `osc 2` is the first free `osc`, `osc 3`…
    /// Returns the new modules by the names they had here. All or nothing:
    /// on a mistake, the modules already added are removed again.
    pub fn insert_into(
        &self,
        controller: &mut Controller,
        context: &Context,
    ) -> Result<Vec<(String, NodeId)>, Box<dyn Error>> {
        let modules: Vec<_> = self
            .modules
            .iter()
            .filter(|m| m.kind != master::INFO.name)
            .collect();
        if controller.nodes().len() + modules.len() > MAX_MODULES {
            return Err(PatchError::TooManyModules.into());
        }
        let mut added = Vec::new();
        let result = self.insert_modules(&modules, controller, context, &mut added);
        if let Err(e) = result {
            for (_, id) in added.into_iter().rev() {
                // Just added and checked: removing them cannot fail but for a
                // full queue, which the next command would report anyway.
                let _ = controller.remove(id);
            }
            return Err(e);
        }
        Ok(added)
    }

    fn insert_modules(
        &self,
        modules: &[&PatchModule],
        controller: &mut Controller,
        context: &Context,
        added: &mut Vec<(String, NodeId)>,
    ) -> Result<(), Box<dyn Error>> {
        for m in modules {
            let name = controller.free_name(base_name(&m.name));
            let module = modules::build(&m.kind, context, seed(&name)).ok_or_else(|| {
                PatchError::UnknownType {
                    module: m.name.clone(),
                    kind: m.kind.clone(),
                }
            })?;
            let id = controller.add(name, module)?;
            added.push((m.name.clone(), id));
            if let Some(file) = &m.file {
                if m.kind != sampler::INFO.name {
                    return Err(PatchError::NotASampler(m.name.clone()).into());
                }
                controller.set_sample(id, Box::new(Sample::load_or_silence(file)))?;
            }
            if let Some(score) = m.score()? {
                controller.set_score(id, score)?;
            }
            let info = controller.node(id).info;
            for (knob, &value) in self.knobs.get(&m.name).into_iter().flatten() {
                let param = info
                    .params
                    .iter()
                    .position(|p| p.name == knob)
                    .ok_or_else(|| PatchError::UnknownKnob {
                        module: m.name.clone(),
                        knob: knob.clone(),
                    })?;
                controller.set_param(id, param, value)?;
            }
        }
        let id = |name: &str| added.iter().find(|(n, _)| n == name).map(|&(_, id)| id);
        for c in &self.cables {
            let (Some(from), Some(to)) = (id(&c.from), id(&c.to)) else {
                let listed = |name: &String| self.modules.iter().any(|m| &m.name == name);
                match [&c.from, &c.to].into_iter().find(|n| !listed(n)) {
                    Some(unknown) => return Err(PatchError::UnknownModule(unknown.clone()).into()),
                    // A cable to the master, left out, goes too.
                    None => continue,
                }
            };
            let cable = c.ports(
                from,
                controller.node(from).info,
                to,
                controller.node(to).info,
            )?;
            controller.connect(cable)?;
        }
        Ok(())
    }
}

/// A module's name without its number: `osc 2` is a copy of `osc`.
fn base_name(name: &str) -> &str {
    match name.rsplit_once(' ') {
        Some((base, number)) if number.parse::<u32>().is_ok() => base,
        _ => name,
    }
}

/// What went wrong with a graph's sound files, one message per file.
pub fn sample_problems(graph: &Graph) -> Vec<String> {
    graph
        .nodes()
        .filter_map(|n| graph.sample(n))
        .filter_map(|s| {
            let problem = s.problem.as_ref()?;
            Some(format!("cannot load {}: {problem}", s.path))
        })
        .collect()
}

/// Adds a module of the given kind to the playing patch, named after its kind
/// (`filter`, `filter 2`…) and set up exactly as loading it from a patch file
/// would.
pub fn add_module(
    controller: &mut Controller,
    context: &Context,
    kind: &str,
) -> Result<NodeId, Box<dyn Error>> {
    let name = controller.free_name(kind);
    let module =
        modules::build(kind, context, seed(&name)).ok_or_else(|| PatchError::UnknownType {
            module: name.clone(),
            kind: kind.to_string(),
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
    use crate::engine::control::{COMMAND_QUEUE_CAPACITY, Engine};
    use crate::engine::graph::{MAX_CABLES, MAX_PARAMS};
    use crate::modules::{
        master::{self, Master},
        oscillator::{self, Oscillator},
        sequencer,
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
        let (g, out) = patch(OSC_TO_MASTER).build(&Context::new(SR)).unwrap();
        let names: Vec<_> = g.nodes().map(|n| g.name(n)).collect();
        assert_eq!(names, ["osc", "master"]);
        assert_eq!(g.name(out), "master");
        assert_eq!(g.param(NodeId(0), oscillator::FREQ), 440.0);
        assert_eq!(g.cables().len(), 2);
    }

    #[test]
    fn graph_round_trips_through_json() {
        let (g, _) = patch(OSC_TO_MASTER).build(&Context::new(SR)).unwrap();
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
        patch(OSC_TO_MASTER)
            .load_into(&mut controller, &Context::new(SR))
            .unwrap();
        assert_eq!(controller.cables().len(), 2);
        assert_eq!(controller.nodes()[0].params[oscillator::FREQ], 440.0);

        let alone = patch(r#"{ "version": 2, "modules": [{ "name": "out", "type": "master" }] }"#);
        alone.load_into(&mut controller, &Context::new(SR)).unwrap();
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
        let (g, _) = v1.build(&Context::new(SR)).unwrap();
        assert_eq!(g.cables().len(), 1);
    }

    #[test]
    fn added_modules_get_free_names_and_save_like_the_others() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 64);
        let first = add_module(&mut controller, &Context::new(SR), "noise").unwrap();
        let second = add_module(&mut controller, &Context::new(SR), "noise").unwrap();
        assert_eq!(controller.node(first).name, "noise");
        assert_eq!(controller.node(second).name, "noise 2");
        let saved = Patch::from_controller(&controller);
        assert_eq!(saved.modules[3].kind, "noise");
        assert!(patch(&saved.to_json()).build(&Context::new(SR)).is_ok());

        let error = add_module(&mut controller, &Context::new(SR), "theremin").unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"module "theremin" has unknown type "theremin""#
        );
    }

    /// An LFO sweeping a filter, both into the master, as in a saved patch.
    const SWEEP: &str = r#"{
        "version": 2,
        "modules": [
            { "name": "lfo", "type": "lfo" },
            { "name": "filter 2", "type": "filter" },
            { "name": "master", "type": "master" }
        ],
        "knobs": { "lfo": { "rate": 3.0 }, "filter 2": { "cutoff": 800.0 }, "master": { "volume": 0.2 } },
        "cables": [
            { "from": "lfo", "output": "sine", "to": "filter 2", "input": "cutoff cv" },
            { "from": "filter 2", "output": "lowpass", "to": "master", "input": "in" },
            { "from": "lfo", "output": "square", "to": "master", "input": "volume cv" }
        ],
        "positions": { "lfo": [0.0, 10.0], "filter 2": [300.0, 10.0], "master": [600.0, 0.0] }
    }"#;

    #[test]
    fn a_group_keeps_the_cables_between_its_modules_only() {
        let group = patch(SWEEP).group(&["lfo", "filter 2", "master"]);
        let names: Vec<_> = group.modules.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["lfo", "filter 2"], "the master stays out");
        assert_eq!(group.cables, patch(SWEEP).cables[..1]);
        assert_eq!(group.knobs.len(), 2);
        assert_eq!(group.positions.len(), 2);
        assert!(patch(&group.to_json()).group(&["lfo"]).cables.is_empty());
    }

    #[test]
    fn inserting_a_group_adds_renamed_copies_with_knobs_and_cables() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 256);
        let context = Context::new(SR);
        patch(SWEEP).load_into(&mut controller, &context).unwrap();
        let group = patch(SWEEP).group(&["lfo", "filter 2"]);
        let added = group.insert_into(&mut controller, &context).unwrap();
        let name = |id| controller.node(id).name.clone();
        let names: Vec<_> = added
            .iter()
            .map(|(old, id)| (old.as_str(), name(*id)))
            .collect();
        assert_eq!(
            names,
            [("lfo", "lfo 2".into()), ("filter 2", "filter".into())]
        );
        let saved = Patch::from_controller(&controller);
        assert_eq!(saved.knobs["lfo 2"]["rate"], 3.0);
        assert_eq!(saved.knobs["filter"]["cutoff"], 800.0);
        let copied = PatchCable {
            from: "lfo 2".into(),
            output: "sine".into(),
            to: "filter".into(),
            input: "cutoff cv".into(),
        };
        assert!(saved.cables.contains(&copied));
        assert_eq!(saved.cables.len(), 4, "the originals keep theirs");
    }

    #[test]
    fn inserting_a_whole_patch_leaves_its_master_out() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 256);
        let added = patch(SWEEP)
            .insert_into(&mut controller, &Context::new(SR))
            .unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(controller.nodes().len(), 4);
        assert_eq!(controller.cables().len(), 1, "the LFO into the filter");
    }

    #[test]
    fn a_mistake_while_inserting_adds_nothing() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 256);
        let bad = patch(&SWEEP.replace(r#""cutoff cv""#, r#""nope""#));
        let error = bad
            .insert_into(&mut controller, &Context::new(SR))
            .unwrap_err();
        assert_eq!(error.to_string(), r#"module "filter 2" has no port "nope""#);
        assert_eq!(controller.nodes().len(), 2);
        assert!(controller.cables().is_empty());
        let names: Vec<_> = controller.nodes().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["osc", "master"]);
    }

    #[test]
    fn a_patch_as_large_as_the_engine_takes_pastes_in_one_go() {
        // The audio thread takes no command meanwhile, as when a paste
        // happens between two blocks.
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), COMMAND_QUEUE_CAPACITY);
        let mut big = patch(r#"{ "version": 2 }"#);
        let count = MAX_MODULES - controller.nodes().len();
        let name = |i: usize| format!("seq {i}");
        for i in 0..count {
            big.modules.push(PatchModule {
                name: name(i),
                kind: sequencer::INFO.name.into(),
                ..Default::default()
            });
            let knobs = sequencer::INFO.params.iter();
            big.knobs
                .insert(name(i), knobs.map(|p| (p.name.into(), p.default)).collect());
        }
        let per_module = MAX_CABLES / count;
        for i in 0..count {
            for param in &sequencer::INFO.params[..per_module] {
                big.cables.push(PatchCable {
                    from: name(i),
                    output: sequencer::INFO.outputs[0].into(),
                    to: name((i + 1) % count),
                    input: format!("{} cv", param.name),
                });
            }
        }
        assert_eq!(sequencer::INFO.params.len(), MAX_PARAMS, "every knob set");

        big.insert_into(&mut controller, &Context::new(SR)).unwrap();
        assert_eq!(controller.nodes().len(), MAX_MODULES);
        assert_eq!(controller.cables().len(), per_module * count);
    }

    #[test]
    fn copies_are_numbered_after_their_base_name() {
        assert_eq!(base_name("osc 2"), "osc");
        assert_eq!(base_name("kick env"), "kick env");
        assert_eq!(base_name("osc"), "osc");
        assert_eq!(base_name("lfo 2b"), "lfo 2b");
    }

    fn sampler_patch(file: &str) -> Patch {
        patch(&format!(
            r#"{{ "version": 2, "modules": [
                {{ "name": "s", "type": "sampler", "file": {file:?} }},
                {{ "name": "master", "type": "master" }}
            ], "knobs": {{ "s": {{ "loop": 1.0 }} }}, "cables": [
                {{ "from": "s", "output": "out", "to": "master", "input": "in" }}
            ] }}"#
        ))
    }

    #[test]
    fn samplers_load_their_file_and_save_its_path() {
        let path = std::env::temp_dir()
            .join(format!("audiochaos-patch-{}.wav", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..4800 {
            wav.write_sample(i16::MAX).unwrap();
        }
        wav.finalize().unwrap();

        let p = sampler_patch(&path);
        let (mut g, out) = p.build(&Context::new(SR)).unwrap();
        assert!(sample_problems(&g).is_empty());
        assert_eq!(
            Patch::from_graph(&g).modules[0].file.as_deref(),
            Some(&*path)
        );
        assert!(g.render(out, master::OUT, 4800)[4000] > 0.1, "it plays");

        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 64);
        assert!(
            p.load_into(&mut controller, &Context::new(SR))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            Patch::from_controller(&controller).modules[0]
                .file
                .as_deref(),
            Some(&*path)
        );
    }

    #[test]
    fn a_missing_file_leaves_the_sampler_silent_and_keeps_the_path() {
        let p = sampler_patch("/nowhere/kick.wav");
        let (mut g, out) = p.build(&Context::new(SR)).unwrap();
        let problems = sample_problems(&g);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].starts_with("cannot load /nowhere/kick.wav: "),
            "{problems:?}"
        );
        assert!(g.render(out, master::OUT, 4800).iter().all(|&s| s == 0.0));
        assert_eq!(
            Patch::from_graph(&g).modules[0].file.as_deref(),
            Some("/nowhere/kick.wav"),
            "saving does not lose it"
        );

        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 64);
        let problems = p.load_into(&mut controller, &Context::new(SR)).unwrap();
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn only_samplers_take_a_file() {
        let json = r#"{ "version": 2, "modules": [
            { "name": "master", "type": "master", "file": "x.wav" }
        ] }"#;
        assert_eq!(
            error(json),
            r#"module "master" is no sampler: it takes no file"#
        );
    }

    /// A song and a notes module, with their scores.
    const SCORED: &str = r#"{ "version": 2, "modules": [
        { "name": "song", "type": "song",
          "sections": [{ "name": "verse", "bars": 8 }, { "name": "chorus", "bars": 4 }],
          "tracks": ["A B", ". A"] },
        { "name": "bass", "type": "notes", "key": "A minor",
          "patterns": { "A": "A2 - . C3", "C": "E2" } },
        { "name": "master", "type": "master" }
    ] }"#;

    #[test]
    fn songs_and_notes_keep_their_scores() {
        let (g, _) = patch(SCORED).build(&Context::new(SR)).unwrap();
        let Some(Score::Arrangement(arrangement)) = g.score(NodeId(0)) else {
            panic!("no arrangement")
        };
        let names: Vec<_> = arrangement
            .sections
            .iter()
            .map(|s| (s.name.as_str(), s.bars))
            .collect();
        assert_eq!(names, [("verse", 8), ("chorus", 4)]);
        assert_eq!(arrangement.sections[0].tracks[..3], [Some(0), None, None]);
        assert_eq!(
            arrangement.sections[1].tracks[..3],
            [Some(1), Some(0), None]
        );
        let Some(Score::Patterns(patterns)) = g.score(NodeId(1)) else {
            panic!("no patterns")
        };
        assert_eq!(patterns.key.name(), "A minor");
        assert_eq!(patterns.patterns[0].len(), 4);
        assert!(patterns.patterns[1].is_empty());

        let saved = Patch::from_graph(&g);
        assert_eq!(saved.modules, patch(SCORED).modules);
    }

    #[test]
    fn modules_without_a_score_in_the_file_keep_their_own() {
        let json = r#"{ "version": 2, "modules": [
            { "name": "song", "type": "song" }, { "name": "master", "type": "master" }
        ] }"#;
        let saved = Patch::from_graph(&patch(json).build(&Context::new(SR)).unwrap().0);
        let sections = saved.modules[0].sections.as_ref().unwrap();
        assert_eq!(sections.len(), 4, "the default arrangement");
        assert_eq!(saved.modules[0].tracks, None, "every track silent");
    }

    #[test]
    fn mistakes_in_scores_are_named() {
        let with = |old: &str, new: &str| error(&SCORED.replace(old, new));
        assert_eq!(
            with(r#""E2""#, r#""E2 H2""#),
            r#"module "bass": pattern C: unknown note "H2": a name and an octave, as "A2""#
        );
        assert_eq!(
            with(r#""A B""#, r#""A B A""#),
            r#"module "song": track 1: 3 cells for 2 sections"#
        );
        assert_eq!(
            with(r#""A minor""#, r#""A moody""#),
            r#"module "bass": unknown key "A moody": a note and a scale, as "A minor""#
        );
        assert_eq!(
            with(r#""C": "E2""#, r#""E": "E2""#),
            r#"module "bass": unknown pattern "E": A to D"#
        );
        assert_eq!(
            with(r#""type": "notes""#, r#""type": "seq""#),
            r#"module "bass": only a notes module has a key and patterns"#
        );
        assert_eq!(
            with(r#""type": "song""#, r#""type": "seq""#),
            r#"module "song": only a song has sections and tracks"#
        );
    }

    #[test]
    fn inserted_copies_keep_their_scores() {
        let (_engine, mut controller) = Engine::new(graph(), NodeId(1), 256);
        let added = patch(SCORED)
            .insert_into(&mut controller, &Context::new(SR))
            .unwrap();
        let saved = Patch::from_controller(&controller);
        let copy = |name: &str| saved.modules.iter().find(|m| m.name == name).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(copy("song").tracks, patch(SCORED).modules[0].tracks);
        assert_eq!(copy("bass").patterns, patch(SCORED).modules[1].patterns);
    }

    #[test]
    fn noise_sources_differ_by_name() {
        assert_ne!(seed("noise"), seed("noise 2"));
        assert_eq!(seed("noise"), seed("noise"));
    }

    fn error(json: &str) -> String {
        match Patch::from_json(json).and_then(|p| p.build(&Context::new(SR))) {
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
            "too many modules (max 256)"
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
