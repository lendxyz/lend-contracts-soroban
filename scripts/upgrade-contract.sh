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
# decimals at `initialize`, and upgrading does not re-read them.
#
# Network, signer and contract ids come from scripts/common.sh. SOURCE must be
# the target contract's admin.
#
# Required env vars:
#   CONTRACT       Which contract to upgrade: factory | rewards.
#
# Optional env vars:
#   CONTRACT_ID    Override the id resolved from common.sh.
#   WASM           Override the built wasm path.
#   BUILD          1 (default) to rebuild first; 0 to use target/ as-is.
#
# Usage:
#   CONTRACT=factory ./scripts/upgrade-contract.sh
#   CONTRACT=rewards NETWORK=mainnet ./scripts/upgrade-contract.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE CONTRACT
need_bin stellar sha256sum

case "$CONTRACT" in
  factory)
    : "${CONTRACT_ID:=${FACTORY_ID:-}}"
    : "${WASM:=$WASM_DIR/lend_factory.wasm}"
    ;;
  rewards)
    : "${CONTRACT_ID:=${REWARDS_ID:-}}"
    : "${WASM:=$WASM_DIR/lend_rewards.wasm}"
    ;;
  *)
    echo "error: CONTRACT must be factory | rewards (got '$CONTRACT')" >&2
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
  echo "       Deploy a fresh instance instead (scripts/deploy-$CONTRACT.sh)," >&2
  echo "       then record the new id in DEPLOYMENTS.md + scripts/common.sh." >&2
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

echo "==> Upgrading $CONTRACT_ID..."
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- upgrade \
  --new_wasm_hash "$NEW_HASH"

echo "==> Verifying the live code changed..."
stellar contract fetch --id "$CONTRACT_ID" "${NETWORK_ARGS[@]}" \
  -o "$AFTER_WASM" >/dev/null
AFTER_HASH="$(sha256sum "$AFTER_WASM" | cut -d' ' -f1)"
if [ "$AFTER_HASH" != "$NEW_HASH" ]; then
  echo "error: $CONTRACT_ID still runs $AFTER_HASH" >&2
  exit 1
fi

echo ""
echo "==> Done. $CONTRACT upgraded in place."
echo "    id:   $CONTRACT_ID"
echo "    from: $BEFORE_HASH"
echo "    to:   $AFTER_HASH"
