#!/usr/bin/env bash
#
# Point the OpLendWallet at the factory whose `backend_signer` authorizes
# redeems. Admin-only (SOURCE must be the wallet admin).
#
# The wallet reads the backend signer from that factory on every redeem. Run
# this once after upgrading a wallet that predates it (it stored the signer
# itself and has no factory yet, so redeems fail until this is done), and
# whenever the factory is redeployed. The wallet only accepts an address that
# answers `backend_signer()` (an account or unrelated contract is refused).
#
# Network, signer, OPLEND_WALLET_ID and FACTORY_ID come from scripts/common.sh;
# override in the env.
#
# Usage:
#   ./scripts/wallet-set-factory.sh
#   FACTORY_ID=C... NETWORK=mainnet ./scripts/wallet-set-factory.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE OPLEND_WALLET_ID FACTORY_ID

echo "==> Network: $NETWORK"
echo "==> Wallet:  $OPLEND_WALLET_ID"
echo "==> Factory: $FACTORY_ID"
echo "==> Source:  $SOURCE"

stellar contract invoke \
  --id "$OPLEND_WALLET_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  -- set_factory \
  --factory "$FACTORY_ID"

echo "==> Verifying..."
CURRENT="$(stellar contract invoke --id "$OPLEND_WALLET_ID" --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" --send=no -- factory | tr -d '"')"
if [ "$CURRENT" != "$FACTORY_ID" ]; then
  echo "error: wallet still points at $CURRENT" >&2
  exit 1
fi
echo "==> Wallet reads the backend signer from $FACTORY_ID"
