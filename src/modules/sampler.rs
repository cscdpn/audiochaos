use crate::engine::module::{BLOCK, Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};
use crate::engine::sample::Sample;

pub const IN_TRIGGER: usize = 0;

pub const OUT: usize = 0;

pub const SPEED: usize = 0;
pub const START: usize = 1;
pub const LENGTH: usize = 2;
pub const LOOP: usize = 3;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "sampler",
    inputs: &["trigger"],
    outputs: &["out"],
    output_details: &["the sound"],
    params: &[
        // Spans as many octaves as an oscillator's `freq`, so a MIDI module's
        // `pitch` plays it in tune too.
        ParamInfo {
            name: "speed",
            min: 0.01,
            max: 100.0,
            default: 1.0,
            scale: Scale::Exponential,
            unit: "x",
        },
        ParamInfo {
            name: "start",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            scale: Scale::Linear,
            unit: "",
        },
        ParamInfo {
            name: "length",
            min: 0.001,
            max: 1.0,
            default: 1.0,
            scale: Scale::Exponential,
            unit: "",
        },
        ParamInfo {
            name: "loop",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            scale: Scale::Linear,
            unit: "",
        },
    ],
};

/// Plays a sound file from `start` for `length`, both as fractions of the
/// file, restarting on each rising edge of `trigger`. Past halfway, `loop`
/// repeats that slice for as long as it stays there, trigger or not: short
/// slices with a modulated `start` stutter and glitch.
pub struct Sampler {
    sample_rate: f32,
    sample: Option<Box<Sample>>,
    position: f64,
    playing: bool,
    last_trigger: f32,
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            sample: None,
            position: 0.0,
            playing: false,
            last_trigger: 0.0,
        }
    }
}

/// Between two frames, as the playback rate rarely lands on one.
fn read(frames: &[f32], position: f64) -> f32 {
    let index = position as usize;
    let fraction = (position - index as f64) as f32;
    let current = frames.get(index).copied().unwrap_or(0.0);
    let next = frames.get(index + 1).copied().unwrap_or(0.0);
    current + (next - current) * fraction
}

impl Module for Sampler {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        let Some(sample) = self.sample.as_ref().filter(|s| !s.frames.is_empty()) else {
            self.last_trigger = inputs[IN_TRIGGER][BLOCK - 1];
            outputs[OUT].fill(0.0);
            return;
        };
        let total = sample.frames.len() as f64;
        let rate = f64::from(sample.sample_rate) / f64::from(self.sample_rate);
        for (i, out) in outputs[OUT].iter_mut().enumerate() {
            let trigger = inputs[IN_TRIGGER][i];
            let start = f64::from(params[START][i]) * total;
            let end = (start + f64::from(params[LENGTH][i]) * total).min(total);
            let looping = params[LOOP][i] > 0.5;

            let edge = self.last_trigger <= 0.0 && trigger > 0.0;
            self.last_trigger = trigger;
            if edge || (looping && !self.playing) {
                self.position = start;
                self.playing = true;
            }

            if !self.playing {
                *out = 0.0;
                continue;
            }
            *out = read(&sample.frames, self.position);
            self.position += f64::from(params[SPEED][i]) * rate;
            // `start` and `length` may have moved under the playhead.
            if self.position >= end || self.position >= total {
                self.position = start;
                self.playing = looping;
            }
        }
    }

    fn set_sample(&mut self, sample: Box<Sample>) -> Option<Box<Sample>> {
        self.playing = false;
        self.sample.replace(sample)
    }

    fn sample(&self) -> Option<&Sample> {
        self.sample.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::graph::{Cable, Graph};
    use crate::midi::{Parser, State};
    use crate::modules::midi::{self, Midi};
    use crate::modules::test_util::run_signal;
    use std::sync::Arc;

    const SR: f32 = 48_000.0;

    fn sampler(frames: Vec<f32>, sample_rate: u32) -> Sampler {
        let mut sampler = Sampler::new(SR);
        sampler.set_sample(Box::new(Sample {
            path: "test.wav".to_string(),
            frames,
            sample_rate,
            problem: None,
        }));
        sampler
    }

    /// A ramp from 0 towards 1, so the output shows where the playhead is.
    fn ramp(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32 / frames as f32).collect()
    }

    const FIRE: Buffer = [1.0; BLOCK];
    const REST: Buffer = [0.0; BLOCK];

    fn play(s: &mut Sampler, trigger: Buffer, params: [f32; 4], blocks: usize) -> Vec<f32> {
        run_signal(s, &[trigger], &params, OUT, blocks)
    }

    #[test]
    fn is_silent_until_triggered_then_plays_once() {
        let mut s = sampler(vec![0.5; 100], 48_000);
        let params = [1.0, 0.0, 1.0, 0.0];
        assert!(play(&mut s, REST, params, 1).iter().all(|&x| x == 0.0));
        let out = play(&mut s, FIRE, params, 2);
        assert!(out[..100].iter().all(|&x| x == 0.5));
        assert!(out[100..].iter().all(|&x| x == 0.0), "stops at the end");
    }

    #[test]
    fn plays_at_the_right_rate() {
        let frames = ramp(1000);
        // Speed 2 at the engine's rate: two frames per sample.
        let out = play(
            &mut sampler(frames.clone(), 48_000),
            FIRE,
            [2.0, 0.0, 1.0, 0.0],
            1,
        );
        assert!((out[10] - frames[20]).abs() < 1e-5);
        // A file at half the engine's rate plays half as many frames per sample.
        let out = play(
            &mut sampler(frames.clone(), 24_000),
            FIRE,
            [1.0, 0.0, 1.0, 0.0],
            1,
        );
        assert!((out[10] - frames[5]).abs() < 1e-5);
        // Between two frames, it interpolates.
        assert!((out[11] - (frames[5] + frames[6]) / 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_rising_edge_restarts_it() {
        let mut s = sampler(ramp(10_000), 48_000);
        let params = [1.0, 0.0, 1.0, 0.0];
        play(&mut s, FIRE, params, 3);
        let trigger = std::array::from_fn(|i| if i < 10 { -1.0 } else { 1.0 });
        let out = play(&mut s, trigger, params, 1);
        assert_eq!(out[10], 0.0, "back at the first frame");
        assert!(out[9] > 0.01);
    }

    #[test]
    fn start_and_length_pick_a_slice() {
        let frames = ramp(1000);
        let mut s = sampler(frames.clone(), 48_000);
        let out = play(&mut s, FIRE, [1.0, 0.5, 0.01, 0.0], 1);
        assert!((out[0] - frames[500]).abs() < 1e-5);
        assert!((out[9] - frames[509]).abs() < 1e-5);
        assert_eq!(out[10], 0.0, "ten frames long");
    }

    #[test]
    fn looping_repeats_the_slice_without_a_trigger() {
        let frames = ramp(1000);
        let mut s = sampler(frames.clone(), 48_000);
        let out = play(&mut s, REST, [1.0, 0.5, 0.01, 1.0], 1);
        assert!((out[0] - frames[500]).abs() < 1e-5);
        assert!((out[10] - frames[500]).abs() < 1e-5);
        assert!((out[25] - frames[505]).abs() < 1e-5);
    }

    #[test]
    fn a_slice_past_the_end_stays_inside_the_file() {
        let frames = ramp(100);
        let mut s = sampler(frames, 48_000);
        let out = play(&mut s, FIRE, [100.0, 1.0, 1.0, 1.0], 4);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }

    /// How many frames the playhead moves per sample, with a MIDI keyboard
    /// playing `note` into `speed cv`.
    fn frames_per_sample_at(note: u8) -> f32 {
        let state = Arc::new(State::default());
        Parser::default().handle(&[0x90, note, 100], &state);
        let mut g = Graph::new(SR);
        let keys = g.add("midi", Box::new(Midi::new(state, SR)));
        let frames = 1_000_000;
        let s = g.add("sampler", Box::new(sampler(ramp(frames), 48_000)));
        g.set_param(s, LOOP, 1.0);
        g.connect(Cable {
            from: keys,
            from_port: midi::OUT_PITCH,
            to: s,
            to_port: INFO.cv_port(SPEED),
        })
        .unwrap();
        let out = g.render(s, OUT, 4800);
        (out[4799] - out[4798]) * frames as f32
    }

    #[test]
    fn a_midi_keyboard_plays_it_in_tune() {
        assert!((frames_per_sample_at(60) - 1.0).abs() < 0.05);
        assert!(
            (frames_per_sample_at(72) - 2.0).abs() < 0.05,
            "an octave up"
        );
        assert!(
            (frames_per_sample_at(48) - 0.5).abs() < 0.05,
            "an octave down"
        );
    }

    #[test]
    fn without_a_sound_it_is_silent() {
        let mut s = Sampler::new(SR);
        assert!(
            play(&mut s, FIRE, [1.0, 0.0, 1.0, 1.0], 2)
                .iter()
                .all(|&x| x == 0.0)
        );
        let mut s = sampler(Vec::new(), 1);
        assert!(
            play(&mut s, FIRE, [1.0, 0.0, 1.0, 1.0], 2)
                .iter()
                .all(|&x| x == 0.0)
        );
    }

    #[test]
    fn replacing_the_sound_hands_back_the_old_one() {
        let mut s = Sampler::new(SR);
        let new = |path: &str| {
            Box::new(Sample {
                path: path.to_string(),
                frames: vec![0.5],
                sample_rate: 48_000,
                problem: None,
            })
        };
        assert!(s.set_sample(new("a")).is_none());
        assert_eq!(s.set_sample(new("b")).unwrap().path, "a");
        assert_eq!(s.sample().unwrap().path, "b");
    }
}
