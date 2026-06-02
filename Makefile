default: build

all: test

# `stellar contract build` builds every workspace contract (factory + op-lend)
# into target/. The factory integration tests `contractimport!` the op-lend
# wasm, so building before `cargo test` is required.
test: build
	cargo test

build:
	stellar contract build
	@ls -l target/wasm32v1-none/release/*.wasm

fmt:
	cargo fmt --all

clean:
	cargo clean
