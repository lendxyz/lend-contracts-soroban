#!/usr/bin/env bash
#
# Invest in a started operation. The caller (SOURCE) is the investor and pays
# USDC; the backend signature + nonce are supplied by the caller.
#
# The signature must cover the contract's build_invest_message:
#   "ONCHAIN_INVEST" || factory_addr || id(u32 BE) || user_addr || shares(i128 BE) || nonce
# signed by the backend signer ed25519 key (passed as 64-byte hex).
#
# Required env vars:
#   SOURCE          Stellar CLI identity of the investor (signs tx + pays USDC).
#   FACTORY_ID      Deployed factory contract address (C...).
#   OP_ID           Operation id (integer).
#   SHARES          Shares to buy (integer, 6 decimals).
#   NONCE           Replay nonce (must match what the signature was built with).
#   SIGNATURE       Backend ed25519 signature, 64-byte hex (0x prefix optional).
#
# Optional env vars:
#   NETWORK         Network name (default: testnet).
#   INVESTOR        Investor address (G...); defaults to `stellar keys address $SOURCE`.
#
# Usage:
#   SOURCE=alice OP_ID=0 SHARES=100 NONCE=abc SIGNATURE=deadbeef... ./scripts/invest.sh
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
req SHARES
req NONCE
req SIGNATURE

INVESTOR="${INVESTOR:-$(stellar keys address "$SOURCE")}"
# stellar CLI wants bare hex for BytesN<64>; the API returns it 0x-prefixed.
SIGNATURE="${SIGNATURE#0x}"

echo "==> Investing in operation $OP_ID on $FACTORY_ID ($NETWORK) as $INVESTOR..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- invest \
  --user "$INVESTOR" \
  --id "$OP_ID" \
  --shares_amount "$SHARES" \
  --nonce "$NONCE" \
  --signature "$SIGNATURE"

echo "==> invested $SHARES shares in operation $OP_ID (nonce $NONCE)"
