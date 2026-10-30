//! Recording to WAV files: offline renders, and live recording of what the
//! speakers play.
//!
//! The audio thread must never touch the disk, so live recording goes through
//! a lock-free queue to a writer thread, the same way commands go the other
//! way.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use rtrb::{Consumer, Producer, RingBuffer};

use super::graph::{Graph, NodeId};

/// How often the writer thread empties the queue. The queue holds a second of
/// audio, so this leaves plenty of slack.
const POLL: Duration = Duration::from_millis(20);

type Writer = WavWriter<BufWriter<File>>;

/// Mono 16-bit: plays anywhere, and the master never leaves -1..1, so nothing
/// is lost but noise far below what the synth makes.
fn spec(sample_rate: u32) -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

/// Refuses to overwrite an existing file.
fn create(path: &Path, sample_rate: u32) -> hound::Result<Writer> {
    let file = File::create_new(path)?;
    WavWriter::new(BufWriter::new(file), spec(sample_rate))
}

fn to_i16(sample: f32) -> i16 {
    // `as` saturates out-of-range values and turns NaN into 0.
    (sample * f32::from(i16::MAX)).round() as i16
}

/// Renders `seconds` of `node`'s first output to a new WAV file, as fast as
/// the CPU allows.
pub fn render(graph: &mut Graph, node: NodeId, seconds: f32, path: &Path) -> hound::Result<()> {
    let sample_rate = graph.sample_rate() as u32;
    let mut wav = create(path, sample_rate)?;
    let mut remaining = (seconds * sample_rate as f32).round() as usize;
    while remaining > 0 {
        graph.process();
        let block = graph.output(node, 0);
        let n = remaining.min(block.len());
        for &sample in &block[..n] {
            wav.write_sample(to_i16(sample))?;
        }
        remaining -= n;
    }
    wav.finalize()
}

/// The audio thread's end: copies samples into the queue while a recording
/// runs. Never blocks nor allocates.
pub struct Tap {
    samples: Producer<f32>,
    armed: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
}

impl Tap {
    pub fn push(&mut self, sample: f32) {
        if self.armed.load(Ordering::Relaxed) && self.samples.push(sample).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The UI's end: starts and stops recordings.
pub struct Recorder {
    sample_rate: u32,
    armed: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
    /// Exactly one of these is set: the queue is either idle here or owned by
    /// the writer thread of the running session.
    idle: Option<Consumer<f32>>,
    session: Option<Session>,
}

struct Session {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    written: Arc<AtomicUsize>,
    thread: JoinHandle<(Consumer<f32>, hound::Result<()>)>,
}

/// A finished recording.
#[derive(Debug)]
pub struct Recording {
    pub path: PathBuf,
    pub duration: Duration,
    /// Samples the writer could not keep up with; 0 unless the disk stalled.
    pub dropped: usize,
}

/// The two ends of a live recording queue holding a second of audio,
/// allocated here, once, rather than on the audio thread.
pub fn tap(sample_rate: u32) -> (Tap, Recorder) {
    let (producer, consumer) = RingBuffer::new(sample_rate as usize);
    let armed = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicUsize::new(0));
    let tap = Tap {
        samples: producer,
        armed: Arc::clone(&armed),
        dropped: Arc::clone(&dropped),
    };
    let recorder = Recorder {
        sample_rate,
        armed,
        dropped,
        idle: Some(consumer),
        session: None,
    };
    (tap, recorder)
}

impl Recorder {
    /// How long the running recording has lasted, if one runs.
    pub fn elapsed(&self) -> Option<Duration> {
        let session = self.session.as_ref()?;
        let written = session.written.load(Ordering::Relaxed);
        Some(Duration::from_secs_f64(
            written as f64 / f64::from(self.sample_rate),
        ))
    }

    /// Starts recording into a new file at `path`. Does nothing if a recording
    /// already runs.
    pub fn start(&mut self, path: PathBuf) -> hound::Result<()> {
        if self.session.is_some() {
            return Ok(());
        }
        let wav = create(&path, self.sample_rate)?;
        let mut samples = self.idle.take().expect("idle when not recording");
        // Leftovers from the end of the previous recording.
        while samples.pop().is_ok() {}
        self.dropped.store(0, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicUsize::new(0));
        let thread = {
            let (stop, written) = (Arc::clone(&stop), Arc::clone(&written));
            thread::spawn(move || write(samples, wav, &stop, &written))
        };
        self.session = Some(Session {
            path,
            stop,
            written,
            thread,
        });
        self.armed.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Stops the running recording and finishes its file. `None` if nothing
    /// was recording.
    pub fn stop(&mut self) -> Option<hound::Result<Recording>> {
        let session = self.session.take()?;
        self.armed.store(false, Ordering::Relaxed);
        session.stop.store(true, Ordering::Relaxed);
        let (samples, result) = session.thread.join().expect("the writer thread panicked");
        self.idle = Some(samples);
        let written = session.written.load(Ordering::Relaxed);
        Some(result.map(|()| Recording {
            path: session.path,
            duration: Duration::from_secs_f64(written as f64 / f64::from(self.sample_rate)),
            dropped: self.dropped.load(Ordering::Relaxed),
        }))
    }

    /// Starts recording into the first free `recording-NNN.wav` in `dir`, or
    /// stops the running recording. Returns a status message for the UI.
    pub fn toggle(&mut self, dir: &Path) -> String {
        match self.stop() {
            Some(Ok(recording)) => {
                let mut message = format!(
                    "saved {} ({:.1} s)",
                    recording.path.display(),
                    recording.duration.as_secs_f32()
                );
                if recording.dropped > 0 {
                    message += &format!(", {} samples lost", recording.dropped);
                }
                message
            }
            Some(Err(e)) => format!("recording failed: {e}"),
            None => {
                let path = free_path(dir);
                match self.start(path.clone()) {
                    Ok(()) => format!("recording to {}", path.display()),
                    Err(e) => format!("cannot record to {}: {e}", path.display()),
                }
            }
        }
    }
}

/// Closing the app mid-recording still leaves a complete file.
impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// A recording's length as `m:ss`, for display.
pub fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn free_path(dir: &Path) -> PathBuf {
    (1..)
        .map(|n| dir.join(format!("recording-{n:03}.wav")))
        .find(|path| !path.exists())
        .expect("some number is free")
}

/// The writer thread: drains the queue into the file until told to stop, then
/// hands the queue back.
fn write(
    mut samples: Consumer<f32>,
    mut wav: Writer,
    stop: &AtomicBool,
    written: &AtomicUsize,
) -> (Consumer<f32>, hound::Result<()>) {
    let mut drain = || -> hound::Result<()> {
        loop {
            // Read the flag first, so the last pass empties everything pushed
            // before the tap was disarmed.
            let last_pass = stop.load(Ordering::Relaxed);
            while let Ok(sample) = samples.pop() {
                wav.write_sample(to_i16(sample))?;
                written.fetch_add(1, Ordering::Relaxed);
            }
            if last_pass {
                return Ok(());
            }
            thread::sleep(POLL);
        }
    };
    let result = drain().and_then(|()| wav.finalize());
    (samples, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::oscillator::Oscillator;

    const SR: u32 = 48_000;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("audiochaos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: &Path) -> (WavSpec, Vec<i16>) {
        let mut reader = hound::WavReader::open(path).unwrap();
        let samples = reader.samples::<i16>().map(Result::unwrap).collect();
        (reader.spec(), samples)
    }

    #[test]
    fn samples_map_to_the_full_16_bit_range() {
        assert_eq!(to_i16(0.0), 0);
        assert_eq!(to_i16(1.0), i16::MAX);
        assert_eq!(to_i16(-1.0), -i16::MAX);
        assert_eq!(to_i16(3.0), i16::MAX);
        assert_eq!(to_i16(f32::NAN), 0);
    }

    #[test]
    fn clock_shows_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs_f32(5.9)), "0:05");
        assert_eq!(clock(Duration::from_secs(125)), "2:05");
    }

    #[test]
    fn render_writes_the_requested_length() {
        let dir = temp_dir("render");
        let path = dir.join("out.wav");
        let mut graph = Graph::new(SR as f32);
        let osc = graph.add("osc", Box::new(Oscillator::new(SR as f32)));
        render(&mut graph, osc, 0.5, &path).unwrap();

        let (spec, samples) = read(&path);
        assert_eq!(spec.sample_rate, SR);
        assert_eq!(spec.channels, 1);
        assert_eq!(samples.len(), SR as usize / 2);
        // The default sine, 110 Hz, at full scale.
        assert!(samples.iter().any(|&s| s > 30_000));

        assert!(
            render(&mut graph, osc, 0.5, &path).is_err(),
            "must not overwrite"
        );
    }

    #[test]
    fn records_what_the_tap_sees_between_start_and_stop() {
        let dir = temp_dir("live");
        let (mut tap, mut recorder) = tap(SR);
        tap.push(0.9); // not recording yet: ignored
        assert!(recorder.elapsed().is_none());

        let path = dir.join("take.wav");
        recorder.start(path.clone()).unwrap();
        let ramp: Vec<f32> = (0..SR).map(|i| i as f32 / SR as f32).collect();
        for &s in &ramp {
            tap.push(s);
        }
        let recording = recorder.stop().unwrap().unwrap();
        tap.push(0.9); // stopped: ignored

        assert_eq!(recording.path, path);
        assert_eq!(recording.duration, Duration::from_secs(1));
        assert_eq!(recording.dropped, 0);
        let (_, samples) = read(&path);
        let expected: Vec<i16> = ramp.iter().map(|&s| to_i16(s)).collect();
        assert_eq!(samples, expected);
        assert!(recorder.stop().is_none());
    }

    #[test]
    fn a_full_queue_counts_lost_samples_instead_of_blocking() {
        // Armed with no writer draining the queue, as if the disk stalled.
        let (mut tap, recorder) = tap(100);
        recorder.armed.store(true, Ordering::Relaxed);
        for _ in 0..1_000 {
            tap.push(0.5);
        }
        assert_eq!(recorder.dropped.load(Ordering::Relaxed), 900);
    }

    #[test]
    fn toggle_numbers_recordings_and_reports_them() {
        let dir = temp_dir("toggle");
        let (mut tap, mut recorder) = tap(SR);
        for expected in ["recording-001.wav", "recording-002.wav"] {
            let started = recorder.toggle(&dir);
            assert!(started.starts_with("recording to"), "{started}");
            assert!(started.ends_with(expected), "{started}");
            tap.push(0.1);
            let saved = recorder.toggle(&dir);
            assert!(saved.starts_with("saved"), "{saved}");
            assert!(dir.join(expected).exists());
        }
    }

    #[test]
    fn dropping_the_recorder_finishes_the_file() {
        let dir = temp_dir("drop");
        let path = dir.join("take.wav");
        let (mut tap, mut recorder) = tap(SR);
        recorder.start(path.clone()).unwrap();
        tap.push(0.5);
        drop(recorder);
        let (_, samples) = read(&path);
        assert_eq!(samples, [to_i16(0.5)]);
    }

    #[test]
    fn a_failed_start_keeps_the_recorder_usable() {
        let dir = temp_dir("fail");
        let path = dir.join("taken.wav");
        std::fs::write(&path, "").unwrap();
        let (_tap, mut recorder) = tap(SR);
        assert!(recorder.start(path).is_err());
        assert!(recorder.elapsed().is_none());
        recorder.start(dir.join("free.wav")).unwrap();
        assert!(recorder.stop().unwrap().is_ok());
    }
}
