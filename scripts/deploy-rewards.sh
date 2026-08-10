#!/usr/bin/env bash
#
# Deploy the LendRewards (merkle reward distribution) contract.
#
# Builds the wasm and deploys it with its constructor (admin + reward token).
#
# Network, signer and USDC come from scripts/common.sh; override in the env.
#
# Optional env vars:
#   REWARD_TOKEN  Reward token contract (default: the network's USDC).
#   ADMIN         Contract admin/owner (default: address of SOURCE).
#
# Usage:
#   ./scripts/deploy-rewards.sh                   # testnet
#   NETWORK=mainnet ./scripts/deploy-rewards.sh   # mainnet, signs on the Ledger
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

REWARDS_WASM="$WASM_DIR/lend_rewards.wasm"

# LendRewards pays out in USDC (DummyUSDC on testnet).
REWARD_TOKEN="${REWARD_TOKEN:-$USDC}"

req SOURCE REWARD_TOKEN

ADMIN="${ADMIN:-$(source_address)}"

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
  "${SIGN_ARGS[@]}" \
  -- \
  --admin "$ADMIN" \
  --reward_token "$REWARD_TOKEN" | tail -n1)"

echo ""
echo "==> Done."
echo "    REWARDS_ID=$REWARDS_ID"
echo "    # record it in DEPLOYMENTS.md and in scripts/common.sh"
