//! The instruments a song is made of, after the templates of the
//! `audiochaos-song` skill: a sequencer, an envelope, a sound and a VCA the
//! envelope opens, then a few effects. The template is fixed; its knobs,
//! notes and rhythms are drawn.

use std::collections::BTreeMap;

use audiochaos::engine::score::note_name;

use crate::builder::{Builder, Stage};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Kick,
    Snare,
    Hats,
    Bass,
    Lead,
    Pad,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Kick,
        Kind::Snare,
        Kind::Hats,
        Kind::Bass,
        Kind::Lead,
        Kind::Pad,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Kick => "kick",
            Kind::Snare => "snare",
            Kind::Hats => "hats",
            Kind::Bass => "bass",
            Kind::Lead => "lead",
            Kind::Pad => "pad",
        }
    }

    /// How many of the patterns A to D it has: a drum only plays or rests.
    pub fn draw_patterns(self, rng: &mut Rng) -> usize {
        match self {
            Kind::Kick | Kind::Snare | Kind::Hats => 1,
            Kind::Bass | Kind::Lead | Kind::Pad => rng.int(1, 4) as usize,
        }
    }
}

/// A key as the generator draws it.
pub struct Key {
    /// Pitch class, 0 for C.
    pub root: u8,
    /// As a patch writes it: `A minor`.
    pub name: String,
    /// Semitones above the root.
    pub intervals: &'static [u8],
}

/// One instrument of the song, before it is added.
pub struct Instrument<'a> {
    pub kind: Kind,
    /// Unique in the song: `kick`, `kick 2`...
    pub name: &'a str,
    /// The song's output that plays it: `track 1`...
    pub track: &'a str,
    pub patterns: usize,
}

const EFFECTS: [&str; 7] = [
    "dist", "crush", "delay", "chorus", "flanger", "phaser", "univibe",
];

/// Adds the instrument's modules and cables, and returns its last module,
/// whose `out` is the instrument's sound.
pub fn add(b: &mut Builder, rng: &mut Rng, instrument: &Instrument, key: &Key) -> String {
    let mut end = match instrument.kind {
        Kind::Kick | Kind::Snare | Kind::Hats => drum(b, rng, instrument),
        Kind::Bass | Kind::Lead | Kind::Pad => melodic(b, rng, instrument, key),
    };
    let mut effects = EFFECTS.to_vec();
    for _ in 0..*rng.pick(&[0, 0, 1, 2]) {
        let effect = effects.remove(rng.below(effects.len()));
        let name = format!("{} {effect}", instrument.name);
        b.module(Stage::Effect, &name, effect);
        effect_knobs(b, rng, &name, effect);
        b.cable(&end, "out", &name, "in");
        end = name;
    }
    end
}

/// A sequencer whose steps are hits or rests, through a `logic` gate.
fn drum(b: &mut Builder, rng: &mut Rng, instrument: &Instrument) -> String {
    let name = instrument.name;
    let (seq, hits, env, vca) = (
        format!("{name} seq"),
        format!("{name} hits"),
        format!("{name} env"),
        format!("{name} vca"),
    );
    b.module(Stage::Sequencer, &seq, "seq");
    b.knob(&seq, "run", 0.0);
    // At least one hit, so the drum is heard wherever it plays.
    let sure_hit = rng.below(8);
    for step in 0..8 {
        let hit = step == sure_hit || rng.chance(50);
        b.knob(
            &seq,
            &format!("step {}", step + 1),
            if hit { 1.0 } else { 0.0 },
        );
    }
    let ticks: &[&str] = match instrument.kind {
        Kind::Hats => &["8th", "16th"],
        _ => &["beat", "8th"],
    };
    b.cable("clock", rng.pick(ticks), &seq, "clock");
    b.cable("song", instrument.track, &seq, "run cv");

    b.module(Stage::Gate, &hits, "logic");
    b.cable(&seq, "cv", &hits, "a");
    b.cable(&seq, "gate", &hits, "b");

    let decay = match instrument.kind {
        Kind::Kick => (0.1, 0.4),
        Kind::Snare => (0.06, 0.2),
        _ => (0.02, 0.08),
    };
    envelope(
        b,
        rng,
        &env,
        [(0.001, 0.005), decay, (0.0, 0.0), (0.03, 0.15)],
    );
    b.cable(&hits, "and", &env, "gate");

    b.module(Stage::Amp, &vca, "vca");
    b.knob(&vca, "gain", 0.0);
    b.cable(&env, "env", &vca, "mod");

    if instrument.kind == Kind::Kick {
        let osc = format!("{name} osc");
        b.module(Stage::Source, &osc, "osc");
        b.knob(&osc, "freq", rng.int(35, 70) as f32);
        b.knob(&osc, "fm depth", rng.int(50, 250) as f32);
        b.cable(&env, "env", &osc, "fm");
        b.cable(&osc, "sine", &vca, "in");
    } else {
        let (noise, filter) = (format!("{name} noise"), format!("{name} filter"));
        b.module(Stage::Source, &noise, "noise");
        b.module(Stage::Filter, &filter, "filter");
        let (cutoff, output) = match instrument.kind {
            Kind::Snare => (rng.int(1000, 3000), "bandpass"),
            _ => (rng.int(5000, 12000), "highpass"),
        };
        b.knob(&filter, "cutoff", cutoff as f32);
        b.knob(&filter, "resonance", rng.value(0.1, 0.5));
        b.cable(&noise, "white", &filter, "in");
        b.cable(&filter, output, &vca, "in");
    }
    vca
}

/// A `notes` module playing an oscillator, through a filter for most.
fn melodic(b: &mut Builder, rng: &mut Rng, instrument: &Instrument, key: &Key) -> String {
    let name = instrument.name;
    let (env, osc, vca) = (
        format!("{name} env"),
        format!("{name} osc"),
        format!("{name} vca"),
    );
    let (octaves, ticks, envelope_ranges, filtered): (_, &[&str], _, _) = match instrument.kind {
        Kind::Bass => (
            (1, 2),
            &["8th", "16th"],
            [(0.001, 0.01), (0.1, 0.3), (0.2, 0.6), (0.03, 0.1)],
            true,
        ),
        Kind::Lead => (
            (3, 5),
            &["8th", "16th"],
            [(0.001, 0.02), (0.1, 0.4), (0.2, 0.7), (0.05, 0.3)],
            rng.chance(50),
        ),
        _ => (
            (3, 4),
            &["beat"],
            [(0.1, 0.8), (0.3, 1.0), (0.5, 0.9), (0.5, 2.0)],
            true,
        ),
    };

    let patterns: BTreeMap<String, String> = (0..instrument.patterns)
        .map(|p| {
            let letter = char::from(b'A' + p as u8).to_string();
            (letter, pattern(rng, instrument.kind, key, octaves))
        })
        .collect();
    let notes = b.module(Stage::Sequencer, name, "notes");
    notes.key = Some(key.name.clone());
    notes.patterns = Some(patterns);
    b.cable("clock", rng.pick(ticks), name, "clock");
    b.cable("song", instrument.track, name, "play");

    envelope(b, rng, &env, envelope_ranges);
    b.cable(name, "gate", &env, "gate");

    b.module(Stage::Source, &osc, "osc");
    // Middle C, so that `pitch` plays the oscillator in tune.
    b.knob(&osc, "freq", 261.63);
    b.cable(name, "pitch", &osc, "freq cv");
    let wave = *rng.pick(&["sine", "saw", "square"]);

    b.module(Stage::Amp, &vca, "vca");
    b.knob(&vca, "gain", 0.0);
    b.cable(&env, "env", &vca, "mod");

    if filtered {
        let filter = format!("{name} filter");
        b.module(Stage::Filter, &filter, "filter");
        b.knob(&filter, "cutoff", rng.int(200, 4000) as f32);
        b.knob(&filter, "resonance", rng.value(0.0, 0.8));
        b.cable(&osc, wave, &filter, "in");
        b.cable(&filter, "lowpass", &vca, "in");
        if rng.chance(35) {
            sweep(b, rng, name, &filter);
        }
    } else {
        b.cable(&osc, wave, &vca, "in");
    }
    vca
}

/// An LFO moving a filter's cutoff, scaled down by an `atten`.
fn sweep(b: &mut Builder, rng: &mut Rng, name: &str, filter: &str) {
    let (lfo, depth) = (format!("{name} lfo"), format!("{name} lfo depth"));
    b.module(Stage::Modulation, &lfo, "lfo");
    if rng.chance(50) {
        b.cable("clock", "bar", &lfo, "clock");
    } else {
        b.knob(&lfo, "rate", rng.value(0.05, 2.0));
    }
    b.module(Stage::Modulation, &depth, "atten");
    b.knob(&depth, "gain a", rng.value(0.05, 0.4));
    let shape = *rng.pick(&["sine", "triangle", "saw", "square"]);
    b.cable(&lfo, shape, &depth, "a");
    b.cable(&depth, "a", filter, "cutoff cv");
}

/// One pattern: a note on its first step, so the instrument is heard
/// wherever it plays, then notes, held notes and rests.
fn pattern(rng: &mut Rng, kind: Kind, key: &Key, octaves: (u32, u32)) -> String {
    // Out of ten steps after the first: notes, then holds; the rest rests.
    let (notes, holds) = match kind {
        Kind::Pad => (3, 6),
        _ => (6, 2),
    };
    let steps = *rng.pick(&[8, 16]);
    let mut text = vec![note(rng, key, octaves)];
    for _ in 1..steps {
        let roll = rng.below(10);
        text.push(if roll < notes {
            note(rng, key, octaves)
        } else if roll < notes + holds {
            "-".to_string()
        } else {
            ".".to_string()
        });
    }
    text.join(" ")
}

/// A note of the key, by name.
fn note(rng: &mut Rng, key: &Key, (low, high): (u32, u32)) -> String {
    let octave = rng.int(low, high) as u8;
    note_name((octave + 1) * 12 + key.root + rng.pick(key.intervals))
}

/// An `adsr`, each of attack, decay, sustain and release drawn in its range.
fn envelope(b: &mut Builder, rng: &mut Rng, name: &str, ranges: [(f32, f32); 4]) {
    b.module(Stage::Envelope, name, "adsr");
    for (knob, (min, max)) in ["attack", "decay", "sustain", "release"].iter().zip(ranges) {
        b.knob(name, knob, rng.value(min, max));
    }
}

/// Knobs that keep an effect audible without drowning the sound.
fn effect_knobs(b: &mut Builder, rng: &mut Rng, name: &str, effect: &str) {
    match effect {
        "dist" => {
            b.knob(name, "drive", rng.value(1.0, 20.0));
            b.knob(name, "fold", rng.value(0.0, 1.0));
        }
        "crush" => {
            b.knob(name, "bits", rng.int(4, 12) as f32);
            b.knob(name, "downsample", rng.int(1, 8) as f32);
        }
        "delay" => {
            b.knob(name, "feedback", rng.value(0.2, 0.7));
            b.knob(name, "mix", rng.value(0.2, 0.5));
            let tick = *rng.pick(&["8th", "dotted 8th", "beat"]);
            b.cable("clock", tick, name, "clock");
        }
        "chorus" => {
            b.knob(name, "rate", rng.value(0.1, 3.0));
            b.knob(name, "depth", rng.value(0.2, 0.8));
            b.knob(name, "mix", rng.value(0.3, 0.6));
        }
        "flanger" => {
            b.knob(name, "rate", rng.value(0.05, 2.0));
            b.knob(name, "depth", rng.value(0.3, 0.9));
            b.knob(name, "time", rng.value(0.001, 0.01));
            b.knob(name, "feedback", rng.value(-0.9, 0.9));
            b.knob(name, "mix", rng.value(0.3, 0.6));
        }
        "phaser" => {
            b.knob(name, "rate", rng.value(0.05, 2.0));
            b.knob(name, "depth", rng.value(0.3, 0.9));
            b.knob(name, "freq", rng.int(300, 2000) as f32);
            b.knob(name, "feedback", rng.value(-0.9, 0.9));
            b.knob(name, "mix", rng.value(0.3, 0.6));
        }
        "univibe" => {
            b.knob(name, "speed", rng.value(0.5, 8.0));
            b.knob(name, "intensity", rng.value(0.3, 0.9));
            b.knob(name, "mix", rng.value(0.3, 0.7));
        }
        _ => unreachable!("{effect} is not in EFFECTS"),
    }
}
