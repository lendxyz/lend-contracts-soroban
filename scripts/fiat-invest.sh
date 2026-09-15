#!/usr/bin/env bash
#
# Settle an off-chain (fiat) investment on chain: mint op-lend shares to a
# holder without moving USDC. `fiat_invest` has no on-chain auth — the backend
# ed25519 signature is the only authorisation — so this script signs the
# FIAT_INVEST payload locally with the backend signer key and submits the call
# in one step:
#
#   1. read the backend signer's secret from the Stellar CLI keystore
#      ($BACKEND_SIGNER_KEY, e.g. lend-mainnet-signer)
#   2. build + sign the payload with scripts/sign-fiat-invest.js
#      ("FIAT_INVEST" || factory || id || user || holder || shares || nonce)
#   3. check the derived pubkey equals the network's $BACKEND_SIGNER
#   4. call fiat_invest(id, shares_amount, user, oplend_holder, nonce, signature)
#
# $SOURCE only pays the fee and signs the transaction; it is not checked by the
# contract. Shares go to $HOLDER, while $INVESTOR is the address recorded in the
# events and whitelisted on the op-lend token (fiat leaves no Position, so it is
# not refundable — see docs/threat-model-stride.md DoS.1).
#
# Network, signer and FACTORY_ID come from scripts/common.sh; override in the env.
#
# Required env vars:
#   OP_ID     Operation id (integer).
#   SHARES    Shares to mint (integer, 6 decimals).
#   INVESTOR  The fiat investor (G...); recorded in events, whitelisted.
#
# Optional env vars:
#   HOLDER             Address the shares are minted to (G... or C...);
#                      defaults to the network's $FIAT_HOLDER (common.sh) —
#                      the custodial address, not the investor.
#   NONCE              Replay nonce (printable ASCII, no spaces). Defaults to a
#                      generated fiat-<op>-<ts>-<rand>; nonces are consumed
#                      permanently, so a rerun needs a fresh one.
#   BACKEND_SIGNER_KEY Stellar CLI identity holding the backend signer secret
#                      (default per NETWORK in common.sh).
#   DRY_RUN            1 to simulate only (`--send=no`): nothing is submitted
#                      and the nonce stays unused. Recommended before every
#                      real run — simulation runs the same ed25519 check and
#                      funding guards the submitted call would.
#
# Usage:
#   OP_ID=1 SHARES=1000000 INVESTOR=G... ./scripts/fiat-invest.sh
#   NETWORK=mainnet OP_ID=1 SHARES=1000000 INVESTOR=G... HOLDER=G... \
#     ./scripts/fiat-invest.sh
#   DRY_RUN=1 OP_ID=1 SHARES=1000000 INVESTOR=G... ./scripts/fiat-invest.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE FACTORY_ID OP_ID SHARES INVESTOR BACKEND_SIGNER BACKEND_SIGNER_KEY \
  FIAT_HOLDER
need_bin node jq stellar

HOLDER="${HOLDER:-$FIAT_HOLDER}"
DRY_RUN="${DRY_RUN:-0}"

# --send=no stops after simulation. Without it the CLI submits, because
# fiat_invest writes ledger state.
SEND_ARGS=()
[ "$DRY_RUN" = "1" ] && SEND_ARGS=(--send=no)

echo "==> Network:  $NETWORK" >&2
echo "==> Factory:  $FACTORY_ID" >&2
echo "==> Operation: $OP_ID" >&2
echo "==> Shares:   $SHARES" >&2
echo "==> Investor: $INVESTOR" >&2
echo "==> Holder:   $HOLDER" >&2

# 1 + 2. Sign the payload. The secret goes to the signer over the environment,
# never in argv; `stellar keys secret` fails loudly for a ledger identity.
echo "==> Signing FIAT_INVEST payload with '$BACKEND_SIGNER_KEY'..." >&2
PROOF="$(
  SIGNER_SECRET="$(stellar keys secret "$BACKEND_SIGNER_KEY")" \
  FACTORY_ID="$FACTORY_ID" \
  OP_ID="$OP_ID" \
  SHARES="$SHARES" \
  INVESTOR="$INVESTOR" \
  HOLDER="$HOLDER" \
  NONCE="${NONCE:-}" \
  node "$SCRIPT_DIR/sign-fiat-invest.js"
)"

NONCE="$(jq -r '.nonce' <<<"$PROOF")"
SIGNATURE="$(jq -r '.signature' <<<"$PROOF")"
SIGNER_HEX="$(jq -r '.signer_hex' <<<"$PROOF")"

# 3. The contract verifies against the signer stored at initialize /
# update_backend_signer. common.sh mirrors it per network, so a mismatch here
# means the wrong key — catch it before burning a nonce on a failed call.
EXPECTED_HEX="$(strkey_to_hex "$BACKEND_SIGNER")"
if [ "$SIGNER_HEX" != "$EXPECTED_HEX" ]; then
  echo "error: '$BACKEND_SIGNER_KEY' is not the backend signer for $NETWORK." >&2
  echo "       derived:  $SIGNER_HEX" >&2
  echo "       expected: $EXPECTED_HEX  (\$BACKEND_SIGNER=$BACKEND_SIGNER)" >&2
  echo "       Set BACKEND_SIGNER_KEY to the right identity, or BACKEND_SIGNER" >&2
  echo "       if the factory was updated with update-backend-signer.sh." >&2
  exit 1
fi

echo "==> Nonce:    $NONCE" >&2
echo "==> Signer:   $BACKEND_SIGNER ($BACKEND_SIGNER_KEY)" >&2

# 4. Simulate, or submit. No require_auth on this path, so $SOURCE is only the
# fee payer; with --send=no it is just the simulation's source account.
stellar contract invoke \
  --id "$FACTORY_ID" \
  --source "$SOURCE" \
  "${NETWORK_ARGS[@]}" \
  "${SIGN_ARGS[@]}" \
  "${SEND_ARGS[@]+"${SEND_ARGS[@]}"}" \
  -- fiat_invest \
  --id "$OP_ID" \
  --shares_amount "$SHARES" \
  --user "$INVESTOR" \
  --oplend_holder "$HOLDER" \
  --nonce "$NONCE" \
  --signature "$SIGNATURE"

echo "" >&2
if [ "$DRY_RUN" = "1" ]; then
  echo "==> Simulated only (DRY_RUN=1); nothing submitted, nonce $NONCE unused." >&2
  echo "    Rerun without DRY_RUN to mint $SHARES shares of operation $OP_ID." >&2
else
  echo "==> Done. Minted $SHARES shares of operation $OP_ID to $HOLDER" >&2
  echo "    investor=$INVESTOR nonce=$NONCE" >&2
fi
