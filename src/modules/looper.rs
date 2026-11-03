use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::modules::dsp::flash;
use crate::modules::sampler::{Playhead, playback};

pub const IN: usize = 0;
pub const IN_RECORD: usize = 1;
pub const IN_TRIGGER: usize = 2;

pub const OUT: usize = 0;

/// The longest recording: 30 s of mono takes 6 MB at 48 kHz, allocated when
/// the module is built since the audio thread cannot.
pub const MAX_SECONDS: f32 = 30.0;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "looper",
    description: "Records what comes in while `record` is held, then plays it like a sampler: on a loop, or a slice on each trigger. Hold a key or a pad to record.",
    inputs: &["in", "record", "trigger"],
    outputs: &["out"],
    output_details: &["the recording"],
    params: &playback(1.0),
};

/// A live sampler. A rising `record` gate starts a new recording over the
/// last one, for as long as the gate stays open, up to `MAX_SECONDS`; then
/// it plays like `sampler`, and loops at once unless `loop` is turned down.
/// It stays silent while recording: patch the input around it to hear
/// yourself. Recordings are not saved with the patch.
pub struct Looper {
    buffer: Vec<f32>,
    /// How much of `buffer` the last recording filled.
    recorded: usize,
    recording: bool,
    last_record: f32,
    playhead: Playhead,
}

impl Looper {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buffer: vec![0.0; (MAX_SECONDS * sample_rate) as usize],
            recorded: 0,
            recording: false,
            last_record: 0.0,
            playhead: Playhead::default(),
        }
    }

    fn record(&mut self, x: f32) {
        if self.recorded < self.buffer.len() {
            self.buffer[self.recorded] = x;
            self.recorded += 1;
        } else {
            self.recording = false;
        }
    }
}

impl Module for Looper {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        // Recording starts and stops on the first sample of a block that
        // changes the gate: a block is 1.3 ms, far below what a hand feels.
        let gate = inputs[IN_RECORD][0];
        if self.last_record <= 0.0 && gate > 0.0 {
            self.recording = true;
            self.recorded = 0;
            self.playhead.stop();
        } else if gate <= 0.0 {
            self.recording = false;
        }
        self.last_record = gate;
        if self.recording {
            for &x in &inputs[IN] {
                self.record(x);
            }
            outputs[OUT].fill(0.0);
            return;
        }
        let frames = &self.buffer[..self.recorded];
        self.playhead
            .play(frames, 1.0, &inputs[IN_TRIGGER], params, &mut outputs[OUT]);
    }

    /// Full while recording, then a flash each time the slice starts over.
    fn led(&self) -> Option<f32> {
        if self.recording {
            return Some(1.0);
        }
        Some(self.playhead.progress().map_or(0.0, flash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::sampler::{LENGTH, LOOP, SPEED, START};
    use crate::modules::test_util::run_signal;

    const SR: f32 = 1_000.0;
    const HELD: Buffer = [1.0; BLOCK];
    const OFF: Buffer = [0.0; BLOCK];
    const LOOPING: [f32; 4] = [1.0, 0.0, 1.0, 1.0];

    fn counting(first: usize) -> Buffer {
        std::array::from_fn(|i| (first + i) as f32)
    }

    fn block(looper: &mut Looper, input: Buffer, record: Buffer, params: [f32; 4]) -> Buffer {
        let out = run_signal(looper, &[input, record, OFF], &params, OUT, 1);
        out.try_into().unwrap()
    }

    #[test]
    fn plays_back_what_was_recorded_on_a_loop() {
        let mut looper = Looper::new(SR);
        assert_eq!(block(&mut looper, counting(0), HELD, LOOPING), OFF);
        assert_eq!(block(&mut looper, counting(BLOCK), HELD, LOOPING), OFF);
        assert_eq!(looper.led(), Some(1.0));
        let first = block(&mut looper, counting(999), OFF, LOOPING);
        assert_eq!(first, counting(0), "the first block recorded");
        assert_eq!(block(&mut looper, OFF, OFF, LOOPING), counting(BLOCK));
        assert_eq!(block(&mut looper, OFF, OFF, LOOPING), counting(0), "again");
    }

    #[test]
    fn a_new_recording_replaces_the_last() {
        let mut looper = Looper::new(SR);
        block(&mut looper, [5.0; BLOCK], HELD, LOOPING);
        block(&mut looper, OFF, OFF, LOOPING);
        block(&mut looper, [7.0; BLOCK], HELD, LOOPING);
        assert_eq!(block(&mut looper, OFF, OFF, LOOPING), [7.0; BLOCK]);
    }

    #[test]
    fn without_loop_it_waits_for_a_trigger() {
        let mut looper = Looper::new(SR);
        let once = [1.0, 0.0, 1.0, 0.0];
        block(&mut looper, [0.5; BLOCK], HELD, once);
        assert_eq!(block(&mut looper, OFF, OFF, once), OFF);
        let out = run_signal(&mut looper, &[OFF, OFF, HELD], &once, OUT, 2);
        assert!(out[..BLOCK].iter().all(|&x| x == 0.5));
        assert!(out[BLOCK..].iter().all(|&x| x == 0.0), "played once");
    }

    #[test]
    fn slices_and_speed_work_on_the_recording() {
        let mut looper = Looper::new(SR);
        block(&mut looper, counting(0), HELD, LOOPING);
        let mut params = LOOPING;
        params[SPEED] = 2.0;
        params[START] = 0.5;
        params[LENGTH] = 0.25;
        assert_eq!(params[LOOP], 1.0);
        let out = block(&mut looper, OFF, OFF, params);
        assert_eq!(&out[..4], &[32.0, 34.0, 36.0, 38.0]);
        assert_eq!(out[8], 32.0, "16 frames long, two at a time");
    }

    #[test]
    fn recording_stops_when_the_buffer_is_full() {
        let mut looper = Looper::new(SR);
        let blocks = (MAX_SECONDS * SR) as usize / BLOCK + 2;
        for _ in 0..blocks {
            block(&mut looper, [0.25; BLOCK], HELD, LOOPING);
        }
        assert_eq!(looper.recorded, looper.buffer.len());
        // Still held, but it plays now.
        assert_eq!(block(&mut looper, OFF, HELD, LOOPING), [0.25; BLOCK]);
    }

    #[test]
    fn the_light_flashes_at_each_turn_of_the_loop() {
        let mut looper = Looper::new(SR);
        assert_eq!(looper.led(), Some(0.0));
        block(&mut looper, counting(0), HELD, LOOPING);
        block(&mut looper, counting(0), HELD, LOOPING);
        block(&mut looper, OFF, OFF, LOOPING);
        let late = looper.led().unwrap();
        assert!(late < 0.2, "{late}");
        block(&mut looper, OFF, OFF, LOOPING);
        assert!(looper.led().unwrap() < late + 1e-6);
    }
}
