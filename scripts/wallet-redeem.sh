#!/usr/bin/env bash
#
# Release op-lend shares the OpLendWallet holds for a fiat investor to a real
# user wallet. `redeem` has no on-chain auth — the backend ed25519 signature
# plus a single-use nonce is the only authorisation, exactly like the factory's
# fiat_invest — so this script signs the OP_REDEEM payload locally with the
# backend signer key and submits the call in one step:
#
#   1. read the backend signer's secret from the Stellar CLI keystore
#      ($BACKEND_SIGNER_KEY, e.g. lend-mainnet-signer)
#   2. build + sign the payload with scripts/sign-redeem.js
#      ("OP_REDEEM" || wallet || op_id || destination || amount || nonce)
#   3. check the derived pubkey equals the network's $BACKEND_SIGNER
#   4. call redeem(op_id, destination, amount, nonce, signature)
#
# With WHITELIST=1 it instead calls whitelist_and_redeem, which whitelists the
# destination on the op-lend token and transfers in the same transaction.
# op-lend's `transfer` requires *both* sides whitelisted, so a wallet that has
# never touched the operation cannot be paid without it. That leg is a second
# signature, over the op token's own whitelist message (oplend || user ||
# nonce), and the op token address is read off the wallet's registry rather than
# passed in — a mismatch there would spend a nonce for nothing.
#
# $SOURCE only pays the fee and signs the transaction; it is not checked by the
# contract. The operation must be registered first with
# scripts/wallet-register-op-lend.sh, and the wallet must actually hold the shares —
# its balance is printed before submitting for exactly that reason.
#
# Network, signer and OPLEND_WALLET_ID come from scripts/common.sh; override in
# the env.
#
# Required env vars:
#   OP_ID        Operation id (integer).
#   DESTINATION  Address the shares are released to (G... or C...).
#   AMOUNT       Shares to release (integer, 6 decimals).
#
# Optional env vars:
#   WHITELIST          1 to whitelist $DESTINATION on the op-lend token in the
#                      same call (whitelist_and_redeem). Needed for a fresh
#                      user wallet; harmless but wasteful once whitelisted.
#   NONCE              Replay nonce for the redeem leg (printable ASCII, no
#                      spaces). Defaults to a generated redeem-<op>-<ts>-<rand>;
#                      nonces are consumed permanently, so a rerun needs a fresh
#                      one.
#   WL_NONCE           Replay nonce for the whitelist leg (WHITELIST=1 only).
#                      Defaults to a generated wl-<op>-<ts>-<rand>. The op
#                      token keeps its own nonce store, so it never clashes
#                      with NONCE.
#   BACKEND_SIGNER_KEY Stellar CLI identity holding the backend signer secret
#                      (default per NETWORK in common.sh).
#   DRY_RUN            1 to simulate only (`--send=no`): nothing is submitted
#                      and neither nonce is spent. Recommended before every
#                      real run — simulation runs the same ed25519 checks,
#                      registry lookup and balance guards the submitted call
#                      would.
#
# Usage:
#   OP_ID=1 DESTINATION=G... AMOUNT=1000000 ./scripts/wallet-redeem.sh
#   WHITELIST=1 OP_ID=1 DESTINATION=G... AMOUNT=1000000 ./scripts/wallet-redeem.sh
#   DRY_RUN=1 WHITELIST=1 OP_ID=1 DESTINATION=G... AMOUNT=1000000 ./scripts/wallet-redeem.sh
#   NETWORK=mainnet OP_ID=1 DESTINATION=G... AMOUNT=1000000 ./scripts/wallet-redeem.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

req SOURCE OPLEND_WALLET_ID OP_ID DESTINATION AMOUNT BACKEND_SIGNER \
  BACKEND_SIGNER_KEY
need_bin node jq stellar

WHITELIST="${WHITELIST:-0}"
DRY_RUN="${DRY_RUN:-0}"

# --send=no stops after simulation. Without it the CLI submits, because redeem
# writes ledger state (the nonce and the token balances).
SEND_ARGS=()
[ "$DRY_RUN" = "1" ] && SEND_ARGS=(--send=no)

echo "==> Network:     $NETWORK" >&2
echo "==> Wallet:      $OPLEND_WALLET_ID" >&2
echo "==> Operation:   $OP_ID" >&2
echo "==> Destination: $DESTINATION" >&2
echo "==> Amount:      $AMOUNT" >&2
echo "==> Whitelist:   $([ "$WHITELIST" = "1" ] && echo "yes, in the same call" || echo no)" >&2

# The wallet can only pay out what it actually holds, and an under-funded wallet
# otherwise fails deep inside the token's transfer. --send=no keeps this a
# simulation (no signature, no ledger prompt).
BALANCE="$(stellar contract invoke \
  --id "$OPLEND_WALLET_ID" --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
  -- op_lend_balance --op_id "$OP_ID" | tr -d '"')"
echo "==> Held:        $BALANCE shares of operation $OP_ID" >&2

# The whitelist leg is signed for the op-lend token, so it needs that token's
# address. Read it from the wallet's own registry (the same entry redeem uses)
# instead of taking it on faith from the environment.
OPLEND_ID=""
if [ "$WHITELIST" = "1" ]; then
  OPLEND_ID="$(stellar contract invoke \
    --id "$OPLEND_WALLET_ID" --source "$SOURCE" "${NETWORK_ARGS[@]}" --send=no \
    -- op_lend --op_id "$OP_ID" | tr -d '"')"
  echo "==> op-lend:     $OPLEND_ID" >&2
fi

# 1 + 2. Sign the payload(s). The secret goes to the signer over the
# environment, never in argv; `stellar keys secret` fails loudly for a ledger
# identity.
echo "==> Signing OP_REDEEM payload with '$BACKEND_SIGNER_KEY'..." >&2
PROOF="$(
  SIGNER_SECRET="$(stellar keys secret "$BACKEND_SIGNER_KEY")" \
  WALLET_ID="$OPLEND_WALLET_ID" \
  OP_ID="$OP_ID" \
  DESTINATION="$DESTINATION" \
  AMOUNT="$AMOUNT" \
  NONCE="${NONCE:-}" \
  OPLEND_ID="$OPLEND_ID" \
  WL_NONCE="${WL_NONCE:-}" \
  node "$SCRIPT_DIR/sign-redeem.js"
)"

NONCE="$(jq -r '.nonce' <<<"$PROOF")"
SIGNATURE="$(jq -r '.signature' <<<"$PROOF")"
SIGNER_HEX="$(jq -r '.signer_hex' <<<"$PROOF")"

# 3. The contract verifies against the signer stored at deploy /
# update_backend_signer. common.sh mirrors it per network, so a mismatch here
# means the wrong key — catch it before burning a nonce on a failed call.
EXPECTED_HEX="$(strkey_to_hex "$BACKEND_SIGNER")"
if [ "$SIGNER_HEX" != "$EXPECTED_HEX" ]; then
  echo "error: '$BACKEND_SIGNER_KEY' is not the backend signer for $NETWORK." >&2
  echo "       derived:  $SIGNER_HEX" >&2
  echo "       expected: $EXPECTED_HEX  (\$BACKEND_SIGNER=$BACKEND_SIGNER)" >&2
  echo "       Set BACKEND_SIGNER_KEY to the right identity, or BACKEND_SIGNER" >&2
  echo "       if the wallet was updated with update_backend_signer." >&2
  exit 1
fi

echo "==> Nonce:       $NONCE" >&2
echo "==> Signer:      $BACKEND_SIGNER ($BACKEND_SIGNER_KEY)" >&2

# 4. Simulate, or submit. No require_auth on either path, so $SOURCE is only the
# fee payer; with --send=no it is just the simulation's source account.
if [ "$WHITELIST" = "1" ]; then
  WL_NONCE="$(jq -r '.whitelist.nonce' <<<"$PROOF")"
  WL_SIGNATURE="$(jq -r '.whitelist.signature' <<<"$PROOF")"
  echo "==> WL nonce:    $WL_NONCE" >&2

  stellar contract invoke \
    --id "$OPLEND_WALLET_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    "${SEND_ARGS[@]+"${SEND_ARGS[@]}"}" \
    -- whitelist_and_redeem \
    --op_id "$OP_ID" \
    --destination "$DESTINATION" \
    --amount "$AMOUNT" \
    --t_nonce "$WL_NONCE" \
    --t_signature "$WL_SIGNATURE" \
    --r_nonce "$NONCE" \
    --r_signature "$SIGNATURE"
else
  stellar contract invoke \
    --id "$OPLEND_WALLET_ID" \
    --source "$SOURCE" \
    "${NETWORK_ARGS[@]}" \
    "${SIGN_ARGS[@]}" \
    "${SEND_ARGS[@]+"${SEND_ARGS[@]}"}" \
    -- redeem \
    --op_id "$OP_ID" \
    --destination "$DESTINATION" \
    --amount "$AMOUNT" \
    --nonce "$NONCE" \
    --signature "$SIGNATURE"
fi

echo "" >&2
if [ "$DRY_RUN" = "1" ]; then
  echo "==> Simulated only (DRY_RUN=1); nothing submitted, nonce $NONCE unused." >&2
  echo "    Rerun without DRY_RUN to release $AMOUNT shares of operation $OP_ID." >&2
else
  echo "==> Done. Released $AMOUNT shares of operation $OP_ID to $DESTINATION" >&2
  echo "    nonce=$NONCE" >&2
fi
