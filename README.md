# audiochaos

A modular synthesizer written in Rust, built for chaotic, aggressive sounds:
small signal-processing bricks that you route together however you like,
in a graphical node editor or a terminal UI.

## Using it

`just run` starts the sound and the editor on the `chaos` preset: two
oscillators frequency-modulating each other, folded, crushed and fed back
through the delay, with random voltages retuning them. `just run bass`
starts on a plain sequenced bassline instead, a calm base to patch from.

### Graphical editor

One block per module, signal inputs (orange) and knob CV inputs (blue) on the
left, outputs on the right, sliders for the knobs.

- Drag from an output to an input to connect; drag a block by its title.
- Right-click a wire to remove it, or a pin to remove all of its wires.
- Ctrl+drag from a pin picks up its wires to move them to another pin;
  dropping them on nothing removes them.
- The canvas pans and zooms with the mouse.
- **Patch** menu: load a built-in preset, **Import…** a patch file, or
  **Export…** the current patch, block positions included.

### Terminal UI

`just tui` (or `--tui`) runs the same synth in the terminal, handy over SSH.

- **Knobs screen**: `↑↓` select a knob, `←→` turn it, `Shift+←→` (or `H`/`L`)
  turn it faster.
- **Patch screen** (`Tab`): `←→` switch between the outputs and inputs
  columns, `↑↓` select, `Enter` connects the selected output to the selected
  input, or disconnects it if already connected. In the inputs column, `●`
  marks a cable from the selected output and `○` a cable from elsewhere.
- `q` or `Esc` quits.

Every knob has a matching `cv` input: patch any signal into it to modulate
the knob. A ±1 signal sweeps the knob's whole range, so go through `atten`
to make a modulation subtle or to invert it.

### Patches

A patch is a JSON file holding knob values and cables, with modules, knobs
and ports referred to by name:

```json
{
  "version": 1,
  "knobs": { "osc 1": { "freq": 55.0 } },
  "cables": [{ "from": "osc 1", "output": "saw", "to": "master", "input": "in" }]
}
```

Knobs left out are at their default, so a patch always fully determines the
sound. The built-in presets live in `presets/` in the same format and are
compiled into the binary. `just run path/to/patch.json` starts on a patch
file; an unknown module, knob or port is reported by name. Block positions
from the graphical editor are saved under `positions`; the terminal UI
ignores them.

### Modules

| Module | Does |
|---|---|
| seq | 8-step CV sequencer with its own clock, `cv` and `gate` outputs |
| lfo | slow sine, triangle, saw and square |
| noise | white noise |
| s&h | sample & hold: grabs `in` on each rising edge of `trigger` |
| atten | three attenuverters: each output is its input times -1..1 |
| osc 1, osc 2 | sine, saw, square, with through-zero linear FM |
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
- Modules are processed in rack order. A cable from a module processed later
  (or from itself) reads its previous block, so feedback is allowed and costs
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
- The editor uses `eframe` with the glow (OpenGL) backend rather than the
  default wgpu, for far fewer dependencies, and is pinned to 0.36 because the
  node-graph widget, `egui-snarl` 0.12, is built on egui 0.36. Nothing on the audio thread allocates:
  delay lines are allocated up front and cables live in a fixed-capacity set.
- The rack is fixed for now: modules cannot be added or removed at runtime.
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
just tui          # terminal UI instead of the editor
just test
just lint   # cargo fmt --check + clippy
just fmt
```

[`just`](https://github.com/casey/just) is optional; the recipes are thin
wrappers around the usual `cargo` commands in the `justfile`.

Dev builds use `opt-level = 1`: unoptimized DSP made the stability tests
slow and risked audio dropouts.

The header shows an audio error count. One error at startup is normal with
the ALSA backend; a count that keeps rising means buffer underruns.
