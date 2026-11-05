# Architecture

How audiochaos is put together, and why. Read this before changing the
engine; [CONTRIBUTING.md](CONTRIBUTING.md) says how to work on it, the
[README](README.md) what it does.

## Map

```
src/
  engine/        the real-time core, no UI and no sound card
    module.rs      the Module trait, BLOCK (64 samples), ModuleInfo
    param.rs       knobs: ranges, scales, smoothing
    graph.rs       modules + cables, computed block by block
    control.rs     Engine (audio thread) and Controller (UI), joined by queues
    input.rs       the sound card's input, from its callback to the engine
    record.rs      WAV: offline renders, live recording through a writer thread
    sample.rs      sound files in memory, for samplers
    transport.rs   the DAW's tempo and position, for clocks
    audio.rs       cpal: sound cards, streams, low latency (feature app)
  modules/       one file per module; mod.rs is the registry (KINDS)
  midi/          MIDI bytes -> atomics (mod.rs); controller ports (ports.rs, app)
  patch.rs       the JSON patch format; graphs built from patches; group edits
  presets.rs     the presets in presets/*.json, compiled in
  gui/
    editor/        the node editor, shared by the program and the plugin
    toolbar.rs     Patch menu and Stop button, shared too
    app.rs         the standalone window: Audio menu, Rec (feature app)
    keyboard.rs    the computer keyboard as a piano (feature app)
  tui/           the terminal UI, a fallback for SSH (feature app)
  session.rs     sound cards + engine + recorder for the program (feature app)
  main.rs        command line
plugin/          VST3 and CLAP instrument, on nice-plug
xtask/           `cargo xtask bundle`, nice-plug's plugin bundler
tests/           stability.rs (fuzzed patches), realtime.rs (no allocation)
presets/         built-in patches
```

Cargo features follow the layers: the core (`engine`, `modules`, `midi`,
`patch`) always builds; `editor` adds the node editor, which is all the
plugin takes; `app`, on by default, adds the sound card, MIDI ports,
eframe window and terminal UI of the standalone program; `asio` adds ASIO on
Windows.

## Threads

```
             standalone program                         plugin
UI thread    Controller, editor or TUI         host GUI    Mutex<Editor>
audio thread Engine (cpal output callback)     host audio  Engine (process)
input thread Capture (cpal input callback)        -        (input in process)
MIDI threads Parser -> midi::State atomics      host audio  Parser
writer       WAV recorder                          -
```

The one rule: **the audio thread never allocates, frees, locks or waits.**
Any of these can stall for longer than a block lasts, and the sound breaks
up. Everything else follows from it:

- The audio thread owns the graph, so it needs no lock. The UI talks to it
  through a lock-free queue (`rtrb`) drained once per block, and keeps its
  own mirror of the patch for display. Both front ends are clients of that
  same `Controller`; in the editor, a wire is drawn only once the controller
  has accepted the cable.
- Adding a module builds it on the UI thread, buffers included, and moves it
  to the audio thread whole; a removed module travels back through a second
  queue to be freed on the UI thread. Modules sit in a fixed number of
  preallocated slots (256), cables in a fixed-capacity set (1024), so neither side
  ever reallocates. Loading a patch swaps in a whole new graph in one go.
- A sound file is read on the UI thread and moved to the audio thread whole;
  the one it replaces comes back the same way as a removed module.
- MIDI: midir's threads, or the host, decode messages into atomics that
  `midi` modules read once per block, so a burst of knob turns never stalls
  the audio.
- Lights and the editor's blue ticks go the other way: after each block, the
  engine stores every module's brightness and every knob's modulated value
  in an atomic per slot, and the editor reads them when it draws, about 30
  times a second while some block has a light. What a light shows comes from
  inside the module, so it is the real pace, knob, CV and clock together. A
  light faster than about 15 Hz no longer blinks evenly.
- Stopping the engine is an atomic flag rather than a queued command, so it
  works even when the queue is full. A stopped engine still drains the queue
  and the input, so edits apply and the input never overflows; it only skips
  computing the graph once its 10 ms fade is done.
- Live recording: the audio thread copies samples into a second lock-free
  queue, and a writer thread empties it to disk. The queue holds a second of
  audio; if the disk stalls longer, samples are dropped and counted rather
  than glitching the sound.
- Two tests hold the rule: `tests/realtime.rs` counts allocations while
  every preset plays with notes, knobs and live edits, and expects none; the
  plugin's tests build nice-plug with `assert_process_allocs`, which aborts
  if `process` allocates.

## The engine

- Audio and control signals share the same `f32` buffers, like voltages in a
  hardware modular.
- Modules are processed in the order they were added. A cable from a module
  processed later (or from itself) reads its previous block, so feedback is
  allowed and costs exactly one block (64 samples) of delay. Feedback is a
  main source of chaos here.
- The graph owns knob smoothing and modulation: each knob's CV is added in
  normalized knob space (logarithmic for frequencies and times), so modules
  just receive per-sample parameter values.
- Every module input is clamped to ±10 and NaN reads as 0, like the power
  rails of a hardware modular. Without it, feedback through VCAs runs off to
  infinity and a single non-finite sample poisons a filter's state for good.
- The master soft-clips with `tanh` and mutes non-finite samples, so the
  speakers never see an exploded patch.
- A `clock` input changes how a module behaves only while a cable is in it:
  the graph tells modules when an input gains or loses its last cable, since
  an unplugged input reads the same zeros as a silent one.
- The modulation effects are modules rather than patches of `delay` and
  `lfo`: all-pass stages exist nowhere else, and a flanger's feedback lives
  within milliseconds that a cable's block of delay would already use up.
- Samplers handle a file's rate with their playback rate, with linear
  interpolation, which aliases at high speeds: welcome here.

## Patches

Patches name modules, knobs and ports rather than numbering them, so a file
reads well, can be written by hand, and stays valid when a module gains a
knob. `patch.rs` checks a patch against the registry before building
anything, so a mistake is reported by name and never half-applied. Block
positions are saved with the patch but only the editor reads them.

## The standalone program

- `session::start` opens the sound cards, builds the patch's graph at the
  card's rate and starts the streams. Switching cards builds a new session
  from the editor's patch: every module is built for one sample rate.
- Switching opens the new cards before closing the old, so a card that does
  not open leaves the sound playing. A card in low latency takes one program
  at a time, which may be the synth itself on the cards it is leaving: then
  the old streams stop, the new ones are tried again, and the old ones play
  again if that fails too.
- The sound card's input has a callback of its own, out of step with the
  output's, so frames go through a lock-free queue, and the engine takes a
  block from it before computing each block. The queue keeps a backlog of
  one callback of each stream, measured as they run: less and it runs dry
  between two input callbacks. When input and output are different devices,
  their clocks drift apart; the queue then catches up at once, or waits to
  refill, with a click, rather than letting the latency grow. The input is
  not resampled: it must run at the output's rate.
- Low latency on Linux opens ALSA's `hw:` device rather than `plughw:`,
  which would convert to floats for us: cpal only gives real-time priority
  to devices that cannot route to a sound server, and `plughw:` is a plugin.
  So the engine converts its samples to the card's own format, 32-bit
  integers on most cards. The menus still list each card by its default
  device; low latency opens the same card's `hw:` device, which ALSA names
  by index, found through `/proc/asound`.
- ASIO is behind a cargo feature because of its build: it needs LLVM, and
  downloads Steinberg's ASIO SDK, which is under GPLv3 or Steinberg's own
  licence. The source here stays MIT; a program built with ASIO falls under
  the SDK's licence too, which matters only when sharing that program.
- The editor uses `eframe` with the glow (OpenGL) backend rather than the
  default wgpu, for far fewer dependencies, and is pinned to egui 0.36
  because the node-graph widget, `egui-snarl` 0.12, is built on it.

## The plugin

- Built on [nice-plug](https://codeberg.org/RustAudio/nice-plug), the
  community continuation of NIH-plug, whose VST3 support uses MIT-licensed
  bindings, so the bundles stay MIT. It draws the editor with nice-plug-egui
  0.5, the last release on egui 0.36, the editor's version.
- The host's audio thread owns the engine as the sound card's callback does
  in the program. The editor, which holds the controller, sits behind a
  mutex that only the host's other threads take: the window to draw, the
  host to save and restore the patch. Saving reads the patch from the
  editor, so the project always holds what is on screen.
- Hosts hand over input and want output in the same call, frame by frame.
  Each input frame goes into the block the engine computes next, so the
  input comes out exactly one block late, whatever the host's buffer size,
  without the standalone program's queue.
- The host's tempo and position reach `clock` modules like MIDI does:
  atomics the plugin writes before each frame and the clocks read at the
  start of each block, so a block starts exactly where the song is.

## Testing

- The engine is a library that renders offline, so it is fully testable
  without any audio hardware: most tests build a graph, render it and
  measure the result (pitch from zero crossings, level from RMS...). The
  helpers are in `modules::test_util`.
- `tests/stability.rs` renders random patches with random knob settings and
  checks the output stays finite and bounded, and that no module gets stuck
  on non-finite state.
- The editor's tests draw it headless with egui and click on what it draws.
- `plugin/tests/clap_host.rs` drives the plugin through its CLAP entry point
  as a host would: notes in, sound out, state saved and restored, clocks
  following a song's tempo.
- What no test covers: real sound cards and MIDI controllers, the window
  itself, and the plugin inside a real DAW. Those are checked by hand.
- CI runs the tests on Linux and Windows at every push, and builds the
  release archives from tags (see [CONTRIBUTING.md](CONTRIBUTING.md#continuous-integration)).
