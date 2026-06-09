#!/usr/bin/env bash
#
# Deploy the Lend contracts to a Stellar network.
#
# Builds the wasms, uploads the op-lend wasm (factory deploys op-lend instances
# from its hash), deploys the factory, then calls `initialize`.
#
# Required env vars:
#   SOURCE          Stellar CLI identity (see `stellar keys ls`) used to sign + pay.
#   USDC            USDC token contract address (C...).
#   ORACLE          Reflector (SEP-40) oracle contract address (C...).
#   BACKEND_SIGNER  Backend ed25519 public key, 32 bytes as 64 hex chars.
#
# Optional env vars (sensible per-network defaults applied if unset):
#   NETWORK         Network name: testnet | mainnet (default: testnet).
#   USDC            USDC SAC; defaults to the network's Circle USDC.
#   ORACLE          Reflector FX oracle; defaults to the network's FX feed.
#   ADMIN           Factory admin address (default: address of SOURCE).
#
# Usage (testnet, defaults for USDC + oracle):
#   SOURCE=alice BACKEND_SIGNER=ab12... ./scripts/deploy.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"

# Verified 2026-06-02 (on-chain + Circle/Stellar docs). See scripts/README.md.
# Reflector FX oracle = the fiat/forex feed (base USD, decimals 14, carries EUR).
case "$NETWORK" in
  testnet)
    : "${USDC:=CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA}"
    : "${ORACLE:=CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W}"
    ;;
  mainnet|pubnet|public)
    : "${USDC:=CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75}"
    : "${ORACLE:=CBKGPWGKSKZF52CFHMTRR23TBWTPMRDIYZ4O2P5VS65BMHYH4DXMCJZC}"
    ;;
esac
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_DIR="$REPO_ROOT/target/wasm32v1-none/release"
OPLEND_WASM="$RELEASE_DIR/lend_operation_token.wasm"
FACTORY_WASM="$RELEASE_DIR/lend_factory.wasm"

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req USDC
req ORACLE
req BACKEND_SIGNER

ADMIN="${ADMIN:-$(stellar keys address "$SOURCE")}"

echo "==> Network:        $NETWORK"
echo "==> Source:         $SOURCE"
echo "==> Admin:          $ADMIN"
echo "==> USDC:           $USDC"
echo "==> Oracle:         $ORACLE"
echo "==> Backend signer: $BACKEND_SIGNER"

echo "==> Building wasms..."
(cd "$REPO_ROOT" && stellar contract build)

echo "==> Uploading op-lend wasm..."
OPLEND_WASM_HASH="$(stellar contract upload \
  --wasm "$OPLEND_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" | tail -n1)"
echo "    op-lend wasm hash: $OPLEND_WASM_HASH"

echo "==> Deploying factory..."
FACTORY_ID="$(stellar contract deploy \
  --wasm "$FACTORY_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" | tail -n1)"
echo "    factory id: $FACTORY_ID"

echo "==> Initializing factory..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN" \
  --usdc "$USDC" \
  --oracle "$ORACLE" \
  --backend_signer "$BACKEND_SIGNER" \
  --oplend_wasm_hash "$OPLEND_WASM_HASH"

echo ""
echo "==> Done."
echo "    FACTORY_ID=$FACTORY_ID"
echo "    OPLEND_WASM_HASH=$OPLEND_WASM_HASH"
