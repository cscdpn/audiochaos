//! Sound coming in from the sound card: a microphone or an instrument.
//!
//! The input and output streams have callbacks of their own, which neither
//! run in step nor ask for the same number of frames. Frames go from one to
//! the other through a lock-free queue, and the engine takes one block from
//! it before each block it computes. The queue keeps a backlog of about one
//! callback of each stream, measured as they run: less and it would run dry
//! between two input callbacks, more is latency for nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use super::module::{BLOCK, Buffer};

/// Channels the engine takes from the input: a two-input interface gives a
/// microphone and an instrument. Further channels are ignored.
pub const CHANNELS: usize = 2;

type Frame = [f32; CHANNELS];

/// Room for far more than any callback asks for, even at 192 kHz.
const QUEUE_FRAMES: usize = 1 << 15;

/// The block of input the graph is computing, for `audio in` modules to
/// read. Written and read on the audio thread only, between blocks; atomics
/// so the modules can share it without a lock.
pub struct LiveInput {
    channels: [[AtomicU32; BLOCK]; CHANNELS],
}

impl Default for LiveInput {
    fn default() -> Self {
        Self {
            channels: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
        }
    }
}

impl LiveInput {
    pub fn read(&self, channel: usize, out: &mut Buffer) {
        for (o, x) in out.iter_mut().zip(&self.channels[channel]) {
            *o = f32::from_bits(x.load(Ordering::Relaxed));
        }
    }

    /// Sets one frame of every channel, for a host handing frames over one
    /// at a time.
    pub fn write_frame(&self, frame: usize, values: [f32; CHANNELS]) {
        for (channel, value) in values.into_iter().enumerate() {
            self.write(channel, frame, value);
        }
    }

    fn write(&self, channel: usize, frame: usize, value: f32) {
        self.channels[channel][frame].store(value.to_bits(), Ordering::Relaxed);
    }
}

/// How many frames each stream's callback handles at most, as seen so far.
#[derive(Default)]
pub struct Callbacks {
    input: AtomicUsize,
    output: AtomicUsize,
}

impl Callbacks {
    pub fn output_ran(&self, frames: usize) {
        self.output.fetch_max(frames, Ordering::Relaxed);
    }

    fn backlog(&self) -> usize {
        self.input.load(Ordering::Relaxed) + self.output.load(Ordering::Relaxed)
    }
}

/// The input stream's end of the queue.
pub struct Capture {
    frames: Producer<Frame>,
    callbacks: Arc<Callbacks>,
    glitches: Arc<AtomicUsize>,
}

impl Capture {
    /// Queues what one input callback got, `channels` interleaved samples
    /// per frame.
    pub fn push(&mut self, data: &[f32], channels: usize) {
        self.callbacks
            .input
            .fetch_max(data.len() / channels, Ordering::Relaxed);
        for samples in data.chunks_exact(channels) {
            let frame = std::array::from_fn(|c| samples.get(c).copied().unwrap_or(0.0));
            if self.frames.push(frame).is_err() {
                self.glitches.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
}

/// The engine's end of the queue: fills `LiveInput` one block at a time.
pub struct Feed {
    frames: Consumer<Frame>,
    live: Arc<LiveInput>,
    callbacks: Arc<Callbacks>,
    glitches: Arc<AtomicUsize>,
    /// Whether the backlog has built up: until then, and after running dry,
    /// the input is silent rather than broken up.
    primed: bool,
}

impl Feed {
    pub fn next_block(&mut self) {
        let backlog = self.callbacks.backlog();
        let available = self.frames.slots();
        if !self.primed {
            if available < backlog + BLOCK {
                return self.silence();
            }
            self.primed = true;
        }
        if available < BLOCK {
            self.glitches.fetch_add(1, Ordering::Relaxed);
            self.primed = false;
            return self.silence();
        }
        // The two streams' clocks drift apart when they are different
        // devices. Rather than letting the latency grow, catch up at once.
        if available > 2 * backlog + BLOCK {
            let late = available - backlog - BLOCK;
            if let Ok(chunk) = self.frames.read_chunk(late) {
                chunk.commit_all();
            }
            self.glitches.fetch_add(1, Ordering::Relaxed);
        }
        for i in 0..BLOCK {
            let frame = self.frames.pop().unwrap_or_default();
            for (c, &value) in frame.iter().enumerate() {
                self.live.write(c, i, value);
            }
        }
    }

    fn silence(&self) {
        for c in 0..CHANNELS {
            for i in 0..BLOCK {
                self.live.write(c, i, 0.0);
            }
        }
    }
}

/// A queue from the input stream to the engine, which shows its frames in
/// `live`. Overflows, underruns and catch-ups are counted in `glitches`.
pub fn queue(
    live: Arc<LiveInput>,
    callbacks: Arc<Callbacks>,
    glitches: Arc<AtomicUsize>,
) -> (Capture, Feed) {
    let (producer, consumer) = RingBuffer::new(QUEUE_FRAMES);
    let capture = Capture {
        frames: producer,
        callbacks: Arc::clone(&callbacks),
        glitches: Arc::clone(&glitches),
    };
    let feed = Feed {
        frames: consumer,
        live,
        callbacks,
        glitches,
        primed: false,
    };
    (capture, feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        live: Arc<LiveInput>,
        callbacks: Arc<Callbacks>,
        glitches: Arc<AtomicUsize>,
        capture: Capture,
        feed: Feed,
    }

    fn fixture() -> Fixture {
        let live = Arc::new(LiveInput::default());
        let callbacks = Arc::new(Callbacks::default());
        let glitches = Arc::new(AtomicUsize::new(0));
        let (capture, feed) = queue(
            Arc::clone(&live),
            Arc::clone(&callbacks),
            Arc::clone(&glitches),
        );
        Fixture {
            live,
            callbacks,
            glitches,
            capture,
            feed,
        }
    }

    impl Fixture {
        /// Stereo frames counting up from `first`, channel 2 negated.
        fn push(&mut self, first: usize, frames: usize) {
            let data: Vec<f32> = (first..first + frames)
                .flat_map(|i| [i as f32, -(i as f32)])
                .collect();
            self.capture.push(&data, 2);
        }

        fn block(&mut self) -> (Buffer, Buffer) {
            self.feed.next_block();
            let mut one = [f32::NAN; BLOCK];
            let mut two = [f32::NAN; BLOCK];
            self.live.read(0, &mut one);
            self.live.read(1, &mut two);
            (one, two)
        }

        fn glitches(&self) -> usize {
            self.glitches.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn waits_for_a_callback_of_each_stream_then_plays_in_order() {
        let mut f = fixture();
        f.callbacks.output_ran(256);
        f.push(0, 256);
        f.push(256, 256);
        assert_eq!(
            f.block().0,
            [0.0; BLOCK],
            "silent until the backlog is there"
        );
        f.push(512, BLOCK);
        let (one, two) = f.block();
        assert_eq!(one[0], 0.0);
        assert_eq!(one[BLOCK - 1], (BLOCK - 1) as f32);
        assert_eq!(two[1], -1.0);
        assert_eq!(f.block().0[0], BLOCK as f32, "the next block follows");
        assert_eq!(f.glitches(), 0);
    }

    #[test]
    fn running_dry_goes_silent_until_the_backlog_is_back() {
        let mut f = fixture();
        f.push(0, BLOCK);
        f.push(BLOCK, BLOCK);
        f.block();
        f.block();
        assert_eq!(f.block().0, [0.0; BLOCK]);
        assert_eq!(f.glitches(), 1);
        // The input's callbacks are a block long: two to wait for.
        f.push(2 * BLOCK, BLOCK);
        assert_eq!(f.block().0, [0.0; BLOCK]);
        f.push(3 * BLOCK, BLOCK);
        assert_eq!(f.block().0[0], (2 * BLOCK) as f32);
    }

    #[test]
    fn catches_up_when_too_far_behind() {
        let mut f = fixture();
        f.push(0, BLOCK);
        f.push(BLOCK, BLOCK);
        f.block();
        // The input's clock ran fast: ten more blocks came in meanwhile.
        for b in 2..12 {
            f.push(b * BLOCK, BLOCK);
        }
        let (one, _) = f.block();
        // Back to a backlog of one input callback, a block here.
        assert_eq!(one[0], (10 * BLOCK) as f32, "{}", one[0]);
        assert_eq!(f.glitches(), 1);
    }

    #[test]
    fn a_mono_input_leaves_channel_two_silent() {
        let mut f = fixture();
        f.capture.push(&[0.5; BLOCK], 1);
        f.capture.push(&[0.5; BLOCK], 1);
        let (one, two) = f.block();
        assert_eq!(one, [0.5; BLOCK]);
        assert_eq!(two, [0.0; BLOCK]);
    }

    #[test]
    fn channels_past_the_second_are_ignored() {
        let mut f = fixture();
        let data: Vec<f32> = (0..BLOCK).flat_map(|_| [1.0, 2.0, 3.0, 4.0]).collect();
        f.capture.push(&data, 4);
        f.capture.push(&data, 4);
        let (one, two) = f.block();
        assert_eq!((one[0], two[0]), (1.0, 2.0));
    }
}
