default: build

all: test

# Factory integration tests `contractimport!` the op-lend wasm, so it must be
# built (into the shared workspace target/) before the tests compile.
test: build-oplend build
	cargo test

build-oplend:
	stellar contract build --package lend-operation-token
	@ls -l ../../target/wasm32v1-none/release/lend_operation_token.wasm

build: build-oplend
	stellar contract build --package lend_factory
	@ls -l ../../target/wasm32v1-none/release/lend_factory.wasm

fmt:
	cargo fmt --all

clean:
	cargo clean
