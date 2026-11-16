.PHONY: build run check fmt lint test seed

build:
	cargo build

run:
	cargo run -p emu-tui

check:
	cargo run -q -p emu-tui -- --check

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test --workspace

seed:
	./scripts/seed-demo.sh