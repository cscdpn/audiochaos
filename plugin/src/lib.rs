//! audiochaos as a VST3 and CLAP instrument: the same engine and node editor
//! as the standalone program, with the host providing audio, MIDI and a
//! window, and saving the patch with the project.

mod gui;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use audiochaos::engine::control::Engine;
use audiochaos::engine::input::LiveInput;
use audiochaos::gui::Editor;
use audiochaos::midi::{self, Parser};
use audiochaos::modules::Context;
use audiochaos::patch::Patch;
use audiochaos::presets;
use nice_plug::midi::MidiResult;
use nice_plug::params::persist::PersistentField;
use nice_plug::prelude::*;
use nice_plug_egui::EguiEditorState;

/// Room for a burst of edits, or a whole patch being loaded, even while the
/// host is not processing: commands wait in the queue until it does.
const COMMAND_QUEUE_CAPACITY: usize = 1024;
/// The rate the patch is built at until the host says which it runs at.
const DEFAULT_SAMPLE_RATE: f32 = 48_000.0;
/// What a new instance plays: one played from the keyboard, rather than the
/// standalone program's default, which plays on its own.
const FIRST_PRESET: &str = "keys";

pub struct Audiochaos {
    params: Arc<AudiochaosParams>,
    /// The audio side, owned by the host's audio thread through `process`.
    engine: Engine,
    /// The UI side: the patch as the editor shows it. The host's threads
    /// reach it to save and restore the patch, the window to draw it; the
    /// audio thread never does.
    editor: Arc<Mutex<Editor>>,
    editor_state: Arc<EguiEditorState>,
    sample_rate: f32,
    midi: Arc<midi::State>,
    parser: Parser,
    input: Arc<LiveInput>,
    /// Whether the host feeds audio in, as the layout it picked says.
    has_input: bool,
    /// The knob values last passed on, to pass on changes only and leave
    /// the knobs to MIDI controllers otherwise.
    knobs: [f32; midi::KNOBS],
}

#[derive(Params)]
pub struct AudiochaosParams {
    /// The patch, saved with the host's project.
    #[persist = "patch"]
    patch: PatchField,
    /// The `midi` modules' knobs, for the host to automate: a MIDI
    /// controller's knobs 1 to 8, in the same 128 steps.
    #[nested(array, group = "Knob")]
    knobs: [KnobParam; midi::KNOBS],
}

#[derive(Params)]
struct KnobParam {
    #[id = "knob"]
    value: FloatParam,
}

impl KnobParam {
    fn new(number: usize) -> Self {
        Self {
            value: FloatParam::new(
                format!("Knob {number}"),
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_step_size(1.0 / 127.0),
        }
    }
}

/// The patch, as a persistent field: read from the editor when the host
/// saves, loaded into it when the host restores.
struct PatchField(Arc<Mutex<Editor>>);

impl<'a> PersistentField<'a, Patch> for PatchField {
    fn set(&self, patch: Patch) {
        let mut editor = lock(&self.0);
        if let Err(e) = editor.load(&patch) {
            editor.status = format!("cannot load the saved patch: {e}");
        }
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&Patch) -> R,
    {
        f(&lock(&self.0).patch())
    }
}

/// The editor even if a panic while drawing poisoned its lock: a
/// half-drawn frame leaves nothing inconsistent behind.
fn lock(editor: &Mutex<Editor>) -> MutexGuard<'_, Editor> {
    editor.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Audiochaos {
    fn default() -> Self {
        let midi = Arc::new(midi::State::default());
        let input = Arc::new(LiveInput::default());
        let context = Context {
            sample_rate: DEFAULT_SAMPLE_RATE,
            midi: Arc::clone(&midi),
            input: Arc::clone(&input),
        };
        let patch = presets::get(FIRST_PRESET).expect("the first preset exists");
        let (graph, output) = patch.build(&context).expect("presets build");
        let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
        let editor = Arc::new(Mutex::new(Editor::new(
            controller,
            context,
            &patch.positions,
        )));
        let params = Arc::new(AudiochaosParams {
            patch: PatchField(Arc::clone(&editor)),
            knobs: std::array::from_fn(|knob| KnobParam::new(knob + 1)),
        });
        Self {
            params,
            engine,
            editor,
            editor_state: gui::state(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            midi,
            parser: Parser::default(),
            input,
            has_input: false,
            knobs: [f32::NAN; midi::KNOBS],
        }
    }
}

impl Audiochaos {
    /// Builds the patch again for another rate, which every module is built
    /// for: knobs, cables and sounds stay, the modules' state starts afresh.
    fn set_sample_rate(&mut self, sample_rate: f32) -> Result<(), String> {
        if sample_rate == self.sample_rate {
            return Ok(());
        }
        let mut editor = lock(&self.editor);
        let patch = editor.patch();
        let context = Context {
            sample_rate,
            midi: Arc::clone(&self.midi),
            input: Arc::clone(&self.input),
        };
        let (graph, output) = patch
            .build(&context)
            .map_err(|e| format!("cannot build the patch at {sample_rate} Hz: {e}"))?;
        let (engine, controller) = Engine::new(graph, output, COMMAND_QUEUE_CAPACITY);
        editor.replace_engine(controller, context);
        // The old engine is dropped here, off the audio thread.
        self.engine = engine;
        self.sample_rate = sample_rate;
        Ok(())
    }

    /// Passes on the knobs the host moved since the last buffer.
    fn follow_knobs(&mut self) {
        for (knob, param) in self.params.knobs.iter().enumerate() {
            let value = param.value.value();
            if value != self.knobs[knob] {
                self.knobs[knob] = value;
                self.midi.set_knob(knob, value);
            }
        }
    }

    fn handle(&mut self, event: PluginNoteEvent<Self>) {
        if let Some(MidiResult::Basic(message)) = event.as_midi() {
            self.parser.handle(&message, &self.midi);
        }
    }
}

impl Plugin for Audiochaos {
    const NAME: &'static str = "audiochaos";
    const VENDOR: &'static str = "cscdpn";
    const URL: &'static str = "https://github.com/cscdpn/audiochaos";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// Stereo in and out, the input reaching `input` modules; or out only,
    /// for hosts that give instruments no input.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];

    /// Controllers and pitch bend too, which `midi` modules play.
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;

    type Editor = nice_plug_egui::EguiEditor<gui::Gui>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        gui::editor(Arc::clone(&self.editor_state), Arc::clone(&self.editor))
    }

    fn activate(
        &mut self,
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.has_input = audio_io_layout.main_input_channels.is_some();
        match self.set_sample_rate(buffer_config.sample_rate) {
            Ok(()) => true,
            Err(e) => {
                lock(&self.editor).status = e;
                false
            }
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.follow_knobs();
        let mut next_event = context.next_event();
        for (frame, mut samples) in buffer.iter_samples().enumerate() {
            while let Some(event) = next_event
                && event.timing() as usize <= frame
            {
                self.handle(event);
                next_event = context.next_event();
            }
            let mut channel = |c| samples.get_mut(c).map_or(0.0, |s| *s);
            let input = if self.has_input {
                [channel(0), channel(1)]
            } else {
                [0.0; 2]
            };
            let sample = self.engine.next_sample_after(input, &self.input);
            for s in samples {
                *s = sample;
            }
        }
        // Events timed past the buffer, if a host sends any.
        while let Some(event) = next_event {
            self.handle(event);
            next_event = context.next_event();
        }
        // Sequencers, LFOs and delays sound on without any input.
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for Audiochaos {
    const CLAP_ID: &'static str = "io.github.cscdpn.audiochaos";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A modular synth for chaotic, aggressive sounds");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for Audiochaos {
    const VST3_CLASS_ID: [u8; 16] = *b"audiochaos-synth";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nice_export_clap!(Audiochaos);
nice_export_vst3!(Audiochaos);

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of output, with no audio coming in.
    fn play(plugin: &mut Audiochaos) -> Vec<f32> {
        let frames = plugin.sample_rate as usize;
        (0..frames)
            .map(|_| plugin.engine.next_sample_after([0.0; 2], &plugin.input))
            .collect()
    }

    fn loudest(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |max, s| max.max(s.abs()))
    }

    #[test]
    fn the_saved_patch_restores_in_another_instance() {
        let saving = Audiochaos::default();
        let bass = presets::get("bass").unwrap();
        lock(&saving.editor).load(&bass).unwrap();
        let saved = saving.params.patch.map(|patch| patch.to_json());

        let restoring = Audiochaos::default();
        restoring
            .params
            .patch
            .set(Patch::from_json(&saved).unwrap());
        assert_eq!(lock(&restoring.editor).patch().to_json(), saved);
    }

    #[test]
    fn another_rate_builds_the_same_patch_again() {
        let mut plugin = Audiochaos::default();
        let before = lock(&plugin.editor).patch();
        plugin.set_sample_rate(44_100.0).unwrap();
        assert_eq!(plugin.sample_rate, 44_100.0);
        assert_eq!(lock(&plugin.editor).patch(), before);
        assert!(play(&mut plugin).iter().all(|s| s.is_finite()));
    }

    #[test]
    fn a_note_from_the_host_plays_the_keys() {
        let mut plugin = Audiochaos::default();
        assert!(loudest(&play(&mut plugin)) < 0.01, "silent until played");
        plugin.handle(NoteEvent::from_midi(0, &[0x90, 60, 100]).unwrap());
        assert!(loudest(&play(&mut plugin)) > 0.1);
    }

    #[test]
    fn host_knobs_only_take_over_when_the_host_moves_them() {
        let mut plugin = Audiochaos::default();
        plugin.follow_knobs();
        assert_eq!(plugin.midi.knob(0), 0.0);
        // A controller turns knob 1; the host's knob has not moved since.
        plugin.handle(NoteEvent::from_midi(0, &[0xb0, 1, 127]).unwrap());
        plugin.follow_knobs();
        assert_eq!(plugin.midi.knob(0), 1.0);
    }
}
