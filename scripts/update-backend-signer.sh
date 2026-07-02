#!/usr/bin/env bash
#
# Update the factory's backend signer (the ed25519 key that authorizes invest /
# predeposit / fiat-invest messages). Admin-only (SOURCE must be the factory
# admin).
#
# Required env vars:
#   SOURCE          Stellar CLI identity (must be the factory admin).
#   FACTORY_ID      Deployed factory contract address (C...).
#   BACKEND_SIGNER  New backend ed25519 public key: 64 hex chars or a G... strkey.
#
# Optional env vars:
#   NETWORK         Network name (default: testnet).
#
# Usage:
#   SOURCE=alice BACKEND_SIGNER=GAOQ67SJ... ./scripts/update-backend-signer.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
case "$NETWORK" in
  testnet)
    : "${FACTORY_ID:=CCLLIO5MTHXQTLL5EEE4C5ECX4MHMFSWDF225R64MQ62BE5MS7TTZTX3}"
    ;;
  # TODO: change this when mainnet
  mainnet|pubnet|public)
    : "${FACTORY_ID:=CAR5T7YSAG5WH37X7V3ASJNXLN57CLYC3BAXUF2YIJ2GOOZJ6PPOWEEF}"
    ;;
esac

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req FACTORY_ID
req BACKEND_SIGNER

# new_signer is BytesN<32>, so the CLI needs 64 hex chars. Accept a G... strkey
# for convenience and decode it to the raw 32-byte ed25519 pubkey
# (strkey = version byte + 32-byte payload + 2-byte crc).
if [[ "$BACKEND_SIGNER" == G* ]]; then
  BACKEND_SIGNER="$(python3 - "$BACKEND_SIGNER" <<'PY'
import base64, sys
s = sys.argv[1]
raw = base64.b32decode(s + "=" * ((8 - len(s) % 8) % 8))
sys.stdout.write(raw[1:33].hex())
PY
)"
fi

echo "==> Updating backend signer on $FACTORY_ID ($NETWORK) to $BACKEND_SIGNER..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- update_backend_signer \
  --new_signer "$BACKEND_SIGNER"

echo "==> backend signer updated"
