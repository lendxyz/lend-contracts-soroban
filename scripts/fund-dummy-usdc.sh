#!/usr/bin/env bash
#
# Fund an address with DummyUSDC. `mint` is open to anyone, so any SOURCE
# identity can top up any address on testnet.
#
# Required env vars:
#   SOURCE         Stellar CLI identity (signs the tx).
#   TO             Address to fund (G...).
#
# Optional env vars:
#   NETWORK        Network name (default: testnet).
#   DUMMY_USDC_ID  Deployed DummyUSDC contract address (C...); defaults per NETWORK.
#   AMOUNT_WHOLE   Whole tokens to mint (default: 10000); scaled by DECIMAL.
#   DECIMAL        Token decimals (default: 6).
#
# Usage:
#   SOURCE=alice DUMMY_USDC_ID=CC... TO=G... AMOUNT_WHOLE=5000 ./scripts/fund-dummy-usdc.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
DECIMAL="${DECIMAL:-6}"
AMOUNT_WHOLE="${AMOUNT_WHOLE:-10000}"

case "$NETWORK" in
  testnet)
    : "${DUMMY_USDC_ID:=CCO56ZVZPLGELBZGAVLTNC5GPZUIF4SIAIGPYNHWBRUSKBLC7HPF5QPN}"
    ;;
esac

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req DUMMY_USDC_ID
req TO

AMOUNT="${AMOUNT_WHOLE}$(printf '0%.0s' $(seq 1 "$DECIMAL"))"

echo "==> Minting $AMOUNT_WHOLE dUSDC ($AMOUNT base units) to $TO on $NETWORK..."
stellar contract invoke \
  --id "$DUMMY_USDC_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- mint \
  --to "$TO" \
  --amount "$AMOUNT"

echo "==> funded $TO with $AMOUNT_WHOLE dUSDC"
