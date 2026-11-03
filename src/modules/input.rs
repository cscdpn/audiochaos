use std::sync::Arc;

use crate::engine::input::{CHANNELS, LiveInput};
use crate::engine::module::{Buffer, Module, ModuleInfo};

pub static INFO: ModuleInfo = ModuleInfo {
    name: "input",
    description: "The sound card's inputs: a microphone, a guitar, another synth. Its light shows the level. Mind the feedback with a microphone near speakers.",
    inputs: &[],
    outputs: &["1", "2"],
    output_details: &["input 1, -1..1", "input 2, -1..1"],
    params: &[],
};

/// What comes in on the sound card's first two inputs, about two callbacks
/// late. Silent when rendering offline, or when no input opened.
pub struct Input {
    live: Arc<LiveInput>,
    peak: f32,
}

impl Input {
    pub fn new(live: Arc<LiveInput>) -> Self {
        Self { live, peak: 0.0 }
    }
}

impl Module for Input {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, _: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        self.peak = 0.0;
        for (channel, out) in outputs.iter_mut().enumerate().take(CHANNELS) {
            self.live.read(channel, out);
            self.peak = out.iter().fold(self.peak, |peak, x| peak.max(x.abs()));
        }
    }

    fn led(&self) -> Option<f32> {
        Some(self.peak.min(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::input::{Callbacks, queue};
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::run;

    #[test]
    fn is_silent_and_dark_without_a_sound_card() {
        let mut input = Input::new(Arc::default());
        assert!(run(&mut input, &[], &[], 0, 2).iter().all(|&x| x == 0.0));
        assert!(run(&mut input, &[], &[], 1, 2).iter().all(|&x| x == 0.0));
        assert_eq!(input.led(), Some(0.0));
    }

    #[test]
    fn plays_what_comes_in_and_lights_up_with_its_level() {
        let live = Arc::new(LiveInput::default());
        let (mut capture, mut feed) = queue(
            Arc::clone(&live),
            Arc::new(Callbacks::default()),
            Arc::default(),
        );
        let frames: Vec<f32> = (0..BLOCK).flat_map(|_| [0.25, -0.5]).collect();
        capture.push(&frames, 2);
        capture.push(&frames, 2);
        feed.next_block();
        let mut input = Input::new(live);
        assert!(run(&mut input, &[], &[], 0, 1).iter().all(|&x| x == 0.25));
        assert!(run(&mut input, &[], &[], 1, 1).iter().all(|&x| x == -0.5));
        assert_eq!(input.led(), Some(0.5));
    }
}
