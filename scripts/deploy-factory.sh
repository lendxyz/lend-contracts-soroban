#!/usr/bin/env bash
#
# Deploy the Lend contracts to a Stellar network.
#
# Builds the wasms, uploads the op-lend wasm (factory deploys op-lend instances
# from its hash), deploys the factory, then calls `initialize`.
#
# Network, signer and addresses come from scripts/common.sh (NETWORK, SOURCE,
# USDC, ORACLE, BACKEND_SIGNER, SIGN_ARGS); override any of them in the env.
#
# Optional env vars:
#   ADMIN           Factory admin address (default: address of SOURCE).
#
# Usage:
#   ./scripts/deploy-factory.sh                   # testnet
#   NETWORK=mainnet ./scripts/deploy-factory.sh   # mainnet, signs on the Ledger
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

OPLEND_WASM="$WASM_DIR/lend_operation_token.wasm"
FACTORY_WASM="$WASM_DIR/lend_factory.wasm"

req SOURCE USDC ORACLE BACKEND_SIGNER

# The contract's backend_signer param is BytesN<32>, so the CLI needs 64 hex
# chars; a G... strkey is decoded for convenience.
BACKEND_SIGNER="$(strkey_to_hex "$BACKEND_SIGNER")"

ADMIN="${ADMIN:-$(source_address)}"

echo "==> Network:        $NETWORK"
echo "==> Source:         $SOURCE"
echo "==> Admin:          $ADMIN"
echo "==> USDC:           $USDC"
echo "==> Oracle:         $ORACLE"
echo "==> Backend signer: $BACKEND_SIGNER"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

echo "==> Uploading op-lend wasm..."
OPLEND_WASM_HASH="$(stellar contract upload \
  --wasm "$OPLEND_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  "${SIGN_ARGS[@]}" | tail -n1)"
echo "    op-lend wasm hash: $OPLEND_WASM_HASH"

echo "==> Deploying factory..."
FACTORY_ID="$(stellar contract deploy \
  --wasm "$FACTORY_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  "${SIGN_ARGS[@]}" | tail -n1)"
echo "    factory id: $FACTORY_ID"

echo "==> Initializing factory..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  "${SIGN_ARGS[@]}" \
  -- initialize \
  --admin "$ADMIN" \
  --usdc "$USDC" \
  --oracle "$ORACLE" \
  --backend_signer "$BACKEND_SIGNER" \
  --oplend_wasm_hash "$OPLEND_WASM_HASH"

echo ""
echo "==> Done."
echo "    FACTORY_ID=$FACTORY_ID"
echo "    OPLEND_WASM_HASH=$OPLEND_WASM_HASH"
echo "    # record both in DEPLOYMENTS.md and in scripts/common.sh"
