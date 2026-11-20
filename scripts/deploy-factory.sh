#!/usr/bin/env bash
#
# Deploy the Lend contracts to a Stellar network.
#
# Builds the wasms, uploads the op-lend wasm (factory deploys op-lend instances
# from its hash), then deploys the factory. Its setup runs in `__constructor`,
# inside the deploy transaction, so there is no separate init call to front-run.
#
# Network, signer and addresses come from scripts/common.sh (NETWORK, SOURCE,
# USDC, EURC, ORACLE, USDC_ORACLE, BACKEND_SIGNER, SIGN_ARGS); override any of
# them in the env.
#
# Required env vars:
#   FIRST_OP_ID     Id of the first operation this factory creates. When
#                   replacing a factory, use its `operation_count` + 1 so op ids
#                   (keyed in the op-lend wallet and rewards) never repeat; 1 on
#                   a network that never had one.
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

req SOURCE USDC EURC ORACLE USDC_ORACLE BACKEND_SIGNER FIRST_OP_ID

# The contract's backend_signer param is BytesN<32>, so the CLI needs 64 hex
# chars; a G... strkey is decoded for convenience.
BACKEND_SIGNER="$(strkey_to_hex "$BACKEND_SIGNER")"

ADMIN="${ADMIN:-$(source_address)}"

echo "==> Network:        $NETWORK"
echo "==> Source:         $SOURCE"
echo "==> Admin:          $ADMIN"
echo "==> USDC:           $USDC"
echo "==> EURC:           $EURC"
echo "==> Oracle:         $ORACLE"
echo "==> USDC oracle:    $USDC_ORACLE"
echo "==> Backend signer: $BACKEND_SIGNER"
echo "==> First op id:    $FIRST_OP_ID"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

# A wasm's hash is the sha256 of its bytes, so it is known before uploading. That
# lets a re-run skip an upload that already landed — which matters here, because
# each step needs a device approval and "submission timeout" does not tell you
# whether the transaction made it (check with: stellar tx fetch <hash>).
OPLEND_WASM_HASH="$(sha256sum "$OPLEND_WASM" | cut -d' ' -f1)"
if stellar contract fetch --wasm-hash "$OPLEND_WASM_HASH" \
  "${NETWORK_ARGS[@]}" -o /dev/null 2>/dev/null; then
  echo "==> op-lend wasm already uploaded, skipping"
else
  echo "==> Uploading op-lend wasm..."
  UPLOADED_HASH="$(stellar contract upload \
    --wasm "$OPLEND_WASM" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" | tail -n1)"
  if [ "$UPLOADED_HASH" != "$OPLEND_WASM_HASH" ]; then
    echo "error: uploaded hash $UPLOADED_HASH != local sha256 $OPLEND_WASM_HASH" >&2
    exit 1
  fi
fi
echo "    op-lend wasm hash: $OPLEND_WASM_HASH"

# Deterministic salt, so re-running after a submission timeout re-derives the same
# contract id instead of paying for a second factory. The factory wasm hash is
# part of it: a new build is a fresh deployment (a constructor cannot re-run on
# an existing id), while re-running the same build finds the one it deployed.
# Override DEPLOY_SALT (any 32-byte hex) to deploy an additional instance on
# purpose.
FACTORY_WASM_HASH="$(sha256sum "$FACTORY_WASM" | cut -d' ' -f1)"
: "${DEPLOY_SALT:=$(printf '%s' "lend-factory:$NETWORK:$FACTORY_WASM_HASH" | sha256sum | cut -d' ' -f1)}"
FACTORY_ID="$(stellar contract id wasm \
  --salt "$DEPLOY_SALT" --source-account "$SOURCE" "${NETWORK_ARGS[@]}")"

if stellar contract info interface --id "$FACTORY_ID" \
  "${NETWORK_ARGS[@]}" >/dev/null 2>&1; then
  echo "==> Factory already deployed, skipping"
else
  echo "==> Deploying factory..."
  DEPLOYED_ID="$(stellar contract deploy \
    --wasm "$FACTORY_WASM" \
    --source "$SOURCE" \
    --salt "$DEPLOY_SALT" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- \
    --admin "$ADMIN" \
    --usdc "$USDC" \
    --eurc "$EURC" \
    --oracle "$ORACLE" \
    --usdc_oracle "$USDC_ORACLE" \
    --backend_signer "$BACKEND_SIGNER" \
    --oplend_wasm_hash "$OPLEND_WASM_HASH" \
    --first_op_id "$FIRST_OP_ID" | tail -n1)"
  if [ "$DEPLOYED_ID" != "$FACTORY_ID" ]; then
    echo "error: deployed id $DEPLOYED_ID != derived $FACTORY_ID" >&2
    exit 1
  fi
fi
echo "    factory id: $FACTORY_ID"

echo ""
echo "==> Done."
echo "    FACTORY_ID=$FACTORY_ID"
echo "    OPLEND_WASM_HASH=$OPLEND_WASM_HASH"
echo "    # record both in DEPLOYMENTS.md and in scripts/common.sh"
