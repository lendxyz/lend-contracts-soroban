#!/usr/bin/env bash
#
# Fund an address with DummyUSDC. `mint` is open to anyone, so any SOURCE
# identity can top up any address on testnet.
#
# Network, signer and DUMMY_USDC_ID come from scripts/common.sh (testnet only —
# there is no DummyUSDC on mainnet); override in the env.
#
# Required env vars:
#   TO             Address to fund (G...).
#
# Optional env vars:
#   AMOUNT_WHOLE   Whole tokens to mint (default: 10000); scaled by DECIMAL.
#   DECIMAL        Token decimals (default: 6).
#
# Usage:
#   TO=G... AMOUNT_WHOLE=5000 ./scripts/fund-dummy-usdc.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

DECIMAL="${DECIMAL:-6}"
AMOUNT_WHOLE="${AMOUNT_WHOLE:-10000}"

req SOURCE DUMMY_USDC_ID TO

AMOUNT="${AMOUNT_WHOLE}$(printf '0%.0s' $(seq 1 "$DECIMAL"))"

echo "==> Minting $AMOUNT_WHOLE dUSDC ($AMOUNT base units) to $TO on $NETWORK..."
stellar contract invoke \
  --id "$DUMMY_USDC_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- mint \
  --to "$TO" \
  --amount "$AMOUNT"

echo "==> funded $TO with $AMOUNT_WHOLE dUSDC"
