//! A patch put together piece by piece, in whatever order is handy, its
//! modules sorted into processing order at the end.

use std::collections::BTreeMap;

use audiochaos::patch::{Patch, PatchCable, PatchModule, VERSION};

/// Where a module goes in the processing order: a cable from a later stage
/// to an earlier one would cost a block of delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Clock,
    Song,
    Sequencer,
    Gate,
    Envelope,
    Modulation,
    Source,
    Filter,
    Amp,
    Effect,
    Mix,
    Master,
}

#[derive(Default)]
pub struct Builder {
    modules: Vec<(Stage, PatchModule)>,
    knobs: BTreeMap<String, BTreeMap<String, f32>>,
    cables: Vec<PatchCable>,
}

impl Builder {
    /// Adds a module, and returns it to fill in its score.
    pub fn module(&mut self, stage: Stage, name: &str, kind: &str) -> &mut PatchModule {
        let module = PatchModule {
            name: name.to_string(),
            kind: kind.to_string(),
            ..Default::default()
        };
        self.modules.push((stage, module));
        &mut self.modules.last_mut().expect("just pushed").1
    }

    pub fn knob(&mut self, module: &str, knob: &str, value: f32) {
        self.knobs
            .entry(module.to_string())
            .or_default()
            .insert(knob.to_string(), value);
    }

    pub fn cable(&mut self, from: &str, output: &str, to: &str, input: &str) {
        self.cables.push(PatchCable {
            from: from.to_string(),
            output: output.to_string(),
            to: to.to_string(),
            input: input.to_string(),
        });
    }

    pub fn finish(mut self, description: String) -> Patch {
        // Stable: modules of one stage keep the order they were added in.
        self.modules.sort_by_key(|&(stage, _)| stage);
        Patch {
            version: VERSION,
            description,
            modules: self.modules.into_iter().map(|(_, m)| m).collect(),
            knobs: self.knobs,
            cables: self.cables,
            positions: BTreeMap::new(),
        }
    }
}
