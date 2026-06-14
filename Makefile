# Mamba developer tasks.
#
# `make install` is the after-every-feature command: it compiles in release and
# drops the `mamba` binary onto your PATH (~/.cargo/bin, via `cargo install`),
# so `mamba` runs from anywhere. The standing workflow is:
#
#     edit → update docs → make test → make install → git commit → git push
#
.PHONY: install build test run check fmt clean help

# Platform data dir the installed binary falls back to (mirrors dirs::data_dir):
# $XDG_DATA_HOME/mamba if set, else ~/.local/share/mamba.
DATA_DIR := $(or $(XDG_DATA_HOME),$(HOME)/.local/share)/mamba

## install : build+install `mamba` onto PATH and sync the problem bank to the data dir
install:
	cargo install --path . --force
	@mkdir -p "$(DATA_DIR)/problems"
	@cp problems/*.json "$(DATA_DIR)/problems/"
	@echo "synced $(words $(wildcard problems/*.json)) problems → $(DATA_DIR)/problems"

## build   : release build into ./target/release/mamba
build:
	cargo build --release

## test    : run the full unit + integration suite
test:
	cargo test

## run     : launch the TUI from this checkout
run:
	cargo run --release

## gen     : mass-generate problems with the Python builder, then verify them
gen:
	python3 tools/build_problems/generate.py --out problems
	cargo run --quiet -- --verify problems

## verify  : run every problem's canonical answer back through the engine
verify:
	cargo run --quiet -- --verify problems

## check   : fast type-check without producing a binary
check:
	cargo check

## fmt     : format the codebase
fmt:
	cargo fmt

## clean   : remove build artifacts
clean:
	cargo clean

## help    : list these targets
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/^## //'
