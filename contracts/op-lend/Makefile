default: build

all: test

test: build
	cargo test

build:
	stellar contract build --package lend-operation-token
	@ls -l ../../target/wasm32v1-none/release/lend_operation_token.wasm

fmt:
	cargo fmt --all

clean:
	cargo clean
