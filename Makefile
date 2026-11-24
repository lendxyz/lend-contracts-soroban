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
#   make deploy-factory FIRST_OP_ID=5   # first op id (old factory's operation_count + 1)
#   make deploy-rewards
#   make deploy-dummy-tokens ASSET=USDC   # or ASSET=EURC
#   make deploy-op-lend-wallet
# 	make distribute-op-rewards OP_ID=1 EPOCH=3                 # uses sample recipients
# 	make distribute-op-rewards OP_ID=1 EPOCH=3 RECIPIENTS=./round3.json
# 	make distribute-op-rewards ASSET=EURC OP_ID=1 EPOCH=3       # EURC rewards contract
#   make create-operation OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARE=1000000
#   make start-operation OP_ID=0
#   make withdraw-funds OP_ID=1 DESTINATION=G...
#   make withdraw-funds OP_ID=1 DESTINATION=G... DRY_RUN=1
#   make invest OP_ID=0 SHARES=100 NONCE=abc SIGNATURE=deadbeef...
#   make invest-with-proof OP_ID=1 AMOUNT=1000000000   # SOURCE forced to test-user
#   make fiat-invest OP_ID=1 SHARES=1000000 INVESTOR=G...   # signs locally
#   make fiat-invest OP_ID=1 SHARES=1000000 INVESTOR=G... DRY_RUN=1
#   make wallet-register-op-lend OP_ID=1   # token copied from the factory
#   make wallet-redeem OP_ID=1 DESTINATION=G... AMOUNT=1000000
#   make wallet-redeem OP_ID=1 DESTINATION=G... AMOUNT=1000000 WHITELIST=1 DRY_RUN=1
#   make fund-dummy-tokens ASSET=USDC TO=G... AMOUNT_WHOLE=5000
#   make fund-dummy-tokens ASSET=EURC TO=G...
#   make update-backend-signer BACKEND_SIGNER=GAOQ67SJ...   # factory only; tokens + wallet follow
#   make upgrade-contract CONTRACT=factory   # in place, same id + state
#   make upgrade-contract CONTRACT=wallet
#   make wallet-set-factory                  # after the wallet upgrade / a factory redeploy
#   make upgrade-contract CONTRACT=oplend OP_ID=5   # via the factory; also sets the hash new ops use
#
# Mainnet (signs on a Ledger — plug it in, unlock, open the Stellar app):
#   make create-operation NETWORK=mainnet OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARE=1000000

deploy-factory:
	./scripts/deploy-factory.sh

deploy-rewards:
	./scripts/deploy-rewards.sh

# Custodial wallet that holds op-lend shares for fiat investors until they are
# redeemed to a real user wallet.
deploy-op-lend-wallet:
	./scripts/deploy-op-lend-wallet.sh

# In-place code swap for contracts that expose upgrade(); see the script header.
upgrade-contract:
	./scripts/upgrade-contract.sh

distribute-op-rewards: RECIPIENTS ?= scripts/recipients.json
distribute-op-rewards: OUT ?= scripts/merkle.json
distribute-op-rewards:
	RECIPIENTS="$(RECIPIENTS)" OUT="$(OUT)" ./scripts/distribute-op-rewards.sh

deploy-dummy-tokens:
	./scripts/deploy-dummy-tokens.sh

create-operation:
	./scripts/create-operation.sh

start-operation:
	./scripts/start-operation.sh

# Admin-only. Sends the USDC and EURC an operation raised since the last
# withdrawal to DESTINATION. DRY_RUN=1 simulates without submitting.
withdraw-funds:
	./scripts/withdraw-funds.sh

invest:
	./scripts/invest.sh

# Fetches the mint proof from the API then invests. The API login needs a local
# secret key, so this one pins its own identity instead of common.sh's.
invest-with-proof:
	SOURCE=test-user ./scripts/invest-with-proof.sh

# Signs the FIAT_INVEST payload with the backend signer key from the Stellar CLI
# keystore, then calls fiat_invest. DRY_RUN=1 simulates without spending the nonce.
fiat-invest:
	./scripts/fiat-invest.sh

# Maps an operation id to its op-lend token on the wallet (copied from the
# factory), without which a redeem has no token to transfer. OP_IDS='[1,2]'
# registers a batch in one call.
wallet-register-op-lend:
	./scripts/wallet-register-op-lend.sh

# Signs the OP_REDEEM payload with the backend signer key from the Stellar CLI
# keystore, then releases shares from the wallet. WHITELIST=1 whitelists the
# destination on the op token in the same call; DRY_RUN=1 simulates without
# spending a nonce.
wallet-redeem:
	./scripts/wallet-redeem.sh

# Points the wallet at FACTORY_ID, whose backend_signer authorizes redeems.
wallet-set-factory:
	./scripts/wallet-set-factory.sh

fund-dummy-tokens:
	./scripts/fund-dummy-tokens.sh

update-backend-signer:
	./scripts/update-backend-signer.sh

fmt:
	cargo fmt --all

clean:
	cargo clean

.PHONY: default all test build fmt clean \
	deploy-factory deploy-rewards distribute-op-rewards deploy-dummy-tokens create-operation start-operation invest \
	invest-with-proof fiat-invest fund-dummy-tokens update-backend-signer upgrade-contract \
	deploy-op-lend-wallet wallet-register-op-lend wallet-redeem wallet-set-factory \
	withdraw-funds
