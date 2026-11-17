#!/usr/bin/env bash
#
# Deploy the OpLendWallet (custodial holder of op-lend shares) contract.
#
# Builds the wasm and deploys it with its constructor (admin + factory).
#
# The wallet is the address the factory's `fiat_invest` mints shares to for an
# investor who has no wallet of their own ($FIAT_HOLDER, once this contract
# replaces the custodial account). Shares are later released with
# scripts/wallet-redeem.sh, which is authorised by a backend OP_REDEEM signature
# rather than by on-chain auth. The wallet reads that signer from the factory
# on every redeem, so rotating it on the factory rotates it here too.
#
# After deploying, map each operation to its op-lend token with
# scripts/wallet-register-op-lend.sh, or redeems cannot find the token to transfer.
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Optional env vars:
#   ADMIN               Contract admin/owner (default: address of SOURCE).
#   FACTORY_ID          Factory whose backend signer authorizes redeems
#                       (default: the network's FACTORY_ID).
#
# Usage:
#   ./scripts/deploy-op-lend-wallet.sh                   # testnet
#   NETWORK=mainnet ./scripts/deploy-op-lend-wallet.sh   # mainnet, signs on the Ledger
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

WALLET_WASM="$WASM_DIR/lend_wallet.wasm"

req SOURCE FACTORY_ID

ADMIN="${ADMIN:-$(source_address)}"

echo "==> Network:        $NETWORK"
echo "==> Source:         $SOURCE"
echo "==> Admin:          $ADMIN"
echo "==> Factory:        $FACTORY_ID"

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
  --factory "$FACTORY_ID" | tail -n1)"

echo ""
echo "==> Done."
echo "    OPLEND_WALLET_ID=$OPLEND_WALLET_ID"
echo "    # record it in DEPLOYMENTS.md and in scripts/common.sh"
echo "    # then: OP_ID=<id> OPLEND=<C...> ./scripts/wallet-register-op-lend.sh"
