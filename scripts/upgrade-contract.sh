#!/usr/bin/env bash
#
# Upgrade a deployed contract in place: same contract id, same state, new code.
#
# Soroban can only replace a contract's wasm from inside the contract itself, so
# this works only where an admin-only `upgrade(new_wasm_hash)` entrypoint was
# already deployed. A contract published before that entrypoint existed can
# never be upgraded — it has to be redeployed under a new id. The script checks
# for the entrypoint before touching anything.
#
# Instance storage survives an upgrade, so the incoming wasm has to cope with
# whatever the outgoing one wrote (or migrate it itself). Write-once config is
# *not* rewritten: e.g. the factory caches its USDC token and that token's
# decimals in its constructor, and upgrading does not re-read them.
#
# Network, signer and contract ids come from scripts/common.sh. SOURCE must be
# the target contract's admin. An op-lend token's admin is the factory, so
# CONTRACT=oplend goes through `factory.oplend_upgrade` (SOURCE = factory
# admin), which also makes the new wasm the code later operations deploy from.
#
# Required env vars:
#   CONTRACT       Which contract to upgrade: factory | rewards | wallet | oplend.
#   OP_ID          With CONTRACT=oplend: the operation whose token to upgrade.
#
# Optional env vars:
#   CONTRACT_ID    Override the id resolved from common.sh (or, for oplend,
#                  from the factory's `get_operation`).
#   WASM           Override the built wasm path.
#   BUILD          1 (default) to rebuild first; 0 to use target/ as-is.
#
# Usage:
#   CONTRACT=factory ./scripts/upgrade-contract.sh
#   CONTRACT=rewards NETWORK=mainnet ./scripts/upgrade-contract.sh
#   CONTRACT=wallet ./scripts/upgrade-contract.sh
#   CONTRACT=oplend OP_ID=5 ./scripts/upgrade-contract.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE CONTRACT
need_bin stellar sha256sum

VIA_FACTORY=0
case "$CONTRACT" in
  factory)
    : "${CONTRACT_ID:=${FACTORY_ID:-}}"
    : "${WASM:=$WASM_DIR/lend_factory.wasm}"
    DEPLOY_SCRIPT="scripts/deploy-factory.sh"
    ;;
  rewards)
    : "${CONTRACT_ID:=${REWARDS_ID_USD:-}}"
    : "${WASM:=$WASM_DIR/lend_rewards.wasm}"
    DEPLOY_SCRIPT="scripts/deploy-rewards.sh"
    ;;
  wallet | op-lend-wallet)
    : "${CONTRACT_ID:=${OPLEND_WALLET_ID:-}}"
    : "${WASM:=$WASM_DIR/lend_wallet.wasm}"
    DEPLOY_SCRIPT="scripts/deploy-op-lend-wallet.sh"
    ;;
  oplend | op-lend)
    req OP_ID FACTORY_ID
    need_bin jq
    VIA_FACTORY=1
    if [ -z "${CONTRACT_ID:-}" ]; then
      CONTRACT_ID="$(stellar contract invoke --id "$FACTORY_ID" \
        --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
        -- get_operation --id "$OP_ID" | jq -r '.op_token')"
    fi
    : "${WASM:=$WASM_DIR/lend_operation_token.wasm}"
    DEPLOY_SCRIPT="scripts/create-operation.sh"
    ;;
  *)
    echo "error: CONTRACT must be factory | rewards | wallet | oplend (got '$CONTRACT')" >&2
    exit 1
    ;;
esac
req CONTRACT_ID

echo "==> Network:  $NETWORK"
echo "==> Contract: $CONTRACT ($CONTRACT_ID)"
echo "==> Source:   $SOURCE"

if [ "${BUILD:-1}" = "1" ]; then
  echo "==> Building wasms..."
  (cd "$REPO_ROOT" && stellar contract build)
fi
[ -f "$WASM" ] || { echo "error: wasm not found: $WASM (BUILD=0?)" >&2; exit 1; }

# Refuse before spending a fee if the deployed code cannot be upgraded at all.
echo "==> Checking the deployed contract exposes upgrade()..."
if ! stellar contract info interface --id "$CONTRACT_ID" "${NETWORK_ARGS[@]}" \
  | grep -q 'fn upgrade'; then
  echo "error: $CONTRACT_ID has no upgrade() entrypoint, so its code is frozen." >&2
  if [ "$VIA_FACTORY" = "1" ]; then
    echo "       This op-lend token predates op-lend upgrade(); only operations" >&2
    echo "       created from a newer op-lend wasm can be upgraded." >&2
  else
    echo "       Deploy a fresh instance instead ($DEPLOY_SCRIPT)," >&2
    echo "       then record the new id in DEPLOYMENTS.md + scripts/common.sh." >&2
  fi
  exit 1
fi

# A contract's wasm hash is the sha256 of its bytes, so fetching the live code
# gives a real before/after rather than trusting that the invoke did anything.
BEFORE_WASM="$(mktemp)"
AFTER_WASM="$(mktemp)"
trap 'rm -f "$BEFORE_WASM" "$AFTER_WASM"' EXIT

stellar contract fetch --id "$CONTRACT_ID" "${NETWORK_ARGS[@]}" \
  -o "$BEFORE_WASM" >/dev/null
BEFORE_HASH="$(sha256sum "$BEFORE_WASM" | cut -d' ' -f1)"
NEW_HASH="$(sha256sum "$WASM" | cut -d' ' -f1)"

echo "==> On chain now: $BEFORE_HASH"
echo "==> Local build:  $NEW_HASH"
if [ "$BEFORE_HASH" = "$NEW_HASH" ]; then
  echo "==> Identical code already deployed; nothing to do."
  exit 0
fi

# Uploading is skipped when the wasm is already on chain, e.g. when upgrading a
# second operation's op-lend to the same build.
if stellar contract fetch --wasm-hash "$NEW_HASH" \
  "${NETWORK_ARGS[@]}" -o /dev/null 2>/dev/null; then
  echo "==> Wasm already uploaded, skipping"
else
  echo "==> Uploading new wasm..."
  UPLOADED_HASH="$(stellar contract upload \
    --wasm "$WASM" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" | tail -n1)"
  if [ "$UPLOADED_HASH" != "$NEW_HASH" ]; then
    echo "error: uploaded hash $UPLOADED_HASH != local sha256 $NEW_HASH" >&2
    exit 1
  fi
fi

if [ "$VIA_FACTORY" = "1" ]; then
  echo "==> Upgrading $CONTRACT_ID through the factory (operation $OP_ID)..."
  stellar contract invoke \
    --id "$FACTORY_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- oplend_upgrade \
    --op_id "$OP_ID" \
    --new_wasm_hash "$NEW_HASH"
else
  echo "==> Upgrading $CONTRACT_ID..."
  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- upgrade \
    --new_wasm_hash "$NEW_HASH"
fi

echo "==> Verifying the live code changed..."
stellar contract fetch --id "$CONTRACT_ID" "${NETWORK_ARGS[@]}" \
  -o "$AFTER_WASM" >/dev/null
AFTER_HASH="$(sha256sum "$AFTER_WASM" | cut -d' ' -f1)"
if [ "$AFTER_HASH" != "$NEW_HASH" ]; then
  echo "error: $CONTRACT_ID still runs $AFTER_HASH" >&2
  exit 1
fi

if [ "$VIA_FACTORY" = "1" ]; then
  echo "==> Verifying the factory deploys new operations from it..."
  STORED_HASH="$(stellar contract invoke --id "$FACTORY_ID" --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" --send=no -- oplend_wasm_hash | tr -d '"')"
  if [ "$STORED_HASH" != "$NEW_HASH" ]; then
    echo "error: factory still deploys op-lend from $STORED_HASH" >&2
    exit 1
  fi
fi

echo ""
echo "==> Done. $CONTRACT upgraded in place."
echo "    id:   $CONTRACT_ID"
echo "    from: $BEFORE_HASH"
echo "    to:   $AFTER_HASH"
