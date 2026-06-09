#!/usr/bin/env bash
#
# Deploy the LendRewards (merkle reward distribution) contract.
#
# Builds the wasm and deploys it with its constructor (admin + reward token).
# The reward token is USDC; it defaults to the network's Circle USDC SAC.
#
# Required env vars:
#   SOURCE        Stellar CLI identity used to sign + pay.
#
# Optional env vars:
#   NETWORK       testnet | mainnet (default: testnet).
#   REWARD_TOKEN  Reward token contract; defaults to the network's USDC.
#   ADMIN         Contract admin/owner (default: address of SOURCE).
#
# Usage:
#   SOURCE=alice ./scripts/deploy-rewards.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REWARDS_WASM="$REPO_ROOT/target/wasm32v1-none/release/lend_rewards.wasm"

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE

# Verified 2026-06-02 (Circle USDC SAC). Same addresses as deploy.sh.
case "$NETWORK" in
  testnet)
    : "${REWARD_TOKEN:=CCO56ZVZPLGELBZGAVLTNC5GPZUIF4SIAIGPYNHWBRUSKBLC7HPF5QPN}"
    ;;
  mainnet|pubnet|public)
    : "${REWARD_TOKEN:=CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75}"
    ;;
esac
req REWARD_TOKEN

ADMIN="${ADMIN:-$(stellar keys address "$SOURCE")}"

echo "==> Network:      $NETWORK"
echo "==> Source:       $SOURCE"
echo "==> Admin:        $ADMIN"
echo "==> Reward token: $REWARD_TOKEN"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

echo "==> Deploying LendRewards..."
REWARDS_ID="$(stellar contract deploy \
  --wasm "$REWARDS_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- \
  --admin "$ADMIN" \
  --reward_token "$REWARD_TOKEN" | tail -n1)"

echo ""
echo "==> Done."
echo "    REWARDS_ID=$REWARDS_ID"
