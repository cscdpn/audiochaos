# Module reference

Generated from the code by `tests/skill.rs`: do not edit by hand.

Every module type a song can use, with its exact port and knob names.
Each knob also has a CV input named after it plus ` cv`: `freq` has
`freq cv`. Several cables into one input add up.

## clock

A tempo as gates, to keep delays, LFOs and sequencers in time. Tap it on `tap` or with the Tap button. In a DAW, it follows the song's tempo, and its bars while the song plays.

- inputs: `tap`
- outputs: `bar` (gate, 4 beats), `beat` (gate), `8th` (gate, ½ beat), `dotted 8th` (gate, ¾ beat), `16th` (gate, ¼ beat)
- knobs:
  - `tempo`: 20..300 BPM, default 120. Beats per minute. Ignored while `tap` has a cable, and in a DAW, which sets the tempo.

## seq

Eight-step CV sequencer, on its own clock or stepped by `clock`. `run` starts and stops it: patch another seq's `cv` into `run cv` to sequence sequencers.

- inputs: `clock`
- outputs: `cv` (step, ±1), `gate` (0 or 1)
- knobs:
  - `rate`: 0.1..50 st/s, default 4. Steps per second. Ignored while `clock` has a cable.
  - `step 1`: -1..1, default 0. This step's CV, ±1.
  - `step 2`: -1..1, default 0. This step's CV, ±1.
  - `step 3`: -1..1, default 0. This step's CV, ±1.
  - `step 4`: -1..1, default 0. This step's CV, ±1.
  - `step 5`: -1..1, default 0. This step's CV, ±1.
  - `step 6`: -1..1, default 0. This step's CV, ±1.
  - `step 7`: -1..1, default 0. This step's CV, ±1.
  - `step 8`: -1..1, default 0. This step's CV, ±1.
  - `run`: 0..1, default 1. Plays past halfway, stops below. Starting again starts from the first step. With the knob at 0, a gate or another seq's `cv` in `run cv` starts and stops it.

## notes

Melodies by note, in patterns A to D drawn on a grid: a step per tick on `clock`. `play` picks the pattern, as a song's track gives it: 0 stops, 1 plays A, 2 B... Unplugged, it plays A. `pitch` into an oscillator's `freq cv`, with `freq` on middle C (261.63 Hz).

- inputs: `clock`, `play`
- outputs: `pitch` (note, for freq cv), `gate` (0 or 1)
- knobs:
  - `rate`: 0.1..50 st/s, default 4. Steps per second. Ignored while `clock` has a cable.

## song

Arranges a whole piece: sections played one after the other, each so many bars long on `clock` (a clock's `bar`), and in each, which tracks play. A track's output is 1 while it plays pattern A, 2 for B..., 0 when silent: into a `notes` module's `play`, a seq's `run cv` or a VCA's `mod`.

- inputs: `clock`
- outputs: `track 1` (0, or pattern 1..4), `track 2` (0, or pattern 1..4), `track 3` (0, or pattern 1..4), `track 4` (0, or pattern 1..4), `track 5` (0, or pattern 1..4), `track 6` (0, or pattern 1..4), `track 7` (0, or pattern 1..4), `track 8` (0, or pattern 1..4)
- knobs:
  - `loop`: 0..1, default 0. Past halfway, the song starts over after its last section. Below, it ends there and every track falls silent.

## lfo

Slow sine, triangle, saw and square, to move other knobs. Follows `clock` when it has a cable.

- inputs: `clock`
- outputs: `sine` (±1), `triangle` (±1), `saw` (±1), `square` (±1)
- knobs:
  - `rate`: 0.01..50 Hz, default 1. Cycles per second. Ignored while `clock` has a cable.

## adsr

Envelope: shapes a note from a gate, rising, falling to a level held with the key, then fading. Into a VCA's mod, or a cutoff.

- inputs: `gate`
- outputs: `env` (0..1)
- knobs:
  - `attack`: 0.001..10 s, default 0.005. Time to rise to full when the gate opens.
  - `decay`: 0.001..10 s, default 0.2. Time to fall from full to the sustain level.
  - `sustain`: 0..1, default 0.7. Level held for as long as the gate stays open.
  - `release`: 0.001..10 s, default 0.3. Time to fade out once the gate closes.

## noise

White noise: hiss for percussion, or random steps through `s&h`.

- inputs: none
- outputs: `white` (±1)

## s&h

Sample & hold: grabs `in` on each rising edge of `trigger` and holds it. Noise in, random steps out.

- inputs: `in`, `trigger`
- outputs: `out` (held input)

## atten

Three attenuverters: scale a modulation down, or invert it.

- inputs: `a`, `b`, `c`
- outputs: `a` (a × gain a), `b` (b × gain b), `c` (c × gain c)
- knobs:
  - `gain a`: -1..1, default 1. How much of the input comes out; negative inverts it.
  - `gain b`: -1..1, default 1. How much of the input comes out; negative inverts it.
  - `gain c`: -1..1, default 1. How much of the input comes out; negative inverts it.

## invert

Turns a signal upside down: `-in` mirrors it around 0, for ±1 signals like an LFO; `1-in` flips it within 0..1, for gates, envelopes and MIDI knobs, so a gate becomes its NOT.

- inputs: `in`
- outputs: `-in` (in × -1), `1-in` (1 - in)

## logic

Logic gates on two signals, each high above 0: `and` while both are, `or` while either is, `xor` while one only is, `not` while `a` is not. Combine gates and sequencer steps into new rhythms.

- inputs: `a`, `b`
- outputs: `and` (0 or 1), `or` (0 or 1), `xor` (0 or 1), `not` (0 or 1, not a)

## osc

Oscillator: sine, saw and square, with linear FM for metallic sounds.

- inputs: `fm`
- outputs: `sine` (±1), `saw` (±1), `square` (±1)
- knobs:
  - `freq`: 1..10000 Hz, default 110. Pitch. With a MIDI `pitch` in `freq cv`, the pitch of middle C.
  - `fm depth`: 1..10000 Hz, default 100. How far `fm` pushes the frequency; past the pitch itself it goes through zero and turns metallic.

## filter

Resonant filter, with lowpass, bandpass and highpass outputs.

- inputs: `in`
- outputs: `lowpass` (lows), `bandpass` (mids), `highpass` (highs)
- knobs:
  - `cutoff`: 20..18000 Hz, default 1000. Where the filter starts cutting.
  - `resonance`: 0..1, default 0.2. Boost around the cutoff: near the top it whistles.

## dist

Drive into clipping or wavefolding, from warm to wrecked.

- inputs: `in`
- outputs: `out` (shaped input)
- knobs:
  - `drive`: 1..100 x, default 4. Gain before shaping: how hard the wave hits the shaper.
  - `fold`: 0..1, default 0. 0 clips (squarer), 1 folds the wave back on itself (wilder).

## crush

Bitcrusher: fewer bits and a lower sample rate, for digital grit.

- inputs: `in`
- outputs: `out` (crushed input)
- knobs:
  - `bits`: 1..16 bit, default 16. Amplitude resolution: fewer bits, coarser steps.
  - `downsample`: 1..64 x, default 1. Holds each sample this many times: a lower sample rate, metallic aliasing.

## delay

Echoes, up to 2 s. Feedback above 1 screams instead of exploding.

- inputs: `in`, `clock`
- outputs: `out` (input + echoes)
- knobs:
  - `time`: 0.001..2 s, default 0.25. Time between echoes. Ignored while `clock` has a cable.
  - `feedback`: 0..1.5, default 0.5. How much of each echo comes back: above 1 it self-oscillates.
  - `mix`: 0..1, default 0.5. 0 dry, 1 only the echoes.

## chorus

Copies of the input, slightly delayed and swaying: a thicker, wider sound.

- inputs: `in`
- outputs: `out` (input + voices)
- knobs:
  - `rate`: 0.05..10 Hz, default 0.8. How fast the voices sway.
  - `depth`: 0..1, default 0.5. How far the voices drift apart: lush, then seasick.
  - `mix`: 0..1, default 0.5. 0 dry, 1 only the voices.

## flanger

A very short swept delay: a jet-like comb sweeping through the sound.

- inputs: `in`
- outputs: `out` (input + sweep)
- knobs:
  - `rate`: 0.01..20 Hz, default 0.3. How fast the comb sweeps.
  - `depth`: 0..1, default 0.7. How far it sweeps around `time`.
  - `time`: 0.0002..0.02 s, default 0.003. Base delay: shorter moves the comb up.
  - `feedback`: -1.5..1.5, default 0.5. Sharpens the comb: above 1 it sings, below 0 it sounds hollow.
  - `mix`: 0..1, default 0.5. 0 dry, 1 only the delayed signal; 0.5 makes the deepest notches.

## phaser

All-pass stages swept together: two notches moving through the sound.

- inputs: `in`
- outputs: `out` (input + sweep)
- knobs:
  - `rate`: 0.01..20 Hz, default 0.5. How fast the notches sweep.
  - `depth`: 0..1, default 0.7. How far the notches sweep around `freq`.
  - `freq`: 100..5000 Hz, default 800. Where the notches sit at rest.
  - `feedback`: -1.5..1.5, default 0.4. Deepens the notches and raises the peaks: above 1 it sings.
  - `mix`: 0..1, default 0.5. 0 dry, 1 only the shifted signal; 0.5 makes the deepest notches.

## univibe

A phaser driven by a lagging lamp: the lopsided throb of the 60s pedal.

- inputs: `in`
- outputs: `out` (input + throb)
- knobs:
  - `speed`: 0.1..15 Hz, default 3. How fast the lamp pulses.
  - `intensity`: 0..1, default 0.7. How far the lamp sweeps the stages.
  - `mix`: 0..1, default 0.5. 0 dry, 1 pure vibrato.

## vca

Amplifier, out = in × (gain + mod): notes from an envelope, tremolo from an LFO, ring modulation from audio.

- inputs: `in`, `mod`
- outputs: `out` (input × gain)
- knobs:
  - `gain`: 0..2, default 1. Gain with nothing in `mod`; at 0, `mod` alone opens it.

## mix

Four inputs summed, each at its own level: voices into one sound, or a crossfade with CV.

- inputs: `1`, `2`, `3`, `4`
- outputs: `out` (sum of inputs × levels)
- knobs:
  - `level 1`: 0..1, default 1. How much of this input goes into the mix.
  - `level 2`: 0..1, default 1. How much of this input goes into the mix.
  - `level 3`: 0..1, default 1. How much of this input goes into the mix.
  - `level 4`: 0..1, default 1. How much of this input goes into the mix.

## master

The output: volume and a soft limiter, the only thing the speakers hear.

- inputs: `in`
- outputs: `out` (to the speakers)
- knobs:
  - `volume`: 0..1, default 0.5. Output level, before the soft limiter.
