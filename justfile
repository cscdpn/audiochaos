default:
    @just --list

build:
    cargo build

run:
    cargo run

test:
    cargo test

fmt:
    cargo fmt

lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
