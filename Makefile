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
# 	make distribute-op-rewards REWARDS_ID=C... OP_ID=1 EPOCH=3 # uses sample
# 	make distribute-op-rewards REWARDS_ID=C... OP_ID=1 EPOCH=3 RECIPIENTS=./round3.json
#   make create-operation OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARES=1000000
#   make start-operation OP_ID=0
#   make invest OP_ID=0 SHARES=100 NONCE=abc SIGNATURE=deadbeef...
#   make invest-with-proof OP_ID=1 AMOUNT=1000000000   # SOURCE defaults to test-user
#   make fund-dummy-usdc DUMMY_USDC_ID=CC... TO=G... AMOUNT_WHOLE=5000
#   make update-backend-signer BACKEND_SIGNER=GAOQ67SJ...
SOURCE ?= lend-testnet
BACKEND_SIGNER ?= GAIOQM6QINN427MWFQUHJZGG6T6KOE2ZGLRS2DVYIUGUOBSREDHJNTQM
export SOURCE BACKEND_SIGNER

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

# Fetches the mint proof from the API then invests. Defaults SOURCE to test-user.
invest-with-proof: SOURCE := test-user
invest-with-proof:
	./scripts/invest-with-proof.sh

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
