# audiochaos

A modular synthesizer written in Rust, built for chaotic, aggressive sounds:
small signal-processing bricks that you route together however you like,
in a graphical node editor or a terminal UI.

## Using it

`just run` starts the sound and the editor on the `chaos` preset: two
oscillators frequency-modulating each other, folded, crushed and fed back
through the delay, with random voltages retuning them. `just run bass`
starts on a plain sequenced bassline instead, a calm base to patch from, and
`just run groovygroove` on a groovier one. `just run keys` is played from a
MIDI controller (see [MIDI controllers](#midi-controllers)).

### Graphical editor

One block per module, signal inputs (orange) and knob CV inputs (blue) on the
left, outputs on the right, sliders for the knobs.

- Right-click the canvas to add a module where you clicked; right-click a
  block's title to remove it. New modules are named after their type
  (`filter`, `filter 2`…). The master is the one output and stays.
- Drag from an output to an input to connect; drag a block by its title.
- Right-click a wire to remove it, or a pin to remove all of its wires.
- Ctrl+drag from a pin picks up its wires to move them to another pin;
  dropping them on nothing removes them.
- The canvas pans and zooms with the mouse.
- **Patch** menu: load a built-in preset, **Import…** a patch file, or
  **Export…** the current patch, block positions included.
- **● Rec** records what you hear (see [Recording](#recording)).

### Terminal UI

`just tui` (or `--tui`) runs the same synth in the terminal, handy over SSH.
It patches and tweaks the modules of the patch it starts on, but cannot add
or remove modules.

- **Knobs screen**: `↑↓` select a knob, `←→` turn it, `Shift+←→` (or `H`/`L`)
  turn it faster.
- **Patch screen** (`Tab`): `←→` switch between the outputs and inputs
  columns, `↑↓` select, `Enter` connects the selected output to the selected
  input, or disconnects it if already connected. In the inputs column, `●`
  marks a cable from the selected output and `○` a cable from elsewhere.
- `r` starts or stops recording.
- `q` or `Esc` quits.

Every knob has a matching `cv` input: patch any signal into it to modulate
the knob. A ±1 signal sweeps the knob's whole range, so go through `atten`
to make a modulation subtle or to invert it.

### Recording

**● Rec** in the editor, or `r` in the terminal UI, records what the speakers
play into `recording-001.wav`, `recording-002.wav`… in the current directory,
until pressed again. Quitting mid-recording still saves a complete file.

`--render` writes a patch to a file without the sound card, much faster than
real time, at 48 kHz:

```sh
just run --render out.wav --seconds 60 bass
```

Files are mono 16-bit WAV, which plays anywhere; the master never leaves
-1..1, so 16 bits lose nothing audible. Neither way overwrites an existing
file.

### MIDI controllers

Every MIDI controller plugged in at startup plays the `midi` modules; the
status bar names the ones found. `just run keys` is a ready-made patch:
keys play a filtered saw, knobs 1 to 4 sweep the cutoff, resonance, delay
feedback and self-FM, and pad 1 fires a noise burst.

A `midi` module outputs:

- `pitch`, `gate`, `velocity`: the keyboard, one note at a time; releasing a
  key falls back to the one still held. Patched into an oscillator's
  `freq cv`, `pitch` plays in tune, the `freq` knob setting the pitch of
  middle C. Pitch bend adds up to two semitones either way.
- `pad 1` to `pad 8`: a gate per pad, from notes on channel 10, the usual
  channel for pads. Pads start at note 36, and further banks land on the
  same eight outputs.
- `knob 1` to `knob 8`: controllers 1 to 8, from 0 to 1, smoothed.

These are the defaults of most small controllers, the Akai MPK mini among
them. `cargo run -- --midi-monitor` prints what a controller actually sends,
to check. On Linux, MIDI needs the ALSA sequencer: if the status bar says
"no MIDI", load it with `sudo modprobe snd-seq`.

### Patches

A patch is a JSON file listing its modules, knob values and cables, with
modules, knobs and ports referred to by name:

```json
{
  "version": 2,
  "modules": [
    { "name": "osc 1", "type": "osc" },
    { "name": "master", "type": "master" }
  ],
  "knobs": { "osc 1": { "freq": 55.0 } },
  "cables": [{ "from": "osc 1", "output": "saw", "to": "master", "input": "in" }]
}
```

Modules are processed in the order listed, and a patch needs exactly one
`master`. Knobs left out are at their default, so a patch always fully
determines the sound. Version 1 files, from before modules could be added
and removed, still load: they get the old fixed rack. The built-in presets
live in `presets/` in the same format and are compiled into the binary. `just run path/to/patch.json` starts on a patch
file; an unknown module, knob or port is reported by name. Block positions
from the graphical editor are saved under `positions`; the terminal UI
ignores them.

### Modules

Module types, as patches and the editor's palette name them:

| Type | Does |
|---|---|
| midi | a MIDI controller's keyboard, pads and knobs as voltages |
| seq | 8-step CV sequencer with its own clock, `cv` and `gate` outputs |
| lfo | slow sine, triangle, saw and square |
| noise | white noise |
| s&h | sample & hold: grabs `in` on each rising edge of `trigger` |
| atten | three attenuverters: each output is its input times -1..1 |
| osc | sine, saw, square, with through-zero linear FM |
| filter | resonant state-variable filter: lowpass, bandpass, highpass |
| dist | drive into a blend of `tanh` clipping and sine wavefolding |
| crush | bitcrusher: bit depth and sample-rate reduction |
| delay | up to 2 s, feedback above 1 self-oscillates instead of exploding |
| vca | `out = in * (gain + mod)`, ring modulator when `mod` is audio |
| master | volume and soft limiter, the only thing the speakers hear |

Some starting points for noise: patch an oscillator into its own `fm`, feed
the delay back into an oscillator's `fm`, push filter resonance to the max,
patch noise through `s&h` into a `freq cv`.

## Design notes

- Audio and control signals share the same `f32` buffers, like voltages in a
  hardware modular.
- Modules are processed in the order they were added. A cable from a module
  processed later (or from itself) reads its previous block, so feedback is allowed and costs
  exactly one block (64 samples) of delay. Feedback is a main source of chaos
  here.
- The graph owns knob smoothing and modulation: each knob's CV is added in
  normalized knob space (logarithmic for frequencies and times), so modules
  just receive per-sample parameter values.
- Every module input is clamped to ±10 and NaN reads as 0, like the power
  rails of a hardware modular. Without it, feedback through VCAs runs off to
  infinity and a single non-finite sample poisons a filter's state for good.
- The master soft-clips with `tanh` and mutes non-finite samples, so the
  speakers never see an exploded patch.
- The audio thread owns the graph, so it needs no lock. The UI talks to it
  through a lock-free queue (`rtrb`) drained once per block, and keeps its own
  mirror of the patch for display. Both front ends are clients of that same
  `Controller`; in the editor, a wire is drawn only once the controller has
  accepted the cable.
- MIDI input follows it too: midir's threads decode messages into atomics
  that `midi` modules read once per block, so a burst of knob turns never
  stalls the audio.
- Live recording follows the same rule: the audio thread copies samples into
  a second lock-free queue, and a writer thread empties it to disk. The queue
  holds a second of audio; if the disk stalls longer, samples are dropped and
  counted rather than glitching the sound.
- The editor uses `eframe` with the glow (OpenGL) backend rather than the
  default wgpu, for far fewer dependencies, and is pinned to 0.36 because the
  node-graph widget, `egui-snarl` 0.12, is built on egui 0.36.
- Nothing on the audio thread allocates: delay lines are allocated up front
  and cables live in a fixed-capacity set.
- Adding a module builds it on the UI thread, buffers included, and moves it
  to the audio thread whole; a removed module travels back through a second
  queue to be freed on the UI thread. Modules sit in a fixed number of
  preallocated slots (64), so neither side ever reallocates. Loading a patch
  swaps in a whole new graph in one go.
- The engine is a library that renders offline, so it is fully testable
  without any audio hardware. A fuzz test renders random patches with random
  knob settings and checks the output stays finite and bounded.

## Build, run, test

Requires a recent stable Rust toolchain and, on Linux, the ALSA development
headers (`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

```sh
just build
just run          # chaos preset
just run bass     # bass preset
just run my.json  # a patch file
just run --render out.wav --seconds 60   # render to WAV, no sound card
just run --midi-monitor                  # what the MIDI controllers send
just tui          # terminal UI instead of the editor
just test
just lint   # cargo fmt --check + clippy
just fmt
```

[`just`](https://github.com/casey/just) is optional; the recipes are thin
wrappers around the usual `cargo` commands in the `justfile`.

Dev builds use `opt-level = 1`: unoptimized DSP made the stability tests
slow and risked audio dropouts.

A USB controller plugged in at boot can become ALSA's card 0, the system
default, although it has no audio output. The first sound card that opens then
takes over, after a few harmless `ALSA lib` lines on the terminal.

The header shows an audio error count. One error at startup is normal with
the ALSA backend; a count that keeps rising means buffer underruns.
