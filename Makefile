.PHONY: build start format check

build:
	cargo build

start:
	cargo run

format:
	cargo fmt

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
