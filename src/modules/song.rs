use std::sync::Arc;

use crate::engine::module::{BLOCK, Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::engine::transport::Transport;
use crate::modules::dsp::{ClockIn, flash};

pub const IN_CLOCK: usize = 0;

pub const SECTIONS: usize = 8;
pub const LOOP: usize = SECTIONS;

/// Parameter index of a section's length, which is also the index of its
/// gate output.
pub const fn section(n: usize) -> usize {
    n
}

/// Beats to the bar in a DAW, as `clock` assumes.
const BEATS_PER_BAR: f64 = 4.0;

const fn length(name: &'static str) -> ParamInfo {
    ParamInfo {
        name,
        min: 0.0,
        max: 64.0,
        default: 4.0,
        scale: Scale::Linear,
        unit: "bars",
        help: "How many bars this section lasts, rounded. At 0 the song skips it.",
    }
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "song",
    description: "Arranges a whole piece in up to eight sections, counting bars on `clock` (a clock's `bar`). Each section's gate is high while it plays: into a seq's `run cv` or a VCA's `mod`, it brings an instrument in and out.",
    inputs: &["clock"],
    outputs: &[
        "section 1",
        "section 2",
        "section 3",
        "section 4",
        "section 5",
        "section 6",
        "section 7",
        "section 8",
    ],
    output_details: &["0 or 1"; SECTIONS],
    params: &[
        length("length 1"),
        length("length 2"),
        length("length 3"),
        length("length 4"),
        length("length 5"),
        length("length 6"),
        length("length 7"),
        length("length 8"),
        ParamInfo {
            name: "loop",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            scale: Scale::Linear,
            unit: "",
            help: "Past halfway, the song starts over after its last section. Below, it ends there and every gate closes.",
        },
    ],
};

/// The section playing at `bar`, counting from the song's first bar, if
/// any: the song may have ended, or have no bars at all.
fn section_at(bar: u32, lengths: &[u32; SECTIONS], looping: bool) -> Option<usize> {
    let total: u32 = lengths.iter().sum();
    let mut bar = if looping && total > 0 {
        bar % total
    } else {
        bar
    };
    for (n, &length) in lengths.iter().enumerate() {
        if bar < length {
            return Some(n);
        }
        bar -= length;
    }
    None
}

/// An arranger: plays sections one after the other, each for its length in
/// bars, with a gate per section that is high while it plays.
///
/// Each rising edge on `clock` is a bar; the first one starts the first
/// section, and the gates are closed until then. The position is a count of
/// bars from the start of the song, so changing a length moves the sections
/// after it at once. Plugging or unplugging the clock starts the song over.
///
/// In the plugin, while the DAW plays, the song is where the DAW's song is
/// (assuming four beats to the bar), and `clock` only sets the light's pace.
pub struct Song {
    clock: ClockIn,
    transport: Arc<Transport>,
    /// Bars since the first, None before the first tick.
    bar: Option<u32>,
    /// Samples since the last tick, for the light.
    since_tick: u32,
    playing: bool,
}

impl Song {
    pub fn new(transport: Arc<Transport>) -> Self {
        Self {
            clock: ClockIn::default(),
            transport,
            bar: None,
            since_tick: 0,
            playing: false,
        }
    }
}

impl Module for Song {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let daw_beats = self.transport.beats();
        if let Some(beats) = daw_beats {
            self.bar = (beats >= 0.0).then(|| (beats / BEATS_PER_BAR).floor() as u32);
        }
        for i in 0..BLOCK {
            if self.clock.tick(inputs[IN_CLOCK][i]) {
                self.since_tick = 0;
                if daw_beats.is_none() {
                    self.bar = Some(self.bar.map_or(0, |bar| bar + 1));
                }
            } else {
                self.since_tick = self.since_tick.saturating_add(1);
            }
            let lengths = std::array::from_fn(|n| params[section(n)][i].round() as u32);
            let looping = params[LOOP][i] > 0.5;
            let playing = self.bar.and_then(|bar| section_at(bar, &lengths, looping));
            for (n, out) in outputs.iter_mut().enumerate() {
                out[i] = if playing == Some(n) { 1.0 } else { 0.0 };
            }
            self.playing = playing.is_some();
        }
    }

    fn set_plugged(&mut self, _input: usize, _plugged: bool) {
        self.bar = None;
    }

    fn led(&self) -> Option<f32> {
        let period = self.clock.period().filter(|_| self.playing);
        Some(period.map_or(0.0, |p| flash((self.since_tick as f32 / p as f32).min(1.0))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two bars, one bar, skipped, three bars, the rest skipped.
    fn params(looping: bool) -> Vec<f32> {
        let mut p = vec![2.0, 1.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0];
        p.push(if looping { 1.0 } else { 0.0 });
        p
    }

    /// Plays one block with `clock` held at a level; returns the section
    /// whose gate is high at its end, checking that no other gate is.
    fn play(song: &mut Song, clock: f32, params: &[f32]) -> Option<usize> {
        let params: Vec<Buffer> = params.iter().map(|&v| [v; BLOCK]).collect();
        let mut outputs = [[0.0; BLOCK]; SECTIONS];
        song.process(&[[clock; BLOCK]], &params, &mut outputs);
        let high: Vec<usize> = (0..SECTIONS)
            .filter(|&n| outputs[n][BLOCK - 1] == 1.0)
            .collect();
        assert!(high.len() <= 1, "{high:?}");
        high.first().copied()
    }

    /// Plays `count` bars, a tick and a block of silence each; returns the
    /// section of each bar.
    fn bars(song: &mut Song, count: usize, params: &[f32]) -> Vec<Option<usize>> {
        (0..count)
            .map(|_| {
                let section = play(song, 1.0, params);
                assert_eq!(play(song, 0.0, params), section, "held between ticks");
                section
            })
            .collect()
    }

    #[test]
    fn sections_play_in_turn_for_their_length_and_empty_ones_are_skipped() {
        let mut song = Song::new(Arc::default());
        assert_eq!(
            play(&mut song, 0.0, &params(false)),
            None,
            "before the first tick"
        );
        assert_eq!(
            bars(&mut song, 6, &params(false)),
            [Some(0), Some(0), Some(1), Some(3), Some(3), Some(3)]
        );
    }

    #[test]
    fn the_song_ends_after_its_last_section_unless_it_loops() {
        let mut song = Song::new(Arc::default());
        bars(&mut song, 6, &params(false));
        assert_eq!(bars(&mut song, 2, &params(false)), [None, None]);

        let mut song = Song::new(Arc::default());
        bars(&mut song, 6, &params(true));
        assert_eq!(
            bars(&mut song, 3, &params(true)),
            [Some(0), Some(0), Some(1)]
        );
    }

    #[test]
    fn a_shorter_section_moves_the_ones_after_it_at_once() {
        let mut song = Song::new(Arc::default());
        bars(&mut song, 2, &params(false));
        let mut shorter = params(false);
        shorter[section(0)] = 1.0;
        // The second bar of the song is now in the second section.
        assert_eq!(play(&mut song, 0.0, &shorter), Some(1));
    }

    #[test]
    fn plugging_the_clock_starts_the_song_over() {
        let mut song = Song::new(Arc::default());
        bars(&mut song, 3, &params(false));
        song.set_plugged(IN_CLOCK, true);
        assert_eq!(play(&mut song, 0.0, &params(false)), None);
        assert_eq!(bars(&mut song, 1, &params(false)), [Some(0)]);
    }

    #[test]
    fn a_playing_daw_sets_the_bar() {
        let transport = Arc::new(Transport::default());
        let mut song = Song::new(Arc::clone(&transport));
        // Into the fourth bar: the second of section 4, after 2 + 1 bars.
        transport.set(Some(120.0), Some(4.0 * 4.0 + 1.5));
        assert_eq!(play(&mut song, 0.0, &params(false)), Some(3));
        // Ticks do not count while the DAW plays, and it jumps back.
        assert_eq!(play(&mut song, 1.0, &params(false)), Some(3));
        transport.set(Some(120.0), Some(0.0));
        assert_eq!(play(&mut song, 0.0, &params(false)), Some(0));
    }

    #[test]
    fn the_light_flashes_on_each_bar_while_a_section_plays() {
        let mut song = Song::new(Arc::default());
        let p = params(false);
        // A bar of eight blocks; the light just after its tick, and late in it.
        let bar = |song: &mut Song| {
            play(song, 1.0, &p);
            let just_after = song.led().unwrap();
            for _ in 0..7 {
                play(song, 0.0, &p);
            }
            (just_after, song.led().unwrap())
        };
        bar(&mut song);
        let (just_after, late) = bar(&mut song);
        assert!(just_after > 0.5, "{just_after}");
        assert!(late < 0.1, "{late}");
        // Past the song's six bars, it stays dark.
        for _ in 0..5 {
            bar(&mut song);
        }
        play(&mut song, 1.0, &p);
        assert_eq!(song.led(), Some(0.0));
    }

    #[test]
    fn a_song_without_bars_plays_nothing_even_looping() {
        let lengths = [0; SECTIONS];
        assert_eq!(section_at(0, &lengths, true), None);
        assert_eq!(section_at(5, &lengths, false), None);
    }
}
