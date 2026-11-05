---
name: audiochaos-song
description: Write a complete piece of music (sections, drums, bass, melody) as an audiochaos patch, a JSON file the audiochaos modular synth plays. Use it when asked to compose, generate or arrange a song or track for audiochaos.
---

# Writing a song for audiochaos

audiochaos is a modular synthesizer: a patch is a set of modules (oscillators,
envelopes, sequencers...) joined by cables. You write a song as one patch
file in JSON. A `song` module counts bars and turns instruments on and off
section by section.

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
    { "name": "kick seq", "type": "seq" },
    { "name": "master", "type": "master" }
  ],
  "knobs": {
    "kick seq": { "run": 0.0, "step 1": 0.5 }
  },
  "cables": [
    { "from": "kick seq", "output": "gate", "to": "kick env", "input": "gate" }
  ]
}
```

Rules:

- `"version": 2` always.
- `modules`: each has a unique `name` you choose and a `type` from
  `reference.md`. Exactly one module of type `master`: what reaches its `in`
  is what you hear.
- `knobs`: module name, then knob name, then a number. Knobs you leave out
  keep their default. Stay within each knob's range.
- `cables`: from a module's output to a module's input, by name. Each knob
  also has an input named after it plus ` cv` (`freq cv`, `run cv`).
  Several cables into one input add up.
- Modules are computed in the order listed: list the `clock` and the `song`
  first, then sequencers, envelopes, sound sources, filters, VCAs, effects,
  the `mix`, and the `master` last.
- Plain JSON: no comments, no trailing commas, numbers with a dot (`0.0`).
- At most 256 modules and 1024 cables.

## Signals in one paragraph

Audio and control signals are numbers. Sound goes from -1 to 1. A gate is 0
or 1: 1 while a note or a section plays. A cable into a knob's `cv` input
moves the knob: a CV of 1 sweeps the whole knob range, so small CVs make
small moves. That is how sequencers play notes: their `cv` goes into an
oscillator's `freq cv`, and each step's value is a pitch from the table
below.

## The method

Work in this order, and write the plan down before the JSON.

1. **Tempo and key.** A tempo (80 to 140 BPM) and a root note: the
   oscillators' `freq` (see "Notes").
2. **Sections.** Up to 8, each a number of bars: intro, verse, chorus,
   break, outro... Set the `song` knobs `length 1` to `length 8`, with 0 for
   unused sections, and `loop` at 0 so the song ends.
3. **Instruments.** Pick from the templates below: kick, snare, hats, bass,
   lead. Two to five instruments is plenty.
4. **Who plays where.** A table of sections and instruments. For each
   section an instrument plays in, one cable from the song's `section N`
   output to that instrument's sequencer `run cv`.
5. **Melodies.** For each pitched sequencer, 8 notes in semitones from the
   root, converted with the table. Use a scale (see "Notes").
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
  Output `section N` is 1 while section N plays.

## How a sequencer plays

A `seq` has 8 steps. With a `clock` output in its `clock` input, it moves
one step per tick and its `gate` opens on every tick. Which output you
choose sets the speed:

| clock output | 8 steps last |
|---|---|
| `16th` | half a bar |
| `8th` | one bar |
| `beat` | two bars |

Set its knob `run` to 0.0 and cable the sections it plays in to its
`run cv`: it starts from step 1 when its section starts and is silent
outside its sections. Without these cables, keep `run` at its default and
it plays all the time.

The gate opens on every step. For rests, give the instrument a rhythm: set
each step to 1.0 (hit) or 0.0 (rest), and pass the gate through a `logic`
module: `seq` output `cv` into `logic` input `a`, `seq` output `gate` into
`logic` input `b`, and `logic` output `and` into the envelope's `gate`.
`example.json` does this for its snare.

## Instrument templates

Replace `X` with the instrument's name. Every instrument is a sequencer,
an envelope (`adsr`), a sound, and a VCA with `gain` at 0.0 opened by the
envelope on its `mod` input.

**Kick**, on `beat`:
- modules: `X seq` (seq), `X env` (adsr), `X osc` (osc), `X vca` (vca)
- knobs: `X seq` run 0.0; `X env` attack 0.001, decay 0.2, sustain 0.0,
  release 0.1; `X osc` freq 45.0, fm depth 120.0; `X vca` gain 0.0
- cables: clock `beat` → `X seq` `clock`; `X seq` `gate` → `X env` `gate`;
  `X env` `env` → `X osc` `fm`; `X env` `env` → `X vca` `mod`;
  `X osc` `sine` → `X vca` `in`

**Snare**, on `beat`, hits on steps 2, 4, 6, 8:
- modules: `X seq` (seq), `X hits` (logic), `X env` (adsr), `noise`
  (noise, one is enough for all drums), `X filter` (filter), `X vca` (vca)
- knobs: `X seq` run 0.0, steps 1 to 8: 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
  1.0; `X env` attack 0.001, decay 0.12, sustain 0.0, release 0.08;
  `X filter` cutoff 1800.0, resonance 0.3; `X vca` gain 0.0
- cables: clock `beat` → `X seq` `clock`; `X seq` `cv` → `X hits` `a`;
  `X seq` `gate` → `X hits` `b`; `X hits` `and` → `X env` `gate`;
  `noise` `white` → `X filter` `in`; `X filter` `bandpass` → `X vca` `in`;
  `X env` `env` → `X vca` `mod`

**Hats**, on `16th` or `8th`: like the snare without the `logic` (every
step hits): `X env` decay 0.04, sustain 0.0, release 0.03; `X filter`
cutoff 7000.0 and its `highpass` output into the VCA.

**Bass**, on `8th`:
- modules: `X seq` (seq), `X env` (adsr), `X osc` (osc), `X filter`
  (filter), `X vca` (vca)
- knobs: `X seq` run 0.0 and 8 notes; `X env` attack 0.003, decay 0.15,
  sustain 0.3, release 0.05; `X osc` freq 55.0; `X filter` cutoff 500.0,
  resonance 0.6; `X vca` gain 0.0
- cables: clock `8th` → `X seq` `clock`; `X seq` `gate` → `X env` `gate`;
  `X seq` `cv` → `X osc` `freq cv`; `X osc` `saw` → `X filter` `in`;
  `X filter` `lowpass` → `X vca` `in`; `X env` `env` → `X vca` `mod`

**Lead**, on `8th` or `beat`, two octaves above the bass:
- like the bass without the filter: `X osc` freq 220.0, its `square` (or
  `saw`) output straight into `X vca` `in`; `X env` attack 0.005, decay 0.2,
  sustain 0.2, release 0.15
- echoes: add `X delay` (delay), knobs feedback 0.45, mix 0.35; cables
  `X vca` `out` → `X delay` `in`, clock `dotted 8th` → `X delay` `clock`;
  then `X delay` `out` goes to the mix instead of `X vca` `out`

More flavours, all in `reference.md`: `dist` (grit, after a VCA), `crush`,
`chorus`, `phaser` on a lead or bass; an `lfo` into a filter's `cutoff cv`
for movement.

## Notes

An oscillator's `freq` is the root note. Roots: A1 55.0, C2 65.41, D2 73.42,
E2 82.41, G2 98.0, A2 110.0, C3 130.81, D3 146.83, E3 164.81, G3 196.0,
A3 220.0, C4 261.63.

A sequencer step is a pitch in semitones above (or below) the root, as this
CV:

| semitones | step value |
|---|---|
| -12 | -0.0753 |
| -7 | -0.0439 |
| -5 | -0.0314 |
| 0 | 0.0 |
| +1 | 0.0063 |
| +2 | 0.0125 |
| +3 | 0.0188 |
| +4 | 0.0251 |
| +5 | 0.0314 |
| +6 | 0.0376 |
| +7 | 0.0439 |
| +8 | 0.0502 |
| +9 | 0.0564 |
| +10 | 0.0627 |
| +11 | 0.069 |
| +12 | 0.0753 |
| +14 | 0.0878 |
| +15 | 0.0941 |
| +17 | 0.1066 |
| +19 | 0.1192 |
| +24 | 0.1505 |

Any other: semitones × 0.00627.

Scales, in semitones from the root (add 12 for the next octave):

- minor: 0, 2, 3, 5, 7, 8, 10
- major: 0, 2, 4, 5, 7, 9, 11
- minor pentatonic: 0, 3, 5, 7, 10
- major pentatonic: 0, 2, 4, 7, 9

Repeating the root and the fifth (7) makes a solid bassline; a lead can
climb and fall through the scale.

## Before you answer

Check each point:

- Every name in `cables` and `knobs` is the `name` of a module in
  `modules`.
- Every output, input and knob name is spelled as in `reference.md`; CV
  inputs end in ` cv`.
- One `master`, and every instrument reaches it, through the `mix`.
- `clock` `bar` goes into `song` `clock`.
- Every sequencer driven by sections has `run` at 0.0, and at least one
  `section N` cable into its `run cv`.
- Every VCA opened by an envelope has `gain` at 0.0.
- Every section with a length above 0 has at least one instrument.
- Knob values are within range: step values between -1 and 1, mix levels
  between 0 and 1.

## Playing it

Save the file, say `my-song.json`, then:

- `audiochaos my-song.json` plays it and opens it in the editor;
- `audiochaos --render my-song.wav --seconds 60 my-song.json` writes it to a
  sound file: give enough seconds for every section (one bar lasts
  240 / tempo seconds).

A mistake stops the patch from loading and names it, for example
`module "bass seq" has no port "cv in"`: fix that name and try again.
