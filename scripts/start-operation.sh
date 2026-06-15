#!/usr/bin/env bash
#
# Start an operation on a deployed factory. Admin-only (SOURCE must be the
# factory admin).
#
# Required env vars:
#   SOURCE          Stellar CLI identity (must be the factory admin).
#   FACTORY_ID      Deployed factory contract address (C...).
#   OP_ID           Operation id (integer).
#
# Optional env vars:
#   NETWORK         Network name (default: testnet).
#
# Usage:
#   SOURCE=alice OP_ID=0 ./scripts/start-operation.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
case "$NETWORK" in
  testnet)
    : "${FACTORY_ID:=CAR5T7YSAG5WH37X7V3ASJNXLN57CLYC3BAXUF2YIJ2GOOZJ6PPOWEEF}"
    ;;
  # TODO: change this when mainnet
  mainnet|pubnet|public)
    : "${FACTORY_ID:=CAR5T7YSAG5WH37X7V3ASJNXLN57CLYC3BAXUF2YIJ2GOOZJ6PPOWEEF}"
    ;;
esac

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req FACTORY_ID
req OP_ID

echo "==> Starting operation $OP_ID on $FACTORY_ID ($NETWORK)..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- start_operation \
  --id "$OP_ID"

echo "==> operation $OP_ID started"
