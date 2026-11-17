#!/usr/bin/env bash
#
# Update the backend signer (the ed25519 key that authorizes invest /
# fiat-invest, op-lend whitelist and wallet redeem messages). It lives on the
# factory only: op-lend tokens and the op-lend wallet read it from there, so
# this one call rotates it everywhere. Admin-only (SOURCE must be the factory
# admin).
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Required env vars:
#   BACKEND_SIGNER  New backend ed25519 public key: 64 hex chars or a G... strkey
#                   (defaults to the network's BACKEND_SIGNER in common.sh).
#
# Usage:
#   BACKEND_SIGNER=GAOQ67SJ... ./scripts/update-backend-signer.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID BACKEND_SIGNER

# new_signer is BytesN<32>, so the CLI needs 64 hex chars; a G... strkey is
# decoded for convenience.
BACKEND_SIGNER="$(strkey_to_hex "$BACKEND_SIGNER")"

echo "==> Updating backend signer on $FACTORY_ID ($NETWORK) to $BACKEND_SIGNER..."
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- update_backend_signer \
  --new_signer "$BACKEND_SIGNER"

echo "==> backend signer updated"
