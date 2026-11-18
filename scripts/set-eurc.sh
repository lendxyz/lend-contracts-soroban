#!/usr/bin/env bash
#
# Set the EURC token the factory's `invest_eur` accepts. Admin-only (SOURCE must
# be the factory admin) and write-once: the factory caches the token's decimals
# and denominates every EURC escrow in it, so it can never be changed after.
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Optional env vars:
#   EURC      EURC token contract (C...). Defaults to the network's EURC in
#             common.sh (mock EURC on testnet, Circle EURC SAC on mainnet).
#   DRY_RUN   1 to simulate only (`--send=no`): nothing is submitted.
#
# Usage:
#   ./scripts/set-eurc.sh
#   DRY_RUN=1 NETWORK=mainnet ./scripts/set-eurc.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID EURC

DRY_RUN="${DRY_RUN:-0}"
SEND_ARGS=()
[ "$DRY_RUN" = "1" ] && SEND_ARGS=(--send=no)

echo "==> Network: $NETWORK" >&2
echo "==> Factory: $FACTORY_ID" >&2
echo "==> EURC:    $EURC" >&2

# set_eurc is write-once, so check the current value before signing anything.
# The getter fails with NotInitialized (Error(Contract, #2)) while unset; a
# factory running a wasm older than the EURC feature has no `eurc` at all.
if CURRENT="$(stellar contract invoke \
  --id "$FACTORY_ID" --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
  -- eurc 2>&1)"; then
  CURRENT="$(tr -d '"' <<<"$CURRENT")"
  if [ "$CURRENT" = "$EURC" ]; then
    echo "==> EURC already set to $EURC; nothing to do" >&2
    exit 0
  fi
  echo "error: factory EURC is already $CURRENT and is write-once" >&2
  exit 1
elif ! grep -q 'Error(Contract, #2)' <<<"$CURRENT"; then
  echo "$CURRENT" >&2
  echo "error: could not read the factory's EURC; if 'eurc' is an unrecognized" >&2
  echo "       subcommand, upgrade the factory wasm first (upgrade-contract.sh)" >&2
  exit 1
fi

stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  "${SEND_ARGS[@]}" \
  -- set_eurc \
  --eurc "$EURC"

if [ "$DRY_RUN" = "1" ]; then
  echo "==> Simulated only (DRY_RUN=1); nothing submitted." >&2
else
  echo "==> Factory EURC set to $EURC" >&2
fi
