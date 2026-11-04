default:
    @just --list

build:
    cargo build

# Opens the editor: `just run`, `just run bass`, `just run my.json`.
run *args:
    cargo run -- {{args}}

# Same, in the terminal UI.
tui *args:
    cargo run -- --tui {{args}}

test:
    cargo test --workspace

# Builds the VST3 and CLAP plugins into target/bundled/.
bundle:
    cargo xtask bundle audiochaos-plugin --release

# Bundles the plugins and copies them where Linux hosts look for them.
install-plugin: bundle
    mkdir -p ~/.vst3 ~/.clap
    rm -rf ~/.vst3/audiochaos.vst3
    cp -r target/bundled/audiochaos.vst3 ~/.vst3/
    cp target/bundled/audiochaos.clap ~/.clap/

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
