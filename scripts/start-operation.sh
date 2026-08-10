#!/usr/bin/env bash
#
# Start an operation on a deployed factory. Admin-only (SOURCE must be the
# factory admin).
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Required env vars:
#   OP_ID           Operation id (integer).
#
# Usage:
#   OP_ID=0 ./scripts/start-operation.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID OP_ID

echo "==> Starting operation $OP_ID on $FACTORY_ID ($NETWORK)..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- start_operation \
  --id "$OP_ID"

echo "==> operation $OP_ID started"
