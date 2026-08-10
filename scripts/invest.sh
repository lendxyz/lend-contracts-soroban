#!/usr/bin/env bash
#
# Invest in a started operation. The caller (SOURCE) is the investor and pays
# USDC; the backend signature + nonce are supplied by the caller.
#
# The signature must cover the contract's build_invest_message:
#   "ONCHAIN_INVEST" || factory_addr || id(u32 BE) || user_addr || shares(i128 BE) || nonce
# signed by the backend signer ed25519 key (passed as 64-byte hex).
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Required env vars:
#   OP_ID           Operation id (integer).
#   SHARES          Shares to buy (integer, 6 decimals).
#   NONCE           Replay nonce (must match what the signature was built with).
#   SIGNATURE       Backend ed25519 signature, 64-byte hex (0x prefix optional).
#
# Optional env vars:
#   INVESTOR        Investor address (G...); defaults to the address of SOURCE.
#
# Usage:
#   OP_ID=0 SHARES=100 NONCE=abc SIGNATURE=deadbeef... ./scripts/invest.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID OP_ID SHARES NONCE SIGNATURE

INVESTOR="${INVESTOR:-$(source_address)}"
# stellar CLI wants bare hex for BytesN<64>; the API returns it 0x-prefixed.
SIGNATURE="${SIGNATURE#0x}"

echo "==> Investing in operation $OP_ID on $FACTORY_ID ($NETWORK) as $INVESTOR..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- invest \
  --user "$INVESTOR" \
  --id "$OP_ID" \
  --shares_amount "$SHARES" \
  --nonce "$NONCE" \
  --signature "$SIGNATURE"

echo "==> invested $SHARES shares in operation $OP_ID (nonce $NONCE)"
