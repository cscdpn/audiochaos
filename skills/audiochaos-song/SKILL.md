---
name: audiochaos-song
description: Write a complete piece of music (sections, drums, bass, melody) as an audiochaos patch, a JSON file the audiochaos modular synth plays. Use it when asked to compose, generate or arrange a song or track for audiochaos.
---

# Writing a song for audiochaos

audiochaos is a modular synthesizer: a patch is a set of modules (oscillators,
envelopes, sequencers...) joined by cables. You write a song as one patch
file in JSON. A `song` module lists the sections and which instrument plays
in each; `notes` modules play melodies written as note names.

This folder holds everything you need:

- `SKILL.md`: this file, the method.
- `reference.md`: every module, with its exact input, output and knob names.
  Only use names from there.
- `example.json`: a complete song in four sections. Start from it.

## The file format

```json
{
  "version": 2,
  "description": "One sentence about the song.",
  "modules": [
    { "name": "song", "type": "song",
      "sections": [{ "name": "verse", "bars": 8 }, { "name": "chorus", "bars": 8 }],
      "tracks": ["A A", ". A"] },
    { "name": "bass", "type": "notes", "key": "A minor",
      "patterns": { "A": "A1 . A1 A2 . A1 E2 D2" } },
    { "name": "master", "type": "master" }
  ],
  "knobs": {
    "bass env": { "decay": 0.15, "sustain": 0.3 }
  },
  "cables": [
    { "from": "song", "output": "track 1", "to": "bass", "input": "play" }
  ]
}
```

Rules:

- `"version": 2` always.
- `modules`: each has a unique `name` you choose and a `type` from
  `reference.md`. Exactly one module of type `master`: what reaches its `in`
  is what you hear.
- Only a `song` has `sections` and `tracks`; only a `notes` module has `key`
  and `patterns`.
- `knobs`: module name, then knob name, then a number. Knobs you leave out
  keep their default. Stay within each knob's range.
- `cables`: from a module's output to a module's input, by name. Each knob
  also has an input named after it plus ` cv` (`freq cv`, `run cv`).
  Several cables into one input add up.
- Modules are computed in the order listed: list the `clock` and the `song`
  first, then sequencers and `notes`, envelopes, sound sources, filters,
  VCAs, effects, the `mix`, and the `master` last.
- Plain JSON: no comments, no trailing commas, numbers with a dot (`0.0`).
- At most 256 modules and 1024 cables.

## Signals in one paragraph

Audio and control signals are numbers. Sound goes from -1 to 1. A gate is 0
or 1: 1 while a note plays. A cable into a knob's `cv` input moves the knob.
A `notes` module's `pitch` output plays an oscillator in tune when the
oscillator's `freq` knob is 261.63 (middle C): always set it so.

## The method

Work in this order, and write the plan down before the JSON.

1. **Tempo and key.** A tempo (80 to 140 BPM) and a key, such as A minor.
2. **Sections.** Up to 16, each a name and a number of bars: intro, verse,
   chorus, break, outro...
3. **Instruments.** Pick from the templates below: kick, snare, hats, bass,
   lead. Two to six instruments is plenty, at most 8.
4. **Who plays where.** One track of the song per instrument: a string with
   one cell per section, `A` where it plays pattern A, `B` for pattern B,
   `.` where it is silent.
5. **Melodies.** For each `notes` module, pattern A (and B, C, D if the
   tracks use them): one note name per step, from the key (see "Notes").
6. **Mix.** Instruments into a `mix` (4 inputs, `level 1` to `level 4`),
   the mix into the `master`. Kick loudest, hats and lead quieter. For more
   than four instruments, add a second `mix` and cable it into the
   `master` `in` too.
7. **Write the JSON**, then check it against "Before you answer".

## The skeleton

Every song has these two modules and this cable:

- `clock` (type `clock`): knob `tempo`. Its outputs tick in time: `bar`,
  `beat`, `8th`, `dotted 8th`, `16th`.
- `song` (type `song`): `clock` output `bar` into `song` input `clock`.
  Its `sections` play in order, each for its `bars`. Output `track N` plays
  the Nth string of `tracks`: it is 0 where the cell is `.`, 1 for `A`, 2
  for `B`, 3 for `C`, 4 for `D`. Keep the knob `loop` at 0 so the song ends.

## Melodies: `notes`

A `notes` module plays a pattern one step per tick of its `clock` input.
Its `play` input takes a song track: it plays the pattern the cell names,
from its first step when the section starts, and is silent on `.`.

- `key`: a root and a scale, as `"A minor"`. It helps you pick notes.
- `patterns`: `"A"` to `"D"`, each a string of steps separated by spaces:
  a note name (`A2`, `C#3`, `Bb4`) starts a note, `-` holds the note before
  one more step, `.` is a rest. Up to 64 steps.
- Outputs: `pitch` into an oscillator's `freq cv` (oscillator `freq` at
  261.63), `gate` into an envelope's `gate`.

Which `clock` output you choose sets the speed:

| clock output | 8 steps last | 16 steps last |
|---|---|---|
| `16th` | half a bar | one bar |
| `8th` | one bar | two bars |
| `beat` | two bars | four bars |

## Drums: `seq`

Drums need rhythm, not pitch: use a `seq` (8 steps). Set its knob `run` to
0.0 and cable its song track into its `run cv`: it plays in the sections
where the cell is not `.`. With a `clock` output in its `clock` input, its
`gate` opens on every tick.

For rests, set each step to 1.0 (hit) or 0.0 (rest), and pass the gate
through a `logic` module: `seq` output `cv` into `logic` input `a`, `seq`
output `gate` into `logic` input `b`, and `logic` output `and` into the
envelope's `gate`.

## Instrument templates

Replace `X` with the instrument's name. Every instrument is a sequencer,
an envelope (`adsr`), a sound, and a VCA with `gain` at 0.0 opened by the
envelope on its `mod` input. `T` is its song track, as `track 1`.

**Kick**, on `beat`:
- modules: `X seq` (seq), `X env` (adsr), `X osc` (osc), `X vca` (vca)
- knobs: `X seq` run 0.0; `X env` attack 0.001, decay 0.2, sustain 0.0,
  release 0.1; `X osc` freq 45.0, fm depth 120.0; `X vca` gain 0.0
- cables: song `T` → `X seq` `run cv`; clock `beat` → `X seq` `clock`;
  `X seq` `gate` → `X env` `gate`; `X env` `env` → `X osc` `fm`;
  `X env` `env` → `X vca` `mod`; `X osc` `sine` → `X vca` `in`

**Snare**, on `beat`, hits on steps 2, 4, 6, 8:
- modules: `X seq` (seq), `X hits` (logic), `X env` (adsr), `noise`
  (noise, one is enough for all drums), `X filter` (filter), `X vca` (vca)
- knobs: `X seq` run 0.0, steps 1 to 8: 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
  1.0; `X env` attack 0.001, decay 0.12, sustain 0.0, release 0.08;
  `X filter` cutoff 1800.0, resonance 0.3; `X vca` gain 0.0
- cables: song `T` → `X seq` `run cv`; clock `beat` → `X seq` `clock`;
  `X seq` `cv` → `X hits` `a`; `X seq` `gate` → `X hits` `b`;
  `X hits` `and` → `X env` `gate`; `noise` `white` → `X filter` `in`;
  `X filter` `bandpass` → `X vca` `in`; `X env` `env` → `X vca` `mod`

**Hats**, on `16th` or `8th`: like the snare without the `logic` (every
step hits): `X env` decay 0.04, sustain 0.0, release 0.03; `X filter`
cutoff 7000.0 and its `highpass` output into the VCA.

**Bass**, on `8th`, notes in octaves 1 and 2:
- modules: `X` (notes), `X env` (adsr), `X osc` (osc), `X filter`
  (filter), `X vca` (vca)
- `X`: `key` and `patterns`; knobs: `X env` attack 0.003, decay 0.15,
  sustain 0.3, release 0.05; `X osc` freq 261.63; `X filter` cutoff 500.0,
  resonance 0.6; `X vca` gain 0.0
- cables: song `T` → `X` `play`; clock `8th` → `X` `clock`;
  `X` `gate` → `X env` `gate`; `X` `pitch` → `X osc` `freq cv`;
  `X osc` `saw` → `X filter` `in`; `X filter` `lowpass` → `X vca` `in`;
  `X env` `env` → `X vca` `mod`

**Lead**, on `8th` or `16th`, notes in octaves 3 and 4:
- like the bass without the filter: `X osc` freq 261.63, its `square` (or
  `saw`) output straight into `X vca` `in`; `X env` attack 0.005, decay 0.2,
  sustain 0.4, release 0.15
- echoes: add `X delay` (delay), knobs feedback 0.45, mix 0.35; cables
  `X vca` `out` → `X delay` `in`, clock `dotted 8th` → `X delay` `clock`;
  then `X delay` `out` goes to the mix instead of `X vca` `out`

**Pad**, long notes on `beat`, octave 3: like the lead with `X env` attack
0.3, decay 0.5, sustain 0.7, release 1.0, `saw` output, and steps held
with `-`: `"A3 - - - F3 - - -"`.

More flavours, all in `reference.md`: `dist` (grit, after a VCA), `crush`,
`chorus`, `phaser` on a lead or bass; an `lfo` into a filter's `cutoff cv`
for movement.

## Notes

A note is a letter, an optional `#` (sharp) or `b` (flat), and an octave:
`C4` is middle C, `A4` the A above it, `A3` an octave lower. Octaves go up
from C: `B3` is just below `C4`.

The notes of some keys:

- C major: C D E F G A B
- G major: G A B C D E F#
- F major: F G A Bb C D E
- A minor: A B C D E F G
- E minor: E F# G A B C D
- D minor: D E F G A Bb C

Any key works: `key` takes a root and one of these scales: major, minor,
harmonic minor, melodic minor, major pentatonic, minor pentatonic, blues,
dorian, phrygian, lydian, mixolydian, locrian, harmonic major, phrygian
dominant, hungarian minor, double harmonic, diminished, half-whole
diminished, augmented, whole tone, hirajoshi, in, chromatic.

Repeating the root and the fifth (A and E in A minor) makes a solid
bassline; a lead can climb and fall through the scale, and rest with `.`
or hold with `-` to breathe. A pattern B for the chorus, a little higher or
busier than A, lifts the song.

## Before you answer

Check each point:

- Every name in `cables` and `knobs` is the `name` of a module in
  `modules`.
- Every output, input and knob name is spelled as in `reference.md`; CV
  inputs end in ` cv`.
- One `master`, and every instrument reaches it, through the `mix`.
- `clock` `bar` goes into `song` `clock`.
- Each string in `tracks` has one cell per section, and each `notes`
  module has a pattern for every letter its track uses.
- Every instrument gets its track: into `play` for `notes`, into `run cv`
  (with `run` at 0.0) for `seq`.
- Every oscillator played by `notes` has `freq` at 261.63.
- Every VCA opened by an envelope has `gain` at 0.0.

## Playing it

Save the file, say `my-song.json`, then:

- `audiochaos my-song.json` plays it and opens it in the editor;
- `audiochaos --render my-song.wav --seconds 60 my-song.json` writes it to a
  sound file: give enough seconds for every section (one bar lasts
  240 / tempo seconds).

A mistake stops the patch from loading and names it, for example
`module "bass seq" has no port "cv in"`: fix that name and try again.
