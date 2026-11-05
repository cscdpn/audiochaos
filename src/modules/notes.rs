use crate::engine::module::{BLOCK, Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::engine::score::{PATTERNS, Patterns, Score, Step};
use crate::modules::dsp::ClockIn;
use crate::modules::midi::pitch_cv;

pub const IN_CLOCK: usize = 0;
pub const IN_PLAY: usize = 1;

/// The note's pitch, for an oscillator's `freq cv` with `freq` on middle C.
pub const OUT_PITCH: usize = 0;
/// 1 while a note sounds; it closes for the second half of a note's last
/// step, so the next note starts afresh.
pub const OUT_GATE: usize = 1;

pub const RATE: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "notes",
    description: "Melodies by note, in patterns A to D drawn on a grid: a step per tick on `clock`. `play` picks the pattern, as a song's track gives it: 0 stops, 1 plays A, 2 B... Unplugged, it plays A. `pitch` into an oscillator's `freq cv`, with `freq` on middle C (261.63 Hz).",
    inputs: &["clock", "play"],
    outputs: &["pitch", "gate"],
    output_details: &["note, for freq cv", "0 or 1"],
    params: &[ParamInfo {
        name: "rate",
        min: 0.1,
        max: 50.0,
        default: 4.0,
        scale: Scale::Exponential,
        unit: "st/s",
        help: "Steps per second. Ignored while `clock` has a cable.",
    }],
};

/// The pattern a `play` level asks for: 0 stops, 1 is A, 2 is B...
fn pattern_for(level: f32) -> Option<usize> {
    (level > 0.5).then(|| (level.round() as usize).clamp(1, PATTERNS) - 1)
}

/// The note sounding at `step`, if any: a note, or the note a run of holds
/// carries on.
fn note_at(steps: &[Step], step: usize) -> Option<u8> {
    steps[..=step].iter().rev().find_map(|s| match s {
        Step::Hold => None,
        Step::Rest => Some(None),
        Step::Note(note) => Some(Some(*note)),
    })?
}

/// A step sequencer that plays notes: patterns of notes, holds and rests,
/// a step per clock tick or at its own rate.
///
/// Starting to play, or switching patterns, starts from the first step: on
/// the current tick if the clock is high, since a song's track may come a
/// block after the tick that went with it, else on the next tick. Stopped,
/// it holds its pitch with the gate closed, so notes fade out in tune.
pub struct Notes {
    sample_rate: f32,
    patterns: Box<Score>,
    /// Position in steps.
    position: f32,
    clock: ClockIn,
    clocked: bool,
    play_plugged: bool,
    /// The pattern playing, if any.
    playing: Option<usize>,
    pitch: f32,
    gate: f32,
}

impl Notes {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            patterns: Box::new(Score::Patterns(Patterns::default())),
            position: 0.0,
            clock: ClockIn::default(),
            clocked: false,
            play_plugged: false,
            playing: None,
            pitch: 0.0,
            gate: 0.0,
        }
    }

    fn steps(&self, pattern: usize) -> &[Step] {
        match &*self.patterns {
            Score::Patterns(patterns) => &patterns.patterns[pattern],
            Score::Arrangement(_) => unreachable!("set_score only takes patterns"),
        }
    }

    /// Back to the first step, or just before it to wait for the next tick.
    fn start_over(&mut self, clock: f32, length: usize) {
        self.position = if self.clocked && clock <= 0.0 {
            length.saturating_sub(1) as f32
        } else {
            0.0
        };
    }
}

impl Module for Notes {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let [pitch, gate] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for i in 0..BLOCK {
            let clock = inputs[IN_CLOCK][i];
            let ticked = self.clock.tick(clock);
            let wanted = if self.play_plugged {
                pattern_for(inputs[IN_PLAY][i])
            } else {
                Some(0)
            };
            let length = wanted.map_or(0, |p| self.steps(p).len());
            if wanted != self.playing {
                self.playing = wanted;
                self.start_over(clock, length);
            } else if ticked && self.clocked && length > 0 {
                self.position = (self.position.floor() + 1.0) % length as f32;
            }
            gate[i] = 0.0;
            if let Some(p) = wanted.filter(|_| length > 0) {
                // The pattern may have just become shorter.
                self.position %= length as f32;
                let step = self.position as usize;
                let steps = self.steps(p);
                let sounding = note_at(steps, step);
                let last = steps.get(step + 1) != Some(&Step::Hold);
                if let Some(note) = sounding {
                    self.pitch = pitch_cv(note, 0.0);
                    let first_half = if self.clocked {
                        clock > 0.0
                    } else {
                        self.position.fract() < 0.5
                    };
                    if !last || first_half {
                        gate[i] = 1.0;
                    }
                }
                if !self.clocked {
                    self.position =
                        (self.position + params[RATE][i] / self.sample_rate) % length as f32;
                }
            }
            pitch[i] = self.pitch;
        }
        self.gate = gate[BLOCK - 1];
    }

    fn set_plugged(&mut self, input: usize, plugged: bool) {
        match input {
            IN_CLOCK => {
                self.clocked = plugged;
                // Just before the first step, so the first tick plays it.
                let length = self.playing.map_or(0, |p| self.steps(p).len());
                self.position = length.saturating_sub(1) as f32;
            }
            _ => {
                self.play_plugged = plugged;
                self.playing = None;
            }
        }
    }

    fn set_score(&mut self, score: Box<Score>) -> Option<Box<Score>> {
        match *score {
            Score::Patterns(_) => Some(std::mem::replace(&mut self.patterns, score)),
            Score::Arrangement(_) => Some(score),
        }
    }

    fn score(&self) -> Option<&Score> {
        Some(&self.patterns)
    }

    fn led(&self) -> Option<f32> {
        Some(self.gate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::score::parse_steps;

    const SR: f32 = 48_000.0;
    /// One step per block, so each block plays one step.
    const ONE_STEP_PER_BLOCK: f32 = SR / BLOCK as f32;

    fn notes(patterns: [&str; PATTERNS]) -> Notes {
        let mut notes = Notes::new(SR);
        let patterns = Patterns {
            patterns: patterns.map(|p| parse_steps(p).unwrap()),
            ..Default::default()
        };
        notes.set_score(Box::new(Score::Patterns(patterns)));
        notes
    }

    /// Plays one block; returns its pitch as a note number, and the gate at
    /// its start and end.
    fn play(notes: &mut Notes, clock: f32, play: f32) -> (i32, f32, f32) {
        let mut outputs = [[0.0; BLOCK]; 2];
        notes.process(
            &[[clock; BLOCK], [play; BLOCK]],
            &[[ONE_STEP_PER_BLOCK; BLOCK]],
            &mut outputs,
        );
        let [pitch, gate] = outputs;
        let note = 60 + (pitch[1] / pitch_cv(61, 0.0)).round() as i32;
        (note, gate[1], gate[BLOCK - 1])
    }

    #[test]
    fn plays_pattern_a_with_holds_and_rests() {
        let mut n = notes(["C4 - E4 . G4", "", "", ""]);
        let played: Vec<_> = (0..6).map(|_| play(&mut n, 0.0, 0.0)).collect();
        assert_eq!(
            played,
            [
                (60, 1.0, 1.0), // held into the next step
                (60, 1.0, 0.0), // closes before E
                (64, 1.0, 0.0),
                (64, 0.0, 0.0), // a rest keeps the pitch, gate closed
                (67, 1.0, 0.0),
                (60, 1.0, 1.0), // and again
            ]
        );
        assert_eq!(n.led(), Some(1.0));
    }

    #[test]
    fn play_picks_the_pattern_and_starts_it_over() {
        let mut n = notes(["C4 D4 E4", "A4 B4", "", ""]);
        n.set_plugged(IN_PLAY, true);
        assert_eq!(play(&mut n, 0.0, 0.0).1, 0.0, "stopped");
        assert_eq!(play(&mut n, 0.0, 1.0).0, 60);
        assert_eq!(play(&mut n, 0.0, 1.0).0, 62);
        assert_eq!(play(&mut n, 0.0, 2.0).0, 69, "B from its first step");
        assert_eq!(play(&mut n, 0.0, 2.0).0, 71);
        // An empty pattern plays nothing.
        assert_eq!(play(&mut n, 0.0, 3.0).1, 0.0);
        assert_eq!(play(&mut n, 0.0, 1.0).0, 60, "A starts over");
    }

    #[test]
    fn a_clock_steps_it_from_the_first_step() {
        let mut n = notes(["C4 D4 E4", "", "", ""]);
        n.set_plugged(IN_CLOCK, true);
        let played: Vec<_> = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0]
            .iter()
            .map(|&clock| play(&mut n, clock, 0.0))
            .collect();
        let pitches: Vec<i32> = played.iter().map(|p| p.0).collect();
        // Silent until the first tick, then a step per rising edge.
        assert_eq!(played[0].1, 0.0);
        assert_eq!(pitches[1..], [60, 60, 62, 62, 64, 64]);
        // The gate follows the clock on each note's last step.
        assert_eq!((played[1].1, played[2].1), (1.0, 0.0));
    }

    #[test]
    fn a_hold_at_the_start_holds_nothing() {
        assert_eq!(note_at(&[Step::Hold, Step::Note(60)], 0), None);
        assert_eq!(
            note_at(&[Step::Note(60), Step::Hold, Step::Hold], 2),
            Some(60)
        );
        assert_eq!(note_at(&[Step::Note(60), Step::Rest, Step::Hold], 2), None);
    }

    #[test]
    fn arrangements_are_handed_back() {
        let mut n = Notes::new(SR);
        let arrangement = Box::new(Score::Arrangement(Default::default()));
        assert_eq!(n.set_score(arrangement.clone()), Some(arrangement));
        assert!(matches!(n.score(), Some(Score::Patterns(_))));
    }
}
