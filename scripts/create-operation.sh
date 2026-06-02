#!/usr/bin/env bash
#
# Create an operation on a deployed factory. Deploys a fresh op-lend token and
# registers it. Admin-only (SOURCE must be the factory admin).
#
# Required env vars:
#   SOURCE          Stellar CLI identity (must be the factory admin).
#   FACTORY_ID      Deployed factory contract address (C...).
#   OP_NAME         Human name, e.g. "Alpha Fund".
#   TOTAL_SHARES    Max shares / supply cap (integer, 6 decimals).
#   EUR_PER_SHARES  Price per share in EUR (integer, 6 decimals; 1 EUR = 1000000).
#
# Optional env vars:
#   NETWORK         Network name (default: testnet).
#
# Usage:
#   SOURCE=alice FACTORY_ID=C... OP_NAME="Alpha" TOTAL_SHARES=1000000 \
#     EUR_PER_SHARES=1000000 ./scripts/create-operation.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req FACTORY_ID
req OP_NAME
req TOTAL_SHARES
req EUR_PER_SHARES

echo "==> Creating operation '$OP_NAME' on $FACTORY_ID ($NETWORK)..."
OP_TOKEN="$(stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- create_operation \
  --op_name "$OP_NAME" \
  --total_shares "$TOTAL_SHARES" \
  --eur_per_shares "$EUR_PER_SHARES" | tail -n1)"

echo "==> op-lend token deployed at: $OP_TOKEN"
