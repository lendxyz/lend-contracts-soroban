#!/usr/bin/env bash
#
# Create an operation on a deployed factory. Deploys a fresh op-lend token and
# registers it. Admin-only (SOURCE must be the factory admin).
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Required env vars:
#   OP_NAME         Human name, e.g. "Alpha Fund".
#   TOTAL_SHARES    Max shares / supply cap (integer, 6 decimals).
#   EUR_PER_SHARES  Price per share in EUR (integer, 6 decimals; 1 EUR = 1000000).
#
# Usage:
#   OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARES=1000000 ./scripts/create-operation.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID OP_NAME TOTAL_SHARES EUR_PER_SHARES

echo "==> Creating operation '$OP_NAME' on $FACTORY_ID ($NETWORK)..."
OP_TOKEN="$(stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  "${SIGN_ARGS[@]}" \
  -- create_operation \
  --op_name "$OP_NAME" \
  --total_shares "$TOTAL_SHARES" \
  --eur_per_shares "$EUR_PER_SHARES" | tail -n1)"

echo "==> op-lend token deployed at: $OP_TOKEN"
