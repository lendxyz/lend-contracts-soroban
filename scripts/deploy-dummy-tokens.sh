#!/usr/bin/env bash
#
# Deploy a dummy testnet token — DummyUSDC or DummyEURC, stand-ins for Circle
# USDC / EURC. Both are the same DummyUSDC wasm with different metadata.
#
# Builds the wasm and deploys it with its constructor (admin + metadata), then
# mints 10M tokens to the admin. Anyone can `mint`. No transfer restrictions.
#
# Note: the seed mint is signed by SOURCE, so it only succeeds when ADMIN is the
# SOURCE identity's address (the default). Override ADMIN and the mint is
# skipped-with-error unless that address signs.
#
# Network and signer come from scripts/common.sh; override in the env.
#
# Required env vars:
#   ASSET      Which dummy token to deploy: USDC or EURC. Picks the default
#              NAME / SYMBOL and the common.sh variable to record the id in
#              (DUMMY_USDC_ID / DUMMY_EURC_ID).
#
# Optional env vars:
#   ADMIN      Contract admin/minter (default: address of SOURCE).
#   DECIMAL    Token decimals (default: 7). Mainnet Circle USDC and EURC are
#              classic assets behind SACs and report 7, and the factory reads
#              whichever value its tokens declare.
#   NAME       Token name (default: "Dummy USD Coin" / "Dummy EUR Coin").
#   SYMBOL     Token symbol (default: "dUSDC" / "dEURC").
#
# Usage:
#   ASSET=USDC ./scripts/deploy-dummy-tokens.sh
#   ASSET=EURC ./scripts/deploy-dummy-tokens.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

DUMMY_TOKEN_WASM="$WASM_DIR/dummy_usdc.wasm"

req ASSET
case "$ASSET" in
  USDC)
    TOKEN_VAR=DUMMY_USDC_ID
    : "${NAME:=Dummy USD Coin}"
    : "${SYMBOL:=dUSDC}"
    ;;
  EURC)
    TOKEN_VAR=DUMMY_EURC_ID
    : "${NAME:=Dummy EUR Coin}"
    : "${SYMBOL:=dEURC}"
    ;;
  *)
    echo "error: unknown ASSET '$ASSET' (expected USDC | EURC)" >&2
    exit 1
    ;;
esac

req SOURCE

ADMIN="${ADMIN:-$(source_address)}"
DECIMAL="${DECIMAL:-7}"

echo "==> Network:  $NETWORK"
echo "==> Source:   $SOURCE"
echo "==> Admin:    $ADMIN"
echo "==> Metadata: $NAME ($SYMBOL), $DECIMAL decimals"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

echo "==> Deploying $SYMBOL..."
TOKEN_ID="$(stellar contract deploy \
  --wasm "$DUMMY_TOKEN_WASM" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- \
  --admin "$ADMIN" \
  --decimal "$DECIMAL" \
  --name "$NAME" \
  --symbol "$SYMBOL" | tail -n1)"

# Seed the deployer/admin with 10M tokens (scaled by DECIMAL).
MINT_WHOLE=10000000
MINT_AMOUNT="${MINT_WHOLE}$(printf '0%.0s' $(seq 1 "$DECIMAL"))"
echo "==> Minting ${MINT_WHOLE} $SYMBOL to admin ($MINT_AMOUNT base units)..."
stellar contract invoke \
  --id "$TOKEN_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- \
  mint \
  --to "$ADMIN" \
  --amount "$MINT_AMOUNT"

echo ""
echo "==> Done."
echo "    $TOKEN_VAR=$TOKEN_ID"
echo "    Minted ${MINT_WHOLE} $SYMBOL to $ADMIN"
echo "    # record it in DEPLOYMENTS.md and in scripts/common.sh"
echo ""
echo "    # mint more:"
echo "    ASSET=$ASSET $TOKEN_VAR=$TOKEN_ID TO=<ADDR> AMOUNT_WHOLE=1000 ./scripts/fund-dummy-tokens.sh"
