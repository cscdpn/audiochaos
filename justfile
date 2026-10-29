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
    cargo test

fmt:
    cargo fmt

lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
