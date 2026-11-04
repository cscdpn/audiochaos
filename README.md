# audiochaos

[![CI](https://github.com/cscdpn/audiochaos/actions/workflows/ci.yml/badge.svg)](https://github.com/cscdpn/audiochaos/actions/workflows/ci.yml)

A modular synthesizer written in Rust, built for chaotic, aggressive sounds:
small signal-processing bricks that you route together however you like,
in a graphical node editor or a terminal UI, on its own or as a VST3 or CLAP
instrument in a DAW.

## Using it

`just run` starts the sound and the editor on the `chaos` preset: two
oscillators frequency-modulating each other, folded, crushed and fed back
through the delay, with random voltages retuning them. `just run bass`
starts on a plain sequenced bassline instead, a calm base to patch from, and
`just run groovygroove` on a groovier one. `just run keys` is played from a
MIDI controller (see [MIDI controllers](#midi-controllers)), and `just run
chords` too, four notes at once (see [Chords](#chords)). `just run looper`
records the sound card's input and loops it (see [Audio input and
looper](#audio-input-and-looper)).

### Graphical editor

One block per module, signal inputs (orange) and knob CV inputs (blue) on the
left, outputs on the right with what they carry (`±1`, `0..1`…), sliders for
the knobs.

- Right-click the canvas to add a module where you clicked; right-click a
  block's title to duplicate or remove it. New modules are named after their type
  (`filter`, `filter 2`…). The master is the one output and stays.
- Drag from an output to an input to connect; drag a block by its title.
- Right-click a wire to remove it, or a pin to remove all of its wires.
- Ctrl+drag from a pin picks up its wires to move them to another pin;
  dropping them on nothing removes them.
- Hover a slider, a block's title or a palette entry to see what it does.
- A blue tick on a slider's rail shows where CV actually takes the knob,
  when that is away from the slider's handle.
- Right-click a slider to reset it (see [below](#knobs-and-cv)).
- The canvas pans and zooms with the mouse.
- **Patch** menu: load a built-in preset, **Arrange blocks** into columns
  where none overlaps another, **Import…** a patch file, or **Export…** the
  current patch, block positions included.
- Shift+click blocks, or Shift+drag around them, to select several; dragging
  one of them moves them all. **Ctrl+D**, or **Duplicate** in a block's menu,
  copies them just below with their knobs, sounds and the cables between
  them: make a kick, duplicate it, retune the copy into a tom. Cables from
  outside the selection are not copied, so plug the copy's trigger again.
- **Ctrl+C** and **Ctrl+V** copy and paste selected blocks through the
  clipboard, also from one window to another. **Save selection…** keeps them
  in a file, in the patch format without a master, and **Insert blocks…**
  adds such a file, or the blocks of any patch but its master, to the one
  playing.
- The computer keyboard plays the `midi` modules, without a MIDI controller
  (see [below](#computer-keyboard)).
- A red light beside a block's title blinks at the pace the module actually
  runs at, CV and clock included, which the slider does not show: `lfo` and
  the modulation effects glow with their LFO (the univibe with its lamp),
  `delay` flashes on each echo, `clock` on each beat, `seq` with its gate.
- **⏸ Stop** silences the synth and holds everything still, sequencers and
  delay tails included; **▶ Start** plays on from there. Both fade over 10 ms,
  so neither clicks. Edits still apply while stopped.
- **● Rec** records what you hear (see [Recording](#recording)).
- **Audio** menu: pick the output and the input among the sound cards plugged
  in now, for when the synth started before the interface was plugged in, and
  **Reconnect MIDI controllers** plugged in since. Switching sound cards
  builds the patch again on the new card, which may run at another rate: knobs,
  cables and sounds stay, but sequencers start over and loopers forget their
  recording; a running recording is saved. If the card does not open, the
  synth plays on where it was. **Low latency** and **Buffer** set how
  quickly the synth answers (see [Low latency](#low-latency)).

### DAW plugin

The same synth runs as a VST3 and CLAP instrument in a DAW: Ardour, Reaper,
Bitwig and others. It is tested on Linux; on Windows it compiles but has not
been tried in a host yet. `just install-plugin` builds both and
copies them to `~/.vst3` and `~/.clap`, where Linux hosts look; on Windows,
build them with `just bundle` and copy `target/bundled/audiochaos.vst3` to
`C:\Program Files\Common Files\VST3` and `audiochaos.clap` to
`C:\Program Files\Common Files\CLAP`.
Ardour only loads VST3; Reaper and Bitwig take either.

- The plugin's window is the node editor, with the **Patch** menu and
  **Stop**: presets, import and export, groups of blocks all work.
- The patch is saved with the DAW's project, block positions included, and
  comes back when it is opened. A new instance starts on `keys`.
- The track's MIDI plays the `midi` and `poly` modules, as a controller
  would: notes, pitch bend and controllers 1 to 8 for the knobs.
- **Knob 1** to **Knob 8** are the `midi` modules' knobs as parameters the DAW
  can automate. Moving one in the DAW takes over that knob; a controller
  turning it after that takes it back.
- `clock` blocks follow the song: its tempo always, and while it plays,
  its position, so bars start with the song's bars (in 4/4). Stopped, they
  run on at the song's tempo, so a patch still plays live. Clock the
  sequencers, LFOs and delays from a `clock` to keep them in time.
- The track's audio reaches `input` modules, one 64-sample block late.
- At another sample rate, the patch is built again: knobs, cables and sounds
  stay, sequencers start over and loopers forget their recording.

The sound card, MIDI ports, recording and computer keyboard are the DAW's
business, so the plugin has none of them.

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
- `s` stops or starts the synth, as **Stop** does in the editor.
- `r` starts or stops recording.
- `q` or `Esc` quits.

### Knobs and CV

Every knob has a matching `cv` input: patch any signal into it to modulate
the knob. The signal **adds** to the knob's position, where 1 is the knob's
whole travel, and the sum stops at the ends of the range. Neither multiplies
or overrides the other, so the knob sets where the signal starts from:

- Knob at its minimum, a 0..1 signal (a MIDI knob) covers the whole range. At
  the middle, the same signal only covers the upper half.
- Knob at its middle, a ±1 signal (an LFO) swings both ways but overshoots
  both ends, and flattens there: go through `atten` at gain 0.5 to fit it
  exactly, at a smaller gain to make it subtle, at a negative one to invert it.

In the editor, a blue tick on the slider shows the value the knob actually
takes, signal and knob together; it follows the signal as it moves. A
`clock` input overrides `time` or `rate` beyond what the tick shows: the
block's light shows that pace.

Right-clicking a slider in the editor returns it to its minimum, its middle
or its default. Signal inputs follow their own rules: `fm` is scaled by the
`fm depth` knob, and `mod` adds to the VCA's gain.

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
status bar names the ones found. In the editor, **Audio › Reconnect MIDI
controllers** picks up those plugged in later. `just run keys` is a ready-made patch:
keys play a filtered saw, knobs 1 to 4 sweep the cutoff, resonance, delay
feedback and self-FM, and pad 1 fires a noise burst.

A `midi` module outputs:

- `pitch`, `gate`, `velocity`: the keyboard, one note at a time; releasing a
  key falls back to the one still held. Patched into an oscillator's
  `freq cv`, `pitch` plays in tune, the `freq` knob setting the pitch of
  middle C. Pitch bend adds up to two semitones either way.
- `pad 1` to `pad 8`: a gate per pad, from notes on channel 10, the usual
  channel for pads, or from notes below 40 on any channel. Further banks land
  on the same eight outputs.
- `knob 1` to `knob 8`: controllers 1 to 8, from 0 to 1, smoothed.

`cargo run -- --midi-monitor` prints what a controller actually sends. On the
Akai MPK mini mk2, which sends everything on channel 1:

- Pad bank B (notes 32 to 39) gives `pad 1` to `pad 8`: 36 to 39 are pads 1 to
  4, 32 to 35 are pads 5 to 8.
- Pad bank A (notes 44 to 51) sends the same messages as the keys, so it plays
  the keyboard.
- The joystick's vertical axis sends controller 1, like the first knob, so it
  moves `knob 1` too. Its horizontal axis is the pitch bend.
- Shifting the keys two octaves down or more makes the lowest keys act as pads.

On Linux, MIDI needs the ALSA sequencer: if the status bar says "no MIDI",
load it with `sudo modprobe snd-seq`.

### Chords

A `poly` module plays up to four notes at once: `pitch`, `gate` and
`velocity` for each of four voices. Patch one voice per chain, an `osc` for
the pitch, an `adsr` envelope for the gate, a `vca` with `gain` at 0 opened
by the envelope on its `mod`, then a `mix` to bring the voices together.
`just run chords` is that patch, through a filter and a chorus.

Each new note takes a free voice, the one released longest ago so the
others' release tails ring on, or else steals the oldest note, which then
glides to the new pitch without a new attack. The pitch bend moves every
voice. A MIDI controller and the computer keyboard share the four voices.

### Computer keyboard

In the editor, the two bottom letter rows play one octave from middle C, as
`midi` and `poly` modules' `pitch`, `gate` and `velocity` (always 100),
alongside any MIDI controller: white keys on the lower row, black keys on the row above.

```
QWERTY:   S D   G H J         AZERTY:   S D   G H J
         Z X C V B N M                 W X C V B N ,
```

Keys are read by position, so the layout does not matter. They play nothing
with Ctrl or Alt held, or while typing a value into a slider. The terminal UI
has no such keyboard: most terminals do not report key releases.

### Sampler

A `sampler` plays a WAV file; click the button on its block to pick one. It
restarts on each rising edge of `trigger`. `start` and `length` choose a slice,
as fractions of the file. Past halfway, `loop` repeats that slice for as long as
it stays there, trigger or not. `speed` is the playback rate; its `speed cv`
takes a `midi` module's `pitch` in tune, the knob setting the speed of middle C.

Some starting points: a pad's gate into `trigger` to fire a drum sound, keys on
`pitch` and `gate` to play one chromatically, an LFO or noise through `s&h` into
`start cv` with a short looping `length` to stutter.

### Audio input and looper

An `input` block brings in the sound card's first two inputs: on a two-input
interface, the microphone on `1` and the instrument on `2`. Its light shows
the level. The input is opened at startup if there is one, or picked in the
editor's **Audio** menu; the status bar says why when it is not, for instance
when it cannot run at the output's rate.

A `looper` records its `in` for as long as its `record` gate is held, up to
30 s, then plays the recording like a `sampler`: `start`, `length`, `speed`,
and `loop`, on by default, or once per `trigger` when turned down. A new
recording replaces the last one, and recordings are not saved with the patch.
`just run looper` records input 1 while a key or a pad is held, and loops it
through a filter and a delay.

- **Latency.** The sound reaches the speakers about two callbacks late, one
  for each stream. Through the system's mixer, callbacks are often 1024
  frames (about 21 ms at 48 kHz each), so the round trip is near 45 ms: fine
  for looping, noticeable when playing through the synth. Turn on
  [Low latency](#low-latency) to hear yourself through it, or use the
  interface's direct monitor.
- **Feedback.** A microphone near the speakers, plus delay feedback, howls.
  Use headphones, or keep the microphone away and the volume down.

### Low latency

By default the synth plays through the system's mixer, alongside other
programs, which costs time: on Linux, ALSA's shared devices only take
callbacks of 1024 frames. **Low latency**, in the editor's **Audio** menu or
with `--low-latency`, plays on the card directly instead. **Buffer** in the
same menu, or `--buffer`, sets the callback size: smaller answers sooner,
until the audio error count in the header starts rising and the sound breaks
up. The status bar says which card plays, and how long a callback lasts.

- **Linux.** The card's own hardware device, with real-time priority for the
  audio thread, given by RealtimeKit (so no special rights needed). 256
  frames (5.3 ms at 48 kHz) unless another size is picked: it plays cleanly
  on a laptop's built-in card, where 128 breaks up; USB interfaces often take
  128 or 64. The card then plays for the synth alone: other programs cannot
  use it meanwhile, and the synth cannot take it while they play on it
  ("device is temporarily busy"). Stop them, or run the synth with
  `pasuspender -- audiochaos --low-latency` under PulseAudio. With no card
  picked, the first one that opens plays; the input is the same card's.
- **Windows.** Through the card's ASIO driver (for the UMC202HD, Behringer's
  own driver), in a build with ASIO, which is off by default: see
  [Build](#build-run-test). The buffer size is the driver's, set in its
  control panel, unless another is picked. Input and output must be the same
  driver; with no input picked, it is.

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
determines the sound. A sampler's sound is a `"file"` entry next to its `"type"`,
a path that is relative to the directory the program runs in unless absolute
(the editor saves absolute ones), so moving a sound breaks the patches using it:
the patch still loads, that sampler stays silent, its button turns red and the
path is kept when saving. Version 1 files, from before modules could be added
and removed, still load: they get the old fixed rack. The built-in presets
live in `presets/` in the same format and are compiled into the binary. `just run path/to/patch.json` starts on a patch
file; an unknown module, knob or port is reported by name. Block positions
from the graphical editor are saved under `positions`; the terminal UI
ignores them.

### Modules

Module types, as patches and the editor's palette name them:

| Type | Does |
|---|---|
| input | the sound card's first two inputs: a microphone, an instrument |
| midi | a MIDI controller's keyboard, pads and knobs as voltages |
| poly | the keyboard as four voices, for chords |
| clock | a tempo, tapped on its `tap` input or Tap button, as gates: bar, beat, 8th, dotted 8th, 16th |
| seq | 8-step CV sequencer with its own clock, `cv` and `gate` outputs, started and stopped by `run` |
| lfo | slow sine, triangle, saw and square |
| adsr | envelope from a gate: attack, decay, sustain, release |
| noise | white noise |
| sampler | plays a slice of a WAV file, once or looping, at any speed |
| looper | records its input while `record` is held, then plays it like `sampler` |
| s&h | sample & hold: grabs `in` on each rising edge of `trigger` |
| atten | three attenuverters: each output is its input times -1..1 |
| invert | `-in` mirrors a ±1 signal, `1-in` flips a 0..1 one: a gate's NOT |
| logic | `and`, `or`, `xor` of two signals and `not` of the first, high above 0 |
| osc | sine, saw, square, with through-zero linear FM |
| filter | resonant state-variable filter: lowpass, bandpass, highpass |
| dist | drive into a blend of `tanh` clipping and sine wavefolding |
| crush | bitcrusher: bit depth and sample-rate reduction |
| delay | up to 2 s, feedback above 1 self-oscillates instead of exploding |
| chorus | three voices swaying around 20 ms, like several players together |
| flanger | a very short swept delay, whose feedback sharpens the comb and can sing |
| phaser | four all-pass stages swept together, two moving notches |
| univibe | staggered stages swept by a lagging lamp: a lopsided throb |
| vca | `out = in * (gain + mod)`, ring modulator when `mod` is audio |
| mix | four inputs summed, each at its own level |
| master | volume and soft limiter, the only thing the speakers hear |

Some starting points for noise: patch an oscillator into its own `fm`, feed
the delay back into an oscillator's `fm`, push filter resonance to the max,
patch noise through `s&h` into a `freq cv`.

The modulation effects each have their own LFO. To drive the flanger or the
phaser from elsewhere, turn `depth` down and patch a signal into `time cv` or
`freq cv`. Their `feedback` self-oscillates through `tanh` above 1 and sounds
hollow below 0. `mix` at 1 makes the univibe a pure vibrato. Other classics
are already a patch away:

- Tremolo: `lfo` into a VCA's `mod`.
- Ring modulation: `osc` into a VCA's `mod`.
- Auto-wah: `lfo` into the `cutoff cv` of a `filter`, on its `bandpass`.

`invert` and `logic` reshape control signals. `1-in` turns a MIDI knob or an
envelope the other way round, so one knob opens a filter while closing
another; on a gate it is a NOT. `logic` counts anything above 0 as high, so
it takes gates, clock outputs, a sequencer's `cv` or an LFO alike: the
`and` of a clock's `8th` and a sequencer's `gate` keeps only the eighths
where a step plays, the `not` of the clock's `beat` rises on each offbeat,
the `xor` of two clock outputs syncopates, and the `not` of a pad's gate in
a `seq`'s `run cv` plays it until the pad is held.

### Tempo

`delay`, `lfo` and `seq` have a `clock` input. With a cable in it, they follow
the clock and ignore their `time` or `rate` knob, CV included:

- `delay`: the time between the last two ticks is the delay time, up to 2 s.
  Until two ticks have come, the knob still sets it. A change of tempo glides.
- `lfo`: one cycle per tick, restarting on each, so it stays in time.
- `seq`: each tick moves to the next step, starting from the first, and `gate`
  follows the clock.

`seq` also has a `run` knob: past halfway it plays, below it stops, holding
its step with the gate closed, and each start starts over from the first
step. With `run` at 0, a signal in `run cv` starts and stops it, which
sequences sequencers: a slow `seq` whose steps are 1 or 0 decides which
bars a faster one plays. Clocked, a sequencer started on a tick plays its
first step on that tick, and one started between ticks waits for the next.

A tick is a rise above 0, so any gate or square clocks them. A `clock` block
gives one tempo to all of them: for echoes on the dotted eighth over a
sequence in eighths, patch its `dotted 8th` into the delay's `clock` and its
`8th` into the sequencer's.

In the DAW plugin, the song's tempo replaces the `tempo` knob and the
tapped tempo, and while the song plays, the clock jumps to where the song
is at every block (see [DAW plugin](#daw-plugin)).

To tap the tempo, patch a MIDI pad into the clock's `tap` input and hit it
in rhythm. The first hit starts a bar, the next ones set the tempo and pull
the nearest beat onto each hit; the last five are averaged, and a pause of
three seconds starts a new tempo. While `tap` has a cable, the tapped tempo
replaces the `tempo` knob, and unplugging it hands the tempo back to the
knob. Without a pad, click the **Tap** button in rhythm: it sets the knob,
and is greyed out while `tap` has a cable. A pad straight into a delay's
`clock` works too: the delay takes the time between the last two hits.

## How it works

The engine, its threads, and the reasons behind its design are in
[ARCHITECTURE.md](ARCHITECTURE.md); how to work on it, add a module or a
preset is in [CONTRIBUTING.md](CONTRIBUTING.md). Changes from one release to
the next are in [CHANGELOG.md](CHANGELOG.md).

## Build, run, test

Requires a recent stable Rust toolchain and, on Linux, the ALSA and D-Bus
development headers (`alsa-lib` and `dbus` on Arch, `libasound2-dev` and
`libdbus-1-dev` on Debian/Ubuntu); D-Bus asks RealtimeKit for the audio
thread's priority. It also runs on Windows, MIDI controllers included, with
nothing else to install.

[Low latency](#low-latency) on Windows needs ASIO, which takes
[LLVM](https://releases.llvm.org/) installed (`winget install LLVM.LLVM`)
and this build, which downloads Steinberg's ASIO SDK the first time:

```sh
cargo build --release --features asio
```

```sh
just build
just run          # chaos preset
just run bass     # bass preset
just run my.json  # a patch file
just run --render out.wav --seconds 60   # render to WAV, no sound card
just run --buffer 128 looper             # smaller sound card buffers
just run --low-latency looper            # the sound card without the mixer
just run --midi-monitor                  # what the MIDI controllers send
just tui          # terminal UI instead of the editor
just bundle         # VST3 and CLAP plugins into target/bundled/
just install-plugin # the same, copied to ~/.vst3 and ~/.clap
just test
just lint   # cargo fmt --check + clippy, whole workspace
just fmt
```

[`just`](https://github.com/casey/just) is optional; the recipes are thin
wrappers around the usual `cargo` commands in the `justfile`.

The plugin's bundles come from `cargo xtask bundle`, nice-plug's bundler,
since a plugin is a folder layout around the library that cargo alone does
not produce.

Dev builds use `opt-level = 1`: unoptimized DSP made the stability tests
slow and risked audio dropouts.

A USB controller plugged in at boot can become ALSA's card 0, the system
default, although it has no audio output. The first sound card that opens then
takes over, after a few harmless `ALSA lib` lines on the terminal. On Linux,
the **Audio** menu lists each card's own default device, which shares the card
with other programs; ALSA's other entries for the same card are exclusive
(what low latency opens), surround layouts or HDMI.

The header shows an audio error count. One error at startup is normal with
the ALSA backend; a count that keeps rising means buffer underruns.

## License

MIT, see [LICENSE](LICENSE): do what you like with it. Most of the code was
written with an AI assistant (Claude), and it comes as is, with no warranty of
any kind.
