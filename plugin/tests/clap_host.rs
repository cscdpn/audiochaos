//! Drives the plugin through its CLAP entry point, as a host would: create,
//! activate, process audio with notes, save and restore its state. This goes
//! through nice-plug's wrapper too, which the unit tests do not.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use audiochaos::patch::Patch;
use audiochaos::presets;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_NOTE_ON, CLAP_EVENT_TRANSPORT,
    CLAP_TRANSPORT_HAS_BEATS_TIMELINE, CLAP_TRANSPORT_HAS_TEMPO, CLAP_TRANSPORT_IS_PLAYING,
    clap_event_header, clap_event_note, clap_event_transport, clap_input_events,
    clap_output_events,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::{CLAP_PROCESS_ERROR, clap_process};
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;

const SAMPLE_RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 512;

unsafe extern "C" fn no_extension(_: *const clap_host, _: *const c_char) -> *const c_void {
    ptr::null()
}

unsafe extern "C" fn ignore(_: *const clap_host) {}

/// A host offering no extensions, which plugins must cope with.
fn host() -> Box<clap_host> {
    Box::new(clap_host {
        clap_version: CLAP_VERSION,
        host_data: ptr::null_mut(),
        name: c"audiochaos tests".as_ptr(),
        vendor: c"".as_ptr(),
        url: c"".as_ptr(),
        version: c"0".as_ptr(),
        get_extension: Some(no_extension),
        request_restart: Some(ignore),
        request_process: Some(ignore),
        request_callback: Some(ignore),
    })
}

/// A running plugin instance. Everything happens on the test's thread,
/// which the plugin takes for both the main and the audio thread.
struct Instance {
    plugin: *const clap_plugin,
    /// Must outlive the plugin, which keeps a pointer to it.
    _host: Box<clap_host>,
}

impl Instance {
    fn new() -> Self {
        let host = host();
        let entry = &audiochaos_plugin::clap_entry;
        unsafe {
            assert!((entry.init.unwrap())(c"".as_ptr()));
            let factory = (entry.get_factory.unwrap())(CLAP_PLUGIN_FACTORY_ID.as_ptr())
                as *const clap_plugin_factory;
            let factory = &*factory;
            assert_eq!((factory.get_plugin_count.unwrap())(factory), 1);
            let descriptor = &*(factory.get_plugin_descriptor.unwrap())(factory, 0);
            let id = CStr::from_ptr(descriptor.id);
            assert_eq!(id, c"io.github.cscdpn.audiochaos");
            let plugin = (factory.create_plugin.unwrap())(factory, &*host, id.as_ptr());
            assert!(!plugin.is_null());
            let p = &*plugin;
            assert!((p.init.unwrap())(plugin));
            assert!((p.activate.unwrap())(plugin, SAMPLE_RATE, 1, MAX_FRAMES));
            assert!((p.start_processing.unwrap())(plugin));
            Self {
                plugin,
                _host: host,
            }
        }
    }

    fn plugin(&self) -> &clap_plugin {
        unsafe { &*self.plugin }
    }

    /// `seconds` of output, left channel, with `notes` at the start.
    fn play(&self, notes: &[clap_event_note], seconds: f64) -> Vec<f32> {
        self.play_song(notes, None, seconds)
    }

    /// The same, while a song at `tempo` plays from its start, if any.
    fn play_song(&self, notes: &[clap_event_note], tempo: Option<f64>, seconds: f64) -> Vec<f32> {
        let mut heard = Vec::new();
        let total = (seconds * SAMPLE_RATE) as usize;
        let mut first = true;
        while heard.len() < total {
            let frames = MAX_FRAMES as usize;
            let events = if first { notes } else { &[] };
            first = false;
            let song = tempo.map(|tempo| {
                let beats = heard.len() as f64 / SAMPLE_RATE * tempo / 60.0;
                playing(tempo, beats)
            });
            let (left, right) = self.process(events, song.as_ref(), frames);
            assert_eq!(left, right, "the same sound on both sides");
            heard.extend(left);
        }
        heard
    }

    /// Loads `patch` as a host restores a project.
    fn load_patch(&self, patch: &Patch) {
        // nice-plug's JSON, after its length on 8 bytes.
        let saved = self.save();
        let mut state: serde_json::Value = serde_json::from_slice(&saved[8..]).unwrap();
        let fields = state["fields"].as_object_mut().expect("persistent fields");
        assert!(
            fields.contains_key("patch"),
            "the patch is saved: {fields:?}"
        );
        fields.insert(
            "patch".to_string(),
            serde_json::Value::String(serde_json::to_string(patch).unwrap()),
        );
        let json = serde_json::to_vec(&state).unwrap();
        let mut edited = (json.len() as u64).to_le_bytes().to_vec();
        edited.extend(json);
        self.load(&edited);
    }

    fn process(
        &self,
        notes: &[clap_event_note],
        song: Option<&clap_event_transport>,
        frames: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut inputs = [vec![0.0f32; frames], vec![0.0f32; frames]];
        let mut outputs = [vec![0.0f32; frames], vec![0.0f32; frames]];
        let mut input_channels = inputs.each_mut().map(|c| c.as_mut_ptr());
        let mut output_channels = outputs.each_mut().map(|c| c.as_mut_ptr());
        let input = clap_audio_buffer {
            data32: input_channels.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let mut output = clap_audio_buffer {
            data32: output_channels.as_mut_ptr(),
            ..input
        };
        let in_events = clap_input_events {
            ctx: notes as *const [clap_event_note] as *mut c_void,
            size: Some(event_count),
            get: Some(event),
        };
        let out_events = clap_output_events {
            ctx: ptr::null_mut(),
            try_push: Some(drop_event),
        };
        // The slice's length travels separately: `ctx` is a thin pointer.
        COUNT.with(|count| count.set(notes.len()));
        let process = clap_process {
            steady_time: -1,
            frames_count: frames as u32,
            transport: song.map_or(ptr::null(), |song| song as *const _),
            audio_inputs: &input,
            audio_outputs: &mut output,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: &in_events,
            out_events: &out_events,
        };
        let status = unsafe { (self.plugin().process.unwrap())(self.plugin, &process) };
        assert_ne!(status, CLAP_PROCESS_ERROR);
        let [left, right] = outputs;
        (left, right)
    }

    fn state(&self) -> &clap_plugin_state {
        unsafe {
            let state =
                (self.plugin().get_extension.unwrap())(self.plugin, CLAP_EXT_STATE.as_ptr());
            assert!(!state.is_null(), "the plugin saves its state");
            &*(state as *const clap_plugin_state)
        }
    }

    fn save(&self) -> Vec<u8> {
        let mut saved = Vec::new();
        let stream = clap_ostream {
            ctx: &mut saved as *mut Vec<u8> as *mut c_void,
            write: Some(write),
        };
        assert!(unsafe { (self.state().save.unwrap())(self.plugin, &stream) });
        saved
    }

    fn load(&self, saved: &[u8]) {
        let mut reader = Reader {
            data: saved,
            read: 0,
        };
        let stream = clap_istream {
            ctx: &mut reader as *mut Reader as *mut c_void,
            read: Some(read),
        };
        assert!(unsafe { (self.state().load.unwrap())(self.plugin, &stream) });
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let p = self.plugin();
        unsafe {
            (p.stop_processing.unwrap())(self.plugin);
            (p.deactivate.unwrap())(self.plugin);
            (p.destroy.unwrap())(self.plugin);
        }
    }
}

thread_local! {
    static COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

unsafe extern "C" fn event_count(_: *const clap_input_events) -> u32 {
    COUNT.with(|count| count.get()) as u32
}

unsafe extern "C" fn event(list: *const clap_input_events, index: u32) -> *const clap_event_header {
    unsafe {
        let notes = (*list).ctx as *const clap_event_note;
        &(*notes.add(index as usize)).header
    }
}

unsafe extern "C" fn drop_event(_: *const clap_output_events, _: *const clap_event_header) -> bool {
    true
}

unsafe extern "C" fn write(stream: *const clap_ostream, buffer: *const c_void, size: u64) -> i64 {
    unsafe {
        let saved = &mut *((*stream).ctx as *mut Vec<u8>);
        saved.extend_from_slice(std::slice::from_raw_parts(
            buffer as *const u8,
            size as usize,
        ));
    }
    size as i64
}

struct Reader<'a> {
    data: &'a [u8],
    read: usize,
}

unsafe extern "C" fn read(stream: *const clap_istream, buffer: *mut c_void, size: u64) -> i64 {
    unsafe {
        let reader = &mut *((*stream).ctx as *mut Reader);
        let rest = &reader.data[reader.read..];
        let n = rest.len().min(size as usize);
        ptr::copy_nonoverlapping(rest.as_ptr(), buffer as *mut u8, n);
        reader.read += n;
        n as i64
    }
}

fn note_on(key: i16) -> clap_event_note {
    clap_event_note {
        header: clap_event_header {
            size: size_of::<clap_event_note>() as u32,
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_NOTE_ON,
            flags: 0,
        },
        note_id: -1,
        port_index: 0,
        channel: 0,
        key,
        velocity: 0.8,
    }
}

/// A song playing at `tempo`, `beats` from its start.
fn playing(tempo: f64, beats: f64) -> clap_event_transport {
    clap_event_transport {
        header: clap_event_header {
            size: size_of::<clap_event_transport>() as u32,
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_TRANSPORT,
            flags: 0,
        },
        flags: CLAP_TRANSPORT_HAS_TEMPO
            | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
            | CLAP_TRANSPORT_IS_PLAYING,
        song_pos_beats: (beats * CLAP_BEATTIME_FACTOR as f64) as i64,
        song_pos_seconds: 0,
        tempo,
        tempo_inc: 0.0,
        loop_start_beats: 0,
        loop_end_beats: 0,
        loop_start_seconds: 0,
        loop_end_seconds: 0,
        bar_start: 0,
        bar_number: 0,
        tsig_num: 4,
        tsig_denom: 4,
    }
}

fn loudest(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |max, s| max.max(s.abs()))
}

#[test]
fn a_note_from_the_host_plays() {
    let instance = Instance::new();
    assert!(
        loudest(&instance.play(&[], 0.5)) < 0.01,
        "silent until played"
    );
    let heard = instance.play(&[note_on(60)], 1.0);
    assert!(loudest(&heard) > 0.1);
    assert!(heard.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
}

#[test]
fn the_state_restores_in_another_instance() {
    let saving = Instance::new();
    let saved = saving.save();
    let restoring = Instance::new();
    restoring.load(&saved);
    assert_eq!(restoring.save(), saved);
}

#[test]
fn a_restored_patch_is_the_one_that_plays() {
    let instance = Instance::new();
    // The chaos preset plays on its own, without a note.
    instance.load_patch(&presets::get("chaos").unwrap());
    assert!(loudest(&instance.play(&[], 1.0)) > 0.1);
}

#[test]
fn clocks_follow_the_songs_tempo_and_bars() {
    let instance = Instance::new();
    // A saw let through on each beat of a clock whose knob says 120 BPM.
    let patch = Patch::from_json(
        r#"{
            "version": 2,
            "modules": [
                { "name": "clock", "type": "clock" },
                { "name": "osc", "type": "osc" },
                { "name": "vca", "type": "vca" },
                { "name": "master", "type": "master" }
            ],
            "knobs": { "clock": { "tempo": 120.0 }, "vca": { "gain": 0.0 } },
            "cables": [
                { "from": "clock", "output": "beat", "to": "vca", "input": "mod" },
                { "from": "osc", "output": "saw", "to": "vca", "input": "in" },
                { "from": "vca", "output": "out", "to": "master", "input": "in" }
            ]
        }"#,
    )
    .unwrap();
    instance.load_patch(&patch);
    // The song at 60 BPM: a beat a second, sounding for its first half.
    let heard = instance.play_song(&[], Some(60.0), 2.0);
    let window = |from: f64, to: f64| {
        let at = |t: f64| (t * SAMPLE_RATE) as usize;
        loudest(&heard[at(from)..at(to)])
    };
    assert!(window(0.05, 0.45) > 0.1, "first half of the first beat");
    assert!(window(0.55, 0.95) < 0.01, "second half, silent");
    assert!(window(1.05, 1.45) > 0.1, "the next beat");
}
