#!/usr/bin/env bash
#
# Deploy the OpLendWallet (custodial holder of op-lend shares) contract.
#
# Builds the wasm and deploys it with its constructor (admin + backend signer).
#
# The wallet is the address the factory's `fiat_invest` mints shares to for an
# investor who has no wallet of their own ($FIAT_HOLDER, once this contract
# replaces the custodial account). Shares are later released with
# scripts/wallet-redeem.sh, which is authorised by a backend OP_REDEEM signature rather
# than by on-chain auth — hence the signer baked in here.
#
# After deploying, map each operation to its op-lend token with
# scripts/wallet-register-op-lend.sh, or redeems cannot find the token to transfer.
#
# Network, signer and BACKEND_SIGNER come from scripts/common.sh; override in
# the env.
#
# Optional env vars:
#   ADMIN               Contract admin/owner (default: address of SOURCE).
#   BACKEND_SIGNER      Backend ed25519 public key: 64 hex chars or a G...
#                       strkey (default: the network's BACKEND_SIGNER).
#
# Usage:
#   ./scripts/deploy-op-lend-wallet.sh                   # testnet
#   NETWORK=mainnet ./scripts/deploy-op-lend-wallet.sh   # mainnet, signs on the Ledger
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

WALLET_WASM="$WASM_DIR/lend_wallet.wasm"

req SOURCE BACKEND_SIGNER

# The constructor's backend_signer param is BytesN<32>, so the CLI needs 64 hex
# chars; a G... strkey is decoded for convenience.
BACKEND_SIGNER_HEX="$(strkey_to_hex "$BACKEND_SIGNER")"

ADMIN="${ADMIN:-$(source_address)}"

echo "==> Network:        $NETWORK"
echo "==> Source:         $SOURCE"
echo "==> Admin:          $ADMIN"
echo "==> Backend signer: $BACKEND_SIGNER_HEX"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

echo "==> Deploying OpLendWallet..."
OPLEND_WALLET_ID="$(stellar contract deploy \
  --wasm "$WALLET_WASM" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- \
  --admin "$ADMIN" \
  --backend_signer "$BACKEND_SIGNER_HEX" | tail -n1)"

echo ""
echo "==> Done."
echo "    OPLEND_WALLET_ID=$OPLEND_WALLET_ID"
echo "    # record it in DEPLOYMENTS.md and in scripts/common.sh"
echo "    # then: OP_ID=<id> OPLEND=<C...> ./scripts/wallet-register-op-lend.sh"
