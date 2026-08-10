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

# Deploy targets wrap scripts/. Shared config (NETWORK, SOURCE, FACTORY_ID,
# addresses, Ledger signing) lives in scripts/common.sh; per-run vars are passed
# on the make line and reach the scripts through the environment.
#   make deploy-factory
#   make deploy-rewards
#   make deploy-dummy-usdc
# 	make distribute-op-rewards OP_ID=1 EPOCH=3                 # uses sample recipients
# 	make distribute-op-rewards OP_ID=1 EPOCH=3 RECIPIENTS=./round3.json
#   make create-operation OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARES=1000000
#   make start-operation OP_ID=0
#   make invest OP_ID=0 SHARES=100 NONCE=abc SIGNATURE=deadbeef...
#   make invest-with-proof OP_ID=1 AMOUNT=1000000000   # SOURCE forced to test-user
#   make fund-dummy-usdc TO=G... AMOUNT_WHOLE=5000
#   make update-backend-signer BACKEND_SIGNER=GAOQ67SJ...
#
# Mainnet (signs on a Ledger — plug it in, unlock, open the Stellar app):
#   make create-operation NETWORK=mainnet OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARES=1000000

deploy-factory:
	./scripts/deploy-factory.sh

deploy-rewards:
	./scripts/deploy-rewards.sh

distribute-op-rewards: RECIPIENTS ?= scripts/recipients.json
distribute-op-rewards: OUT ?= scripts/merkle.json
distribute-op-rewards:
	RECIPIENTS="$(RECIPIENTS)" OUT="$(OUT)" ./scripts/distribute-op-rewards.sh

deploy-dummy-usdc:
	./scripts/deploy-dummy-usdc.sh

create-operation:
	./scripts/create-operation.sh

start-operation:
	./scripts/start-operation.sh

invest:
	./scripts/invest.sh

# Fetches the mint proof from the API then invests. The API login needs a local
# secret key, so this one pins its own identity instead of common.sh's.
invest-with-proof:
	SOURCE=test-user ./scripts/invest-with-proof.sh

fund-dummy-usdc:
	./scripts/fund-dummy-usdc.sh

update-backend-signer:
	./scripts/update-backend-signer.sh

fmt:
	cargo fmt --all

clean:
	cargo clean

.PHONY: default all test build fmt clean \
	deploy-factory deploy-rewards distribute-op-rewards deploy-dummy-usdc create-operation start-operation invest \
	invest-with-proof fund-dummy-usdc update-backend-signer
