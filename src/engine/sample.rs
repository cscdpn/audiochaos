//! Audio files loaded into memory, for samplers.

use hound::{SampleFormat, WavReader};

/// A sound in memory, mono. Read on the UI thread and moved to the audio thread
/// whole.
pub struct Sample {
    /// Where it came from, as given, so a saved patch can refer to it.
    pub path: String,
    pub frames: Vec<f32>,
    pub sample_rate: u32,
    /// Why the file could not be read, in which case `frames` is empty. Kept
    /// rather than treated as fatal: one lost file should not stop a patch
    /// from loading, nor saving the patch again lose its path.
    pub problem: Option<String>,
}

impl Sample {
    /// Reads a WAV file of any common format, mixing its channels down.
    pub fn load(path: &str) -> Result<Sample, String> {
        let reader = WavReader::open(path).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        let interleaved = match spec.sample_format {
            SampleFormat::Float => reader.into_samples::<f32>().collect::<Result<Vec<_>, _>>(),
            SampleFormat::Int => {
                let full_scale = 2.0_f32.powi(i32::from(spec.bits_per_sample) - 1);
                reader
                    .into_samples::<i32>()
                    .map(|s| s.map(|s| s as f32 / full_scale))
                    .collect()
            }
        }
        .map_err(|e| e.to_string())?;
        if interleaved.is_empty() {
            return Err("the file holds no audio".to_string());
        }
        let channels = usize::from(spec.channels);
        Ok(Sample {
            path: path.to_string(),
            frames: interleaved
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect(),
            sample_rate: spec.sample_rate,
            problem: None,
        })
    }

    /// `load`, except that a file that cannot be read gives a silent sample
    /// remembering its path and why.
    pub fn load_or_silence(path: &str) -> Sample {
        Sample::load(path).unwrap_or_else(|problem| Sample {
            path: path.to_string(),
            frames: Vec::new(),
            sample_rate: 1,
            problem: Some(problem),
        })
    }

    /// The file's name alone, for display.
    pub fn file_name(path: &str) -> &str {
        path.rsplit(['/', '\\']).next().unwrap_or(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};
    use std::path::PathBuf;

    fn temp_path(name: &str) -> String {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("audiochaos-sample-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_str().unwrap().to_string()
    }

    fn spec(channels: u16, bits: u16, format: SampleFormat) -> WavSpec {
        WavSpec {
            channels,
            sample_rate: 44_100,
            bits_per_sample: bits,
            sample_format: format,
        }
    }

    #[test]
    fn reads_16_bit_stereo_as_mono() {
        let path = temp_path("stereo16.wav");
        let mut wav = WavWriter::create(&path, spec(2, 16, SampleFormat::Int)).unwrap();
        for s in [16_384_i16, 0, -16_384, -16_384] {
            wav.write_sample(s).unwrap();
        }
        wav.finalize().unwrap();
        let sample = Sample::load(&path).unwrap();
        assert_eq!(sample.frames, [0.25, -0.5]);
        assert_eq!(sample.sample_rate, 44_100);
        assert_eq!(sample.path, path);
    }

    #[test]
    fn reads_24_bit_and_float() {
        let path = temp_path("int24.wav");
        let mut wav = WavWriter::create(&path, spec(1, 24, SampleFormat::Int)).unwrap();
        wav.write_sample(1 << 22).unwrap();
        wav.finalize().unwrap();
        assert_eq!(Sample::load(&path).unwrap().frames, [0.5]);

        let path = temp_path("float.wav");
        let mut wav = WavWriter::create(&path, spec(1, 32, SampleFormat::Float)).unwrap();
        wav.write_sample(-0.75_f32).unwrap();
        wav.finalize().unwrap();
        assert_eq!(Sample::load(&path).unwrap().frames, [-0.75]);
    }

    #[test]
    fn unreadable_files_are_errors_or_silence() {
        let path = temp_path("does-not-exist.wav");
        assert!(Sample::load(&path).is_err());
        let silent = Sample::load_or_silence(&path);
        assert!(silent.frames.is_empty());
        assert!(silent.problem.is_some());
        assert_eq!(silent.path, path);

        let path = temp_path("not-audio.wav");
        std::fs::write(&path, "hello").unwrap();
        assert!(Sample::load(&path).is_err());

        let path = temp_path("empty.wav");
        WavWriter::create(&path, spec(1, 16, SampleFormat::Int))
            .unwrap()
            .finalize()
            .unwrap();
        assert!(Sample::load(&path).is_err());
    }

    #[test]
    fn file_name_drops_the_directories() {
        assert_eq!(Sample::file_name("/a/b/kick.wav"), "kick.wav");
        assert_eq!(Sample::file_name("kick.wav"), "kick.wav");
        assert_eq!(Sample::file_name("C:\\a\\kick.wav"), "kick.wav");
    }
}
