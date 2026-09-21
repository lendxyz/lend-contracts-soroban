#!/usr/bin/env bash
#
# Register an operation's op-lend token on the OpLendWallet, so a redeem knows
# which token to transfer. Admin-only (SOURCE must be the wallet admin).
#
# The wallet holds shares of many operations at once and is told about each one
# explicitly: `redeem`/`whitelist_and_redeem` look the operation id up in this
# map, and an unregistered id fails on chain before any transfer. The map takes
# contract (`C...`) addresses only — an op-lend token is a contract, so a `G...`
# account is rejected on chain rather than silently stored.
#
# Re-registering an id overwrites it, which is how a redeployed op-lend token is
# pointed at.
#
# Network, signer and OPLEND_WALLET_ID come from scripts/common.sh; override in
# the env.
#
# Required env vars:
#   OP_ID   Operation id (integer).
#   OPLEND  That operation's op-lend token contract (C...).
#
# Optional env vars:
#   ENTRIES JSON array of {op_id, op_lend} objects. When set, one
#           register_op_lends call registers the whole batch and OP_ID/OPLEND
#           are ignored — cheaper than one transaction per operation.
#
# Usage:
#   OP_ID=1 OPLEND=C... ./scripts/wallet-register-op-lend.sh
#   ENTRIES='[{"op_id":1,"op_lend":"C..."},{"op_id":2,"op_lend":"C..."}]' \
#     ./scripts/wallet-register-op-lend.sh
#   NETWORK=mainnet OP_ID=1 OPLEND=C... ./scripts/wallet-register-op-lend.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE OPLEND_WALLET_ID

echo "==> Network: $NETWORK"
echo "==> Wallet:  $OPLEND_WALLET_ID"
echo "==> Source:  $SOURCE"

if [ -n "${ENTRIES:-}" ]; then
  echo "==> Registering a batch of op-lend tokens..."
  echo "    entries: $ENTRIES"
  stellar contract invoke \
    --id "$OPLEND_WALLET_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- register_op_lends \
    --entries "$ENTRIES"
else
  req OP_ID OPLEND
  echo "==> Registering operation $OP_ID -> $OPLEND..."
  stellar contract invoke \
    --id "$OPLEND_WALLET_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    -- register_op_lend \
    --op_id "$OP_ID" \
    --op_lend "$OPLEND"
fi

echo "==> op-lend registration updated"
