# audiochaos

A modular synthesizer written in Rust, built for chaotic, aggressive sounds:
small signal-processing bricks that you route together however you like,
driven from a terminal UI.

## Status

Early work. The signal graph, audio output and a first set of modules
(oscillator, VCA, master) exist. `just run` plays a sine and reads new
frequencies from stdin; the TUI is still to come.

## Design notes

- Audio and control signals share the same `f32` buffers, like voltages in a
  hardware modular.
- Modules are processed in insertion order. A cable from a module processed
  later (or from itself) reads its previous block, so feedback is allowed and
  costs exactly one block of delay. Feedback is a main source of chaos here.
- The audio thread owns the graph, so it needs no lock. The UI talks to it
  through a lock-free queue (`rtrb`) drained once per block, and parameters
  are smoothed so changes never click.
- The master module soft-clips with `tanh` and mutes non-finite samples:
  feedback patches can explode, and the speakers should never see that.
- The engine is a library that renders offline, so it is fully testable
  without any audio hardware.

## Build, run, test

Requires a recent stable Rust toolchain and, on Linux, the ALSA development
headers (`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

```sh
just build
just run
just test
just lint   # cargo fmt --check + clippy
just fmt
```

[`just`](https://github.com/casey/just) is optional; the recipes are thin
wrappers around the usual `cargo` commands in the `justfile`.
