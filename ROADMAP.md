# Roadmap

What is agreed for coming releases, and how it was meant to work. Done
items move to the [CHANGELOG](CHANGELOG.md).

## 0.2

### `chords`: chord progressions

Today a chord takes one oscillator chain per voice, each tuned by hand, as
in the `chords` preset. A `chords` block would play a progression written
as chord symbols.

- In the block, a row of cells, one per bar or half bar, each a chord
  typed by name: `Am`, `F`, `Cmaj7`, `G7`, `Dm/F`...
- Outputs: `pitch 1` to `pitch 4`, one per voice, for four oscillators;
  `root`, for a bassline to follow; and `gate`.
- Voicing chosen automatically: each voice moves to the nearest note of
  the next chord, so progressions flow instead of jumping.
- Patterns A to D, picked by `play` from a song's track like `notes`, so
  the verse and the chorus can have their own progressions.
- In patch files, chords by name like notes:
  `"patterns": { "A": "Am F C G" }`.
- Later, perhaps: `notes` patterns transposed by the chord playing, so a
  riff follows the progression.
