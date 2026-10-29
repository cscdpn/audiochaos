default:
    @just --list

build:
    cargo build

# Plays a preset: `just run` (chaos) or `just run bass`.
run *args:
    cargo run -- {{args}}

test:
    cargo test

fmt:
    cargo fmt

lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
