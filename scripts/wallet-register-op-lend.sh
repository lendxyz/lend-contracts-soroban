#!/usr/bin/env bash
#
# Register an operation's op-lend token on the OpLendWallet, so a redeem knows
# which token to transfer. Admin-only (SOURCE must be the wallet admin).
#
# The wallet holds shares of many operations at once and is told about each one
# explicitly: `redeem`/`whitelist_and_redeem` look the operation id up in this
# map, and an unregistered id fails on chain before any transfer. Only the id is
# passed: the wallet copies the token from its factory's `get_operation`, so an
# operation can never be mapped to another operation's token, and an id the
# factory does not know is refused on chain.
#
# Re-registering an id refreshes it from the factory (e.g. after pointing the
# wallet at a new factory with scripts/wallet-set-factory.sh).
#
# Network, signer and OPLEND_WALLET_ID come from scripts/common.sh; override in
# the env.
#
# Required env vars:
#   OP_ID   Operation id (integer).
#
# Optional env vars:
#   OP_IDS  JSON array of operation ids. When set, one register_op_lends call
#           registers the whole batch and OP_ID is ignored — cheaper than one
#           transaction per operation.
#
# Usage:
#   OP_ID=1 ./scripts/wallet-register-op-lend.sh
#   OP_IDS='[1,2,3]' ./scripts/wallet-register-op-lend.sh
#   NETWORK=mainnet OP_ID=1 ./scripts/wallet-register-op-lend.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE OPLEND_WALLET_ID

echo "==> Network: $NETWORK"
echo "==> Wallet:  $OPLEND_WALLET_ID"
echo "==> Source:  $SOURCE"

if [ -n "${OP_IDS:-}" ]; then
  echo "==> Registering operations $OP_IDS from the factory..."
  stellar contract invoke \
    --id "$OPLEND_WALLET_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- register_op_lends \
    --op_ids "$OP_IDS"
else
  req OP_ID
  echo "==> Registering operation $OP_ID from the factory..."
  stellar contract invoke \
    --id "$OPLEND_WALLET_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- register_op_lend \
    --op_id "$OP_ID"
fi

echo "==> op-lend registration updated"
