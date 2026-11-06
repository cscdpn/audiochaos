# Changelog

Notable changes, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
[Semantic Versioning](https://semver.org/): until 1.0, a minor version may
break patch files, which then fail to load with a message rather than load
wrong.

## [0.1.0] - unreleased

The first release.

### Sound
- 29 modules: sources (`osc`, `noise`, `lfo`, `seq`, `notes`, `clock`,
  `adsr`, `sampler`, `looper`, `input`, `midi`, `poly`), processors
  (`filter`, `dist`, `crush`, `delay`, `chorus`, `flanger`, `phaser`,
  `univibe`, `vca`, `mix`, `pan`), control (`s&h`, `atten`, `invert`,
  `logic`), the `song` arranger and the `master`.
- Stereo: `pan` places a sound between the speakers, into the master's
  `left` and `right`; its `in` still plays on both.
- Melodies by note name with `notes`: four patterns of up to 64 steps,
  drawn on a piano roll in the key of your choice among 23 scales.
- Whole pieces in one patch with `song`: named sections, and eight tracks
  saying who plays which pattern in each, edited as a grid.
- Every knob has a CV input; cables may feed back, with one block (64
  samples) of delay.
- Eight presets: `chaos`, `bass`, `groovygroove`, `keys`, `chords`, `looper`,
  `song`, a whole short piece in sections, and `stereo`.
- Patches as JSON files, block positions included; version 1 files load.
  Up to 256 modules and 1024 cables in a patch.
- A skill in `skills/audiochaos-song` to have a language model, even a
  small local one, write a song patch without reading the code.

### Playing
- MIDI controllers: keys, pitch bend, pads and eight knobs, mapped on an
  Akai MPK mini mk2; four-voice chords with `poly`.
- The computer keyboard as a one-octave piano.
- Tap tempo, and clock inputs on `delay`, `lfo` and `seq`; sequencers can
  start and stop each other.
- Global Stop and Start, with a fade.
- The sound card's input, and a live looper.
- Live recording to stereo WAV, and offline rendering with `--render`.

### Editor
- Node editor: blocks, wires, sliders with tooltips, lights that blink at
  each module's pace, ticks where CV takes each knob.
- Duplicate, copy, paste and save groups of blocks; Arrange blocks.
- Audio menu: switch sound cards, reconnect MIDI controllers, low latency
  and buffer size, without restarting.
- A terminal UI, for SSH, with fewer features.

### DAW plugin
- VST3 and CLAP instrument with the same editor; the patch is saved with
  the project, host MIDI plays it, Knob 1-8 are automatable, and clocks
  follow the song's tempo and position.

### Platforms
- Linux and Windows. Low latency: ALSA hardware devices with a real-time
  audio thread on Linux; ASIO on Windows, in builds with `--features asio`.
