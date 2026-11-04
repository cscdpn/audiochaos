use std::sync::Arc;

use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::engine::transport::Transport;
use crate::modules::dsp::{ClockIn, flash};

pub const IN_TAP: usize = 0;

pub const OUT_BAR: usize = 0;
pub const OUT_BEAT: usize = 1;
pub const OUT_EIGHTH: usize = 2;
pub const OUT_DOTTED_EIGHTH: usize = 3;
pub const OUT_SIXTEENTH: usize = 4;

pub const TEMPO: usize = 0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "clock",
    description: "A tempo as gates, to keep delays, LFOs and sequencers in time. Tap it on `tap` or with the Tap button. In a DAW, it follows the song's tempo, and its bars while the song plays.",
    inputs: &["tap"],
    outputs: &["bar", "beat", "8th", "dotted 8th", "16th"],
    output_details: &[
        "gate, 4 beats",
        "gate",
        "gate, ½ beat",
        "gate, ¾ beat",
        "gate, ¼ beat",
    ],
    params: &[ParamInfo {
        name: "tempo",
        min: 20.0,
        max: 300.0,
        default: 120.0,
        scale: Scale::Linear,
        unit: "BPM",
        help: "Beats per minute. Ignored while `tap` has a cable, and in a DAW, which sets the tempo.",
    }],
};

/// Each output's period, in beats.
const PERIODS: [f64; 5] = [4.0, 1.0, 0.5, 0.75, 0.25];
/// Beats after which every output starts a period at once.
const CYCLE: f64 = 12.0;
/// A pause this long starts a new tempo: slower than the slowest one a
/// clock takes, 20 BPM.
const RESTART_SECONDS: f64 = 3.0;
/// Taps averaged, so one tap a little off barely moves the tempo.
const TAPS_KEPT: usize = 5;

/// Tap tempo: a tempo from the rhythm of taps. Fixed-size, so the audio
/// thread can use it as well as the editor's Tap button.
#[derive(Default)]
pub struct Tap {
    /// Times of the latest taps, in seconds, oldest first.
    taps: [f64; TAPS_KEPT],
    count: usize,
}

impl Tap {
    /// Records a tap at `time`, in seconds. Returns the tempo in BPM from the
    /// taps so far, or None for the first tap of a new tempo.
    pub fn tap(&mut self, time: f64) -> Option<f32> {
        if self.count > 0 && time - self.taps[self.count - 1] > RESTART_SECONDS {
            self.count = 0;
        }
        if self.count == TAPS_KEPT {
            self.taps.copy_within(1.., 0);
            self.count -= 1;
        }
        self.taps[self.count] = time;
        self.count += 1;
        let intervals = self.count - 1;
        let span = time - self.taps[0];
        (intervals > 0).then(|| (60.0 * intervals as f64 / span) as f32)
    }
}

/// A tempo as gates, high for the first half of each period, to drive the
/// `clock` inputs of delays, LFOs and sequencers, or anything triggered.
///
/// The editor's Tap button sets the `tempo` knob. With a cable in `tap`, the
/// rhythm of its rising edges sets the tempo instead, a MIDI pad making a tap
/// tempo pedal: the first tap of a new tempo starts a bar, the next ones
/// pull the nearest beat onto them.
///
/// In the plugin, the DAW's tempo overrides both, and while the song plays,
/// each block starts where the song is, so bars line up with the song's
/// (assuming four beats to the bar). Stopped, it runs on at the DAW's tempo.
pub struct Clock {
    sample_rate: f32,
    /// In f64: in f32, adding a sample's worth of beats would lose about a
    /// percent to rounding, and the tempo would drift.
    beats: f64,
    edges: ClockIn,
    tap_plugged: bool,
    tap: Tap,
    /// The tempo tapped on `tap`, once there is one.
    tapped: Option<f32>,
    /// Samples played, to time taps.
    now: u64,
    transport: Arc<Transport>,
}

impl Clock {
    pub fn new(sample_rate: f32, transport: Arc<Transport>) -> Self {
        Self {
            sample_rate,
            beats: 0.0,
            edges: ClockIn::default(),
            tap_plugged: false,
            tap: Tap::default(),
            tapped: None,
            now: 0,
            transport,
        }
    }

    fn on_tap(&mut self) {
        let seconds = self.now as f64 / f64::from(self.sample_rate);
        let tempo = &INFO.params[TEMPO];
        match self.tap.tap(seconds) {
            None => self.beats = 0.0,
            Some(bpm) => {
                self.tapped = Some(tempo.clamp(bpm));
                self.beats = self.beats.round() % CYCLE;
            }
        }
    }
}

impl Module for Clock {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let host_tempo = self.transport.tempo();
        if let Some(beats) = self.transport.beats() {
            self.beats = beats.rem_euclid(CYCLE);
        }
        for (i, &knob) in params[TEMPO].iter().enumerate() {
            if self.edges.tick(inputs[IN_TAP][i]) && self.tap_plugged {
                self.on_tap();
            }
            let tempo = host_tempo.unwrap_or_else(|| f64::from(self.tapped.unwrap_or(knob)));
            for (out, period) in outputs.iter_mut().zip(PERIODS) {
                out[i] = if self.beats % period < period / 2.0 {
                    1.0
                } else {
                    0.0
                };
            }
            let step = tempo / 60.0 / f64::from(self.sample_rate);
            self.beats = (self.beats + step) % CYCLE;
            self.now += 1;
        }
    }

    fn set_plugged(&mut self, _input: usize, plugged: bool) {
        // Back to the knob when unplugged; a new pad taps a new tempo.
        self.tap_plugged = plugged;
        self.tap = Tap::default();
        self.tapped = None;
    }

    fn led(&self) -> Option<f32> {
        Some(flash(self.beats.fract() as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::{run, run_signal};

    const SR: f32 = 48_000.0;

    fn rising_edges(signal: &[f32]) -> Vec<usize> {
        (1..signal.len())
            .filter(|&n| signal[n - 1] <= 0.0 && signal[n] > 0.0)
            .collect()
    }

    #[test]
    fn each_output_ticks_at_its_division_of_the_tempo() {
        // 120 BPM: a beat every 24,000 samples. Twelve seconds, a whole cycle.
        for (port, beats) in [
            (OUT_BAR, 4.0),
            (OUT_BEAT, 1.0),
            (OUT_EIGHTH, 0.5),
            (OUT_DOTTED_EIGHTH, 0.75),
            (OUT_SIXTEENTH, 0.25),
        ] {
            let out = run(
                &mut Clock::new(SR, Arc::default()),
                &[0.0],
                &[120.0],
                port,
                12 * 48_000 / BLOCK,
            );
            let edges = rising_edges(&out);
            let period = (24_000.0 * beats) as usize;
            for pair in edges.windows(2) {
                assert!((pair[1] - pair[0]).abs_diff(period) <= 1, "port {port}");
            }
            assert_eq!(edges.len(), (24.0 / beats) as usize - 1, "port {port}");
        }
    }

    #[test]
    fn a_daw_tempo_overrides_the_knob() {
        let transport = Arc::new(Transport::default());
        transport.set(Some(60.0), None);
        let mut clock = Clock::new(SR, Arc::clone(&transport));
        let out = run(&mut clock, &[0.0], &[120.0], OUT_BEAT, 4 * 48_000 / BLOCK);
        let edges = rising_edges(&out);
        assert_eq!(edges.len(), 3, "one beat a second, not two");
        assert!((edges[1] - edges[0]).abs_diff(48_000) <= 1);
    }

    #[test]
    fn a_playing_daw_sets_where_the_bar_is() {
        let transport = Arc::new(Transport::default());
        let mut clock = Clock::new(SR, Arc::clone(&transport));
        // Halfway through the first beat of the second bar.
        transport.set(Some(120.0), Some(4.5));
        let mut first = |port| run(&mut clock, &[0.0], &[120.0], port, 1)[0];
        assert_eq!(first(OUT_BEAT), 0.0, "second half of a beat");
        transport.set(Some(120.0), Some(4.5));
        assert_eq!(first(OUT_EIGHTH), 1.0, "first half of an eighth");
        transport.set(Some(120.0), Some(4.5));
        assert_eq!(first(OUT_BAR), 1.0, "first half of the bar");
        // The song jumps back to its start: so does the clock.
        transport.set(Some(120.0), Some(0.0));
        run(&mut clock, &[0.0], &[120.0], OUT_BEAT, 1);
        assert!(clock.beats < 0.01, "{}", clock.beats);
    }

    #[test]
    fn gates_are_high_for_half_their_period() {
        let out = run(
            &mut Clock::new(SR, Arc::default()),
            &[0.0],
            &[120.0],
            OUT_BEAT,
            2 * 48_000 / BLOCK,
        );
        let high = out.iter().filter(|&&s| s == 1.0).count();
        assert!(high.abs_diff(out.len() / 2) <= 2, "{high}");
    }

    #[test]
    fn the_light_flashes_on_each_beat() {
        let mut clock = Clock::new(SR, Arc::default());
        // Just past a beat, then just before the next one.
        run(&mut clock, &[0.0], &[120.0], OUT_BEAT, 24_000 / BLOCK + 1);
        assert!(clock.led().unwrap() > 0.9);
        run(&mut clock, &[0.0], &[120.0], OUT_BEAT, 24_000 / BLOCK - 2);
        assert!(clock.led().unwrap() < 0.1);
    }

    #[test]
    fn two_taps_make_a_tempo() {
        let mut tap = Tap::default();
        assert_eq!(tap.tap(10.0), None);
        assert_eq!(tap.tap(10.5), Some(120.0));
    }

    #[test]
    fn taps_are_averaged() {
        let mut tap = Tap::default();
        for time in [0.0, 0.5, 1.0, 1.6] {
            tap.tap(time);
        }
        // Four intervals over two seconds, however uneven.
        assert_eq!(tap.tap(2.0), Some(120.0));
    }

    #[test]
    fn only_the_latest_taps_count() {
        let mut tap = Tap::default();
        for n in 0..10 {
            tap.tap(f64::from(n));
        }
        // Slowing down: the old 60 BPM taps fade out of the average.
        let mut bpm = None;
        for n in 1..=TAPS_KEPT {
            bpm = tap.tap(9.0 + 2.0 * n as f64);
        }
        assert_eq!(bpm, Some(30.0));
    }

    #[test]
    fn a_long_pause_starts_over() {
        let mut tap = Tap::default();
        tap.tap(0.0);
        tap.tap(0.5);
        assert_eq!(tap.tap(10.0), None);
        assert_eq!(tap.tap(11.0), Some(60.0));
    }

    /// Plays a clock whose `tap` input gets a short pulse at each of `taps`
    /// (in samples), the knob at 60 BPM, for `samples`; returns its beat.
    fn tapped(clock: &mut Clock, taps: &[usize], samples: usize) -> Vec<f32> {
        (0..samples / BLOCK)
            .flat_map(|b| {
                let pulse: Buffer = std::array::from_fn(|i| {
                    let n = b * BLOCK + i;
                    taps.iter().any(|&t| (t..t + 100).contains(&n)) as u8 as f32
                });
                run_signal(clock, &[pulse], &[60.0], OUT_BEAT, 1)
            })
            .collect()
    }

    #[test]
    fn taps_on_the_tap_input_set_the_tempo_and_the_beat() {
        let mut clock = Clock::new(SR, Arc::default());
        clock.set_plugged(IN_TAP, true);
        // Taps 0.4 s apart, 150 BPM, starting off the beat.
        let taps = [10_000, 29_200, 48_400, 67_600];
        let beat = tapped(&mut clock, &taps, 4 * 48_000);
        let edges = rising_edges(&beat);
        let after: Vec<_> = edges.iter().filter(|&&n| n >= taps[3]).collect();
        // On the last tap, then every 0.4 s.
        assert!(after[0].abs_diff(taps[3]) <= 1, "{edges:?}");
        for pair in after.windows(2) {
            assert!((pair[1] - pair[0]).abs_diff(19_200) <= 1, "{edges:?}");
        }
    }

    #[test]
    fn the_first_tap_starts_a_bar() {
        let mut clock = Clock::new(SR, Arc::default());
        clock.set_plugged(IN_TAP, true);
        // Late in a beat at 60 BPM, when the beat gate is low.
        tapped(&mut clock, &[30_000], 30_000 + BLOCK);
        assert!(clock.beats < 0.01, "{}", clock.beats);
    }

    #[test]
    fn without_a_cable_taps_are_ignored_and_unplugging_returns_to_the_knob() {
        let taps = [0, 19_200, 38_400];
        let beat_period = |clock: &mut Clock| {
            let beat = tapped(clock, &[], 3 * 48_000);
            let edges = rising_edges(&beat);
            edges[1] - edges[0]
        };
        let mut clock = Clock::new(SR, Arc::default());
        tapped(&mut clock, &taps, 48_000);
        assert!(beat_period(&mut clock).abs_diff(48_000) <= 1);

        clock.set_plugged(IN_TAP, true);
        tapped(&mut clock, &taps.map(|t| t + 48_000), 2 * 48_000);
        assert!(beat_period(&mut clock).abs_diff(19_200) <= 1);
        clock.set_plugged(IN_TAP, false);
        assert!(beat_period(&mut clock).abs_diff(48_000) <= 1);
    }
}
