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

# Deploy targets wrap scripts/. SOURCE + BACKEND_SIGNER default below; override
# on the make line. Other vars (FACTORY_ID, OP_NAME, ...) pass through the env.
# See the script header or scripts/README.md for the full var list.
#   make deploy-factory
#   make deploy-rewards
#   make deploy-dummy-usdc
#   make create-operation FACTORY_ID=C... OP_NAME="Alpha" \
#     TOTAL_SHARES=1000000 EUR_PER_SHARES=1000000
SOURCE ?= lend-testnet
BACKEND_SIGNER ?= GAIOQM6QINN427MWFQUHJZGG6T6KOE2ZGLRS2DVYIUGUOBSREDHJNTQM
export SOURCE BACKEND_SIGNER

deploy-factory:
	./scripts/deploy-factory.sh

deploy-rewards:
	./scripts/deploy-rewards.sh

deploy-dummy-usdc:
	./scripts/deploy-dummy-usdc.sh

create-operation:
	./scripts/create-operation.sh

fmt:
	cargo fmt --all

clean:
	cargo clean

.PHONY: default all test build fmt clean \
	deploy-factory deploy-rewards deploy-dummy-usdc create-operation
