#!/usr/bin/env bash
#
# Withdraw the USDC an operation raised since the last withdrawal from the
# factory to DESTINATION. Admin-only (SOURCE must be the factory admin).
#
# Allowed any time after the operation starts, including during the sale, and
# repeatable: each call sends usdc_raised - usdc_withdrawn_amount.
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Required env vars:
#   OP_ID           Operation id (integer).
#   DESTINATION     Address (G... or C...) receiving the USDC.
#
# Optional env vars:
#   DRY_RUN         1 to simulate only (`--send=no`): nothing is submitted.
#
# Usage:
#   OP_ID=1 DESTINATION=G... ./scripts/withdraw-usdc.sh
#   DRY_RUN=1 OP_ID=1 DESTINATION=G... ./scripts/withdraw-usdc.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID OP_ID DESTINATION

DRY_RUN="${DRY_RUN:-0}"
SEND_ARGS=()
[ "$DRY_RUN" = "1" ] && SEND_ARGS=(--send=no)

# view NAME — read-only factory getter for $OP_ID. --send=no keeps it a
# simulation (no signature, no ledger prompt).
view() {
  stellar contract invoke \
    --id "$FACTORY_ID" --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
    -- "$1" --id "$OP_ID" | tr -d '"'
}

RAISED="$(view usdc_raised)"
WITHDRAWN="$(view usdc_withdrawn_amount)"
PENDING=$((RAISED - WITHDRAWN))

echo "==> Network:     $NETWORK" >&2
echo "==> Factory:     $FACTORY_ID" >&2
echo "==> Operation:   $OP_ID" >&2
echo "==> Destination: $DESTINATION" >&2
echo "==> Raised:      $RAISED (USDC base units)" >&2
echo "==> Withdrawn:   $WITHDRAWN" >&2
echo "==> Pending:     $PENDING" >&2

# The contract rejects this with NothingToWithdraw; fail before any signing.
if [ "$PENDING" -le 0 ]; then
  echo "error: nothing to withdraw for operation $OP_ID" >&2
  exit 1
fi

stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  "${SEND_ARGS[@]}" \
  -- withdraw_usdc \
  --id "$OP_ID" \
  --destination "$DESTINATION"

if [ "$DRY_RUN" = "1" ]; then
  echo "==> Simulated only (DRY_RUN=1); nothing submitted." >&2
else
  echo "==> Withdrew $PENDING USDC base units of operation $OP_ID to $DESTINATION" >&2
fi
