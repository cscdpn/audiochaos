use std::sync::Arc;

use crate::engine::module::{BLOCK, Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::engine::score::{Arrangement, Score, TRACKS};
use crate::engine::transport::Transport;
use crate::modules::dsp::{ClockIn, flash};

pub const IN_CLOCK: usize = 0;

pub const LOOP: usize = 0;

/// Output index of a track.
pub const fn track(n: usize) -> usize {
    n
}

/// Beats to the bar in a DAW, as `clock` assumes.
const BEATS_PER_BAR: f64 = 4.0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "song",
    description: "Arranges a whole piece: sections played one after the other, each so many bars long on `clock` (a clock's `bar`), and in each, which tracks play. A track's output is 1 while it plays pattern A, 2 for B..., 0 when silent: into a `notes` module's `play`, a seq's `run cv` or a VCA's `mod`.",
    inputs: &["clock"],
    outputs: &[
        "track 1", "track 2", "track 3", "track 4", "track 5", "track 6", "track 7", "track 8",
    ],
    output_details: &["0, or pattern 1..4"; TRACKS],
    params: &[ParamInfo {
        name: "loop",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        scale: Scale::Linear,
        unit: "",
        help: "Past halfway, the song starts over after its last section. Below, it ends there and every track falls silent.",
    }],
};

/// The section playing at `bar`, counting from the song's first bar, if
/// any: the song may have ended, or have no bars at all.
pub fn section_at(bar: u32, arrangement: &Arrangement, looping: bool) -> Option<usize> {
    let total: u32 = arrangement.sections.iter().map(|s| s.bars).sum();
    let mut bar = if looping && total > 0 {
        bar % total
    } else {
        bar
    };
    for (n, section) in arrangement.sections.iter().enumerate() {
        if bar < section.bars {
            return Some(n);
        }
        bar -= section.bars;
    }
    None
}

/// An arranger: plays its sections one after the other, each for its
/// length in bars, and says on each track's output which pattern it plays.
///
/// Each rising edge on `clock` is a bar; the first one starts the first
/// section, and every track is silent until then. The position is a count
/// of bars from the start of the song, so changing a length moves the
/// sections after it at once. Plugging or unplugging the clock starts the
/// song over.
///
/// In the plugin, while the DAW plays, the song is where the DAW's song is
/// (assuming four beats to the bar), and `clock` only sets the light's pace.
pub struct Song {
    arrangement: Box<Score>,
    clock: ClockIn,
    transport: Arc<Transport>,
    /// Bars since the first, None before the first tick.
    bar: Option<u32>,
    /// Samples since the last tick, for the light.
    since_tick: u32,
    /// The section playing, for the editor.
    section: Option<usize>,
}

impl Song {
    pub fn new(transport: Arc<Transport>) -> Self {
        Self {
            arrangement: Box::new(Score::Arrangement(Arrangement::default())),
            clock: ClockIn::default(),
            transport,
            bar: None,
            since_tick: 0,
            section: None,
        }
    }

    fn arrangement(&self) -> &Arrangement {
        match &*self.arrangement {
            Score::Arrangement(arrangement) => arrangement,
            Score::Patterns(_) => unreachable!("set_score only takes arrangements"),
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
            let looping = params[LOOP][i] > 0.5;
            let arrangement = self.arrangement();
            let section = self
                .bar
                .and_then(|bar| section_at(bar, arrangement, looping));
            let tracks = section.map(|s| arrangement.sections[s].tracks);
            for (n, out) in outputs.iter_mut().enumerate() {
                let pattern = tracks.and_then(|tracks| tracks[n]);
                out[i] = pattern.map_or(0.0, |p| f32::from(p) + 1.0);
            }
            self.section = section;
        }
    }

    fn set_plugged(&mut self, _input: usize, _plugged: bool) {
        self.bar = None;
    }

    fn set_score(&mut self, score: Box<Score>) -> Option<Box<Score>> {
        match *score {
            Score::Arrangement(_) => Some(std::mem::replace(&mut self.arrangement, score)),
            Score::Patterns(_) => Some(score),
        }
    }

    fn score(&self) -> Option<&Score> {
        Some(&self.arrangement)
    }

    fn led(&self) -> Option<f32> {
        let period = self.clock.period().filter(|_| self.section.is_some());
        Some(period.map_or(0.0, |p| flash((self.since_tick as f32 / p as f32).min(1.0))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::score::Section;

    /// Two bars, one bar, skipped, three bars: track 1 plays A in the first
    /// and B in the last, track 2 A in the second.
    fn arrangement() -> Box<Score> {
        let section = |bars, tracks: [Option<u8>; 2]| {
            let mut all = [None; TRACKS];
            all[..2].copy_from_slice(&tracks);
            Section {
                name: String::new(),
                bars,
                tracks: all,
            }
        };
        Box::new(Score::Arrangement(Arrangement {
            sections: vec![
                section(2, [Some(0), None]),
                section(1, [None, Some(0)]),
                section(0, [Some(2), Some(2)]),
                section(3, [Some(1), None]),
            ],
        }))
    }

    fn song(transport: Arc<Transport>) -> Song {
        let mut song = Song::new(transport);
        song.set_score(arrangement());
        song
    }

    fn params(looping: bool) -> [f32; 1] {
        [if looping { 1.0 } else { 0.0 }]
    }

    /// Plays one block with `clock` held at a level; returns the first two
    /// tracks at its end, checking the others are silent.
    fn play(song: &mut Song, clock: f32, params: &[f32]) -> [f32; 2] {
        let params: Vec<Buffer> = params.iter().map(|&v| [v; BLOCK]).collect();
        let mut outputs = [[0.0; BLOCK]; TRACKS];
        song.process(&[[clock; BLOCK]], &params, &mut outputs);
        assert!(outputs[2..].iter().all(|o| o[BLOCK - 1] == 0.0));
        [outputs[0][BLOCK - 1], outputs[1][BLOCK - 1]]
    }

    /// Plays `count` bars, a tick and a block of silence each; returns the
    /// first two tracks in each bar.
    fn bars(song: &mut Song, count: usize, params: &[f32]) -> Vec<[f32; 2]> {
        (0..count)
            .map(|_| {
                let tracks = play(song, 1.0, params);
                assert_eq!(play(song, 0.0, params), tracks, "held between ticks");
                tracks
            })
            .collect()
    }

    const SILENT: [f32; 2] = [0.0, 0.0];

    #[test]
    fn sections_play_in_turn_and_tracks_say_their_pattern() {
        let mut song = song(Arc::default());
        assert_eq!(
            play(&mut song, 0.0, &params(false)),
            SILENT,
            "before the first tick"
        );
        assert_eq!(
            bars(&mut song, 6, &params(false)),
            [
                [1.0, 0.0],
                [1.0, 0.0],
                [0.0, 1.0],
                [2.0, 0.0],
                [2.0, 0.0],
                [2.0, 0.0]
            ]
        );
    }

    #[test]
    fn the_song_ends_after_its_last_section_unless_it_loops() {
        let mut song = self::song(Arc::default());
        bars(&mut song, 6, &params(false));
        assert_eq!(bars(&mut song, 2, &params(false)), [SILENT, SILENT]);

        let mut song = self::song(Arc::default());
        bars(&mut song, 6, &params(true));
        assert_eq!(
            bars(&mut song, 3, &params(true)),
            [[1.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
        );
    }

    #[test]
    fn a_new_arrangement_moves_the_song_at_once() {
        let mut song = song(Arc::default());
        bars(&mut song, 2, &params(false));
        let Score::Arrangement(mut shorter) = *arrangement() else {
            unreachable!()
        };
        shorter.sections[0].bars = 1;
        let old = song.set_score(Box::new(Score::Arrangement(shorter)));
        assert_eq!(old, Some(arrangement()), "the old one comes back");
        // The second bar of the song is now in the second section.
        assert_eq!(play(&mut song, 0.0, &params(false)), [0.0, 1.0]);
    }

    #[test]
    fn patterns_are_handed_back() {
        let mut song = song(Arc::default());
        let patterns = Box::new(Score::Patterns(Default::default()));
        assert_eq!(song.set_score(patterns.clone()), Some(patterns));
        assert_eq!(song.score(), Some(&*arrangement()));
    }

    #[test]
    fn plugging_the_clock_starts_the_song_over() {
        let mut song = song(Arc::default());
        bars(&mut song, 3, &params(false));
        song.set_plugged(IN_CLOCK, true);
        assert_eq!(play(&mut song, 0.0, &params(false)), SILENT);
        assert_eq!(bars(&mut song, 1, &params(false)), [[1.0, 0.0]]);
    }

    #[test]
    fn a_playing_daw_sets_the_bar() {
        let transport = Arc::new(Transport::default());
        let mut song = song(Arc::clone(&transport));
        // Into the fourth bar: the second of the last section, after 2 + 1.
        transport.set(Some(120.0), Some(4.0 * 4.0 + 1.5));
        assert_eq!(play(&mut song, 0.0, &params(false)), [2.0, 0.0]);
        // Ticks do not count while the DAW plays, and it jumps back.
        assert_eq!(play(&mut song, 1.0, &params(false)), [2.0, 0.0]);
        transport.set(Some(120.0), Some(0.0));
        assert_eq!(play(&mut song, 0.0, &params(false)), [1.0, 0.0]);
    }

    #[test]
    fn the_light_flashes_on_each_bar_while_a_section_plays() {
        let mut song = song(Arc::default());
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
        let empty = Arrangement {
            sections: Vec::new(),
        };
        assert_eq!(section_at(0, &empty, true), None);
        assert_eq!(section_at(5, &empty, false), None);
    }
}
