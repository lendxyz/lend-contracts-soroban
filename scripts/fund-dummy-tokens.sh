#!/usr/bin/env bash
#
# Fund an address with a dummy testnet token (DummyUSDC or DummyEURC). `mint` is
# open to anyone, so any SOURCE identity can top up any address on testnet.
#
# Network, signer and DUMMY_USDC_ID / DUMMY_EURC_ID come from scripts/common.sh
# (testnet only — there are no dummy tokens on mainnet); override in the env.
#
# Required env vars:
#   ASSET          Which dummy token to mint: USDC (DUMMY_USDC_ID) or EURC
#                  (DUMMY_EURC_ID).
#   TO             Address to fund (G...).
#
# Optional env vars:
#   AMOUNT_WHOLE   Whole tokens to mint (default: 10000); scaled by DECIMAL.
#   DECIMAL        Token decimals (default: read off the contract).
#
# Usage:
#   ASSET=USDC TO=G... AMOUNT_WHOLE=5000 ./scripts/fund-dummy-tokens.sh
#   ASSET=EURC TO=G... ./scripts/fund-dummy-tokens.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

AMOUNT_WHOLE="${AMOUNT_WHOLE:-10000}"

req ASSET
case "$ASSET" in
  USDC) TOKEN_VAR=DUMMY_USDC_ID ;;
  EURC) TOKEN_VAR=DUMMY_EURC_ID ;;
  *)
    echo "error: unknown ASSET '$ASSET' (expected USDC | EURC)" >&2
    exit 1
    ;;
esac

req SOURCE "$TOKEN_VAR" TO
TOKEN_ID="${!TOKEN_VAR}"

SYMBOL="$(stellar contract invoke \
  --id "$TOKEN_ID" --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
  -- symbol | tr -d '"')"

# Read the scale off the token rather than assuming it: new dummy deploys use 7
# decimals to mirror Circle's USDC / EURC, while the DummyUSDC deployed earlier
# is still 6 — guessing either way mints or starves by 10x.
DECIMAL="${DECIMAL:-$(stellar contract invoke \
  --id "$TOKEN_ID" --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
  -- decimals)}"

AMOUNT="${AMOUNT_WHOLE}$(printf '0%.0s' $(seq 1 "$DECIMAL"))"

echo "==> Minting $AMOUNT_WHOLE $SYMBOL ($AMOUNT base units) to $TO on $NETWORK..."
stellar contract invoke \
  --id "$TOKEN_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- mint \
  --to "$TO" \
  --amount "$AMOUNT"

echo "==> funded $TO with $AMOUNT_WHOLE $SYMBOL"
