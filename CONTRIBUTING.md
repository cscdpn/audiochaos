# Contributing

## Setup

A stable Rust toolchain, plus on Linux the ALSA and D-Bus development
headers (see the [README](README.md#build-run-test)). [`just`](https://github.com/casey/just)
is optional: each recipe in the `justfile` is one or two cargo commands.

```sh
just test   # every test of the workspace: library, program, plugin
just lint   # cargo fmt --check, then clippy with warnings as errors
just fmt
```

Before a change is done: `just fmt`, `just lint` and `just test` pass. Also
`cargo clippy --no-default-features --features editor --all-targets -- -D warnings`
when touching code the plugin uses, since it builds the library without the
`app` feature.

## Rules

- **The audio thread never allocates, frees, locks or waits.** That covers
  `Module::process` and everything it calls, `Engine::next_sample` and the
  plugin's `process`. Allocate in constructors, on the UI thread; see
  [ARCHITECTURE.md](ARCHITECTURE.md#threads). `tests/realtime.rs` and the
  plugin's tests catch most mistakes, not all.
- New behaviour comes with tests, and a fixed bug with a test that failed
  before the fix. Test names say what behaves how:
  `a_stopped_engine_still_takes_changes`.
- Comments and docs explain why: the constraint, the trade-off, what was
  tried. The code says what.
- A dependency is added only when it earns its place, at a deliberate
  version in `Cargo.toml`, with a comment when the choice is not obvious
  (egui stays on 0.36 for egui-snarl and nice-plug-egui). `Cargo.lock` is
  committed, so every build uses the same versions.
- Commits are small, and their message says why the change was made.
- English everywhere in the repository.

## Adding a module

1. Write `src/modules/<name>.rs`, after a module that looks like yours
   (`invert.rs` is the smallest, `delay.rs` has knobs, CV and a clock
   input):
   - constants for its input, output and knob indices, so patches in code
     read `delay::TIME` rather than `0`;
   - `pub static INFO: ModuleInfo`, with a description and a `help` text
     per knob, which the editor shows as tooltips. Port and knob names are
     what patch files refer to: renaming one breaks saved patches;
   - a struct implementing `Module`, allocating everything in `new`;
   - tests, with the helpers of `modules::test_util`.
2. Declare it in `src/modules/mod.rs` and add a `Kind` to `KINDS`, where it
   should appear in the editor's palette.
3. Add it to the Modules table of the README.

The registry's tests then check its descriptions, that its knobs fit
(`MAX_PARAMS`), and `tests/stability.rs` feeds it chaotic signals.

## Adding a preset

Save a patch from the editor (**Patch → Export…**), move it to `presets/`
and list it in `src/presets.rs`. Presets are compiled in, listed in the
Patch menu, checked to load and round-trip, and played by the allocation
test.

## Releasing

1. Update `CHANGELOG.md` and the version in `[workspace.package]` of the
   root `Cargo.toml`, which every crate shares.
2. `just lint && just test`, then the hand checks: sound, MIDI controller,
   sound card input, low latency, the plugin in a DAW, on Linux and Windows.
3. Tag `vX.Y.Z` and push the tag.
