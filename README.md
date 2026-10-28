# audiochaos

A modular synthesizer written in Rust, built for chaotic, aggressive sounds:
small signal-processing bricks that you route together however you like,
driven from a terminal UI.

## Status

Early work. The signal graph and its offline renderer exist; audio output,
the module library and the TUI are still to come.

## Design notes

- Audio and control signals share the same `f32` buffers, like voltages in a
  hardware modular.
- Modules are processed in insertion order. A cable from a module processed
  later (or from itself) reads its previous block, so feedback is allowed and
  costs exactly one block of delay. Feedback is a main source of chaos here.
- The engine is a library that renders offline, so it is fully testable
  without any audio hardware.

## Build, run, test

Requires a recent stable Rust toolchain and, on Linux, the ALSA development
headers (`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

```sh
cargo build
cargo run
cargo test
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```
