#!/usr/bin/env bash
#
# Deploy the DummyUSDC token — a testnet stand-in for Circle USDC.
#
# Builds the wasm and deploys it with its constructor (admin + metadata), then
# mints 10M tokens to the admin. Admin can `mint`; anyone can `faucet`. No
# transfer restrictions.
#
# Note: the seed mint is signed by SOURCE, so it only succeeds when ADMIN is the
# SOURCE identity's address (the default). Override ADMIN and the mint is
# skipped-with-error unless that address signs.
#
# Required env vars:
#   SOURCE     Stellar CLI identity used to sign + pay.
#
# Optional env vars:
#   NETWORK    testnet | mainnet (default: testnet).
#   ADMIN      Contract admin/minter (default: address of SOURCE).
#   DECIMAL    Token decimals (default: 6, matching USDC).
#   NAME       Token name (default: "Dummy USD Coin").
#   SYMBOL     Token symbol (default: "dUSDC").
#
# Usage:
#   SOURCE=alice ./scripts/deploy-dummy-usdc.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DUMMY_USDC_WASM="$REPO_ROOT/target/wasm32v1-none/release/dummy_usdc.wasm"

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE

ADMIN="${ADMIN:-$(stellar keys address "$SOURCE")}"
DECIMAL="${DECIMAL:-6}"
NAME="${NAME:-Dummy USD Coin}"
SYMBOL="${SYMBOL:-dUSDC}"

echo "==> Network:  $NETWORK"
echo "==> Source:   $SOURCE"
echo "==> Admin:    $ADMIN"
echo "==> Metadata: $NAME ($SYMBOL), $DECIMAL decimals"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

echo "==> Deploying DummyUSDC..."
DUMMY_USDC_ID="$(stellar contract deploy \
  --wasm "$DUMMY_USDC_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" \
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
  --id "$DUMMY_USDC_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- \
  mint \
  --to "$ADMIN" \
  --amount "$MINT_AMOUNT"

echo ""
echo "==> Done."
echo "    DUMMY_USDC_ID=$DUMMY_USDC_ID"
echo "    Minted ${MINT_WHOLE} $SYMBOL to $ADMIN"
echo ""
echo "    # admin mint more:"
echo "    stellar contract invoke --id \$DUMMY_USDC_ID --source $SOURCE --network $NETWORK -- mint --to <ADDR> --amount 1000000000"
echo "    # open faucet (anyone):"
echo "    stellar contract invoke --id \$DUMMY_USDC_ID --source $SOURCE --network $NETWORK -- faucet --to <ADDR> --amount 1000000000"
