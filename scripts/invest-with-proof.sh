#!/usr/bin/env bash
#
# Fetch a mint-proof (backend signature + nonce) from the API and immediately
# invest with it. This wraps invest.sh end to end:
#
#   1. resolve the investor address from a Stellar CLI key name
#   2. POST /auth/message to get a sign-in message
#   3. sign that message (raw ed25519 over the message bytes) with the key
#   4. POST /auth/verify with the signature to obtain a JWT
#   5. POST /users/signatures/mint-proof (Bearer JWT) for the proof + nonce
#   6. forward the returned nonce + signature to invest.sh
#
# Required env vars:
#   SOURCE          Stellar CLI identity of the investor (signs tx + pays USDC,
#                   logs into the API, and whose address is the user address).
#   OP_ID           Operation id (integer). Sent as "op_id"; forwarded to invest.
#   AMOUNT          Shares to buy (integer, 6 decimals). Sent as "amount";
#                   forwarded to invest.sh as SHARES.
#
# Optional env vars:
#   NETWORK         Network name (default: testnet). Forwarded to invest.sh.
#   FACTORY_ID      Factory contract address (C...). Forwarded to invest.sh and,
#                   if set, sent to the API as "contract_id".
#   INVESTOR        Investor address (G...); defaults to `stellar keys address $SOURCE`.
#   API_BASE        API v1 root
#                   (default: https://lend-api-testnet-stellar.up.railway.app/v1).
#   JWT             Pre-obtained JWT; skips the /auth/message + /auth/verify flow.
#   INTERNAL_TOKEN  Value for the X-Internal-Token header (NuxtAuth; only needed
#                   off testnet, where that middleware is bypassed).
#
# Usage:
#   SOURCE=alice OP_ID=1 AMOUNT=1000000000 ./scripts/invest-with-proof.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
API_BASE="${API_BASE:-https://lend-api-testnet-stellar.up.railway.app/v1}"

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req OP_ID
req AMOUNT

for bin in jq curl node; do
  command -v "$bin" >/dev/null || { echo "error: $bin is required" >&2; exit 1; }
done

INVESTOR="${INVESTOR:-$(stellar keys address "$SOURCE")}"

# Common headers. NuxtAuthMiddleware is bypassed on testnet, but allow passing
# the internal token for non-testnet deployments.
HDRS=(-H "Content-Type: application/json")
[ -n "${INTERNAL_TOKEN:-}" ] && HDRS+=(-H "X-Internal-Token: ${INTERNAL_TOKEN}")

# api_post <url> <json> -> echoes body, fails with the raw body on non-200.
api_post() {
  local url="$1" payload="$2" resp http_code body
  resp="$(curl -sS -X POST "$url" "${HDRS[@]}" -d "$payload" -w $'\n%{http_code}')"
  http_code="${resp##*$'\n'}"
  body="${resp%$'\n'*}"
  if [ "$http_code" != "200" ]; then
    echo "error: POST $url failed (HTTP $http_code)" >&2
    echo "$body" >&2
    return 1
  fi
  printf '%s' "$body"
}

if [ -z "${JWT:-}" ]; then
  # 1. Request the sign-in message for the investor address.
  echo "==> Requesting sign-in message for $INVESTOR..." >&2
  MESSAGE="$(api_post "$API_BASE/auth/message" \
    "$(jq -n --arg a "$INVESTOR" '{address: $a}')" | jq -r '.data.message // empty')"
  [ -n "$MESSAGE" ] || { echo "error: no message in /auth/message response" >&2; exit 1; }

  # 2. Sign the message: raw ed25519 over the UTF-8 message bytes, signed with
  #    the account's secret seed (Stellar S... strkey), hex-encoded. Matches the
  #    API's ed25519.Verify(pub, []byte(message), sig).
  echo "==> Signing sign-in message..." >&2
  SIGNATURE_HEX="$(LEND_SECRET="$(stellar keys secret "$SOURCE")" MESSAGE="$MESSAGE" node -e '
    const crypto = require("crypto");
    const B32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let bits = 0, val = 0; const out = [];
    for (const c of (process.env.LEND_SECRET || "").trim()) {
      const i = B32.indexOf(c);
      if (i < 0) continue;
      val = (val << 5) | i; bits += 5;
      if (bits >= 8) { bits -= 8; out.push((val >> bits) & 0xff); }
    }
    // strkey = [version byte][32-byte seed][2-byte crc]; take the seed.
    const seed = Buffer.from(out).slice(1, 33);
    const der = Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]);
    const key = crypto.createPrivateKey({ key: der, format: "der", type: "pkcs8" });
    process.stdout.write(crypto.sign(null, Buffer.from(process.env.MESSAGE, "utf8"), key).toString("hex"));
  ')"
  [ -n "$SIGNATURE_HEX" ] || { echo "error: failed to sign sign-in message" >&2; exit 1; }

  # 3. Verify the signature and obtain a JWT.
  echo "==> Verifying signature and obtaining JWT..." >&2
  JWT="$(api_post "$API_BASE/auth/verify" \
    "$(jq -n --arg a "$INVESTOR" --arg m "$MESSAGE" --arg s "$SIGNATURE_HEX" \
      '{address: $a, message: $m, signature: $s}')" | jq -r '.data.token // empty')"
  [ -n "$JWT" ] || { echo "error: no token in /auth/verify response" >&2; exit 1; }
fi

# 4. Request the mint proof: { amount, op_id [, contract_id] }, Bearer JWT.
echo "==> Requesting mint proof for $INVESTOR (op $OP_ID, amount $AMOUNT)..." >&2
PAYLOAD="$(jq -n \
  --arg amount "$AMOUNT" \
  --arg op_id "$OP_ID" \
  --arg contract_id "${FACTORY_ID:-}" \
  '{amount: $amount, op_id: $op_id}
   + (if $contract_id == "" then {} else {contract_id: $contract_id} end)')"

PROOF="$(curl -sS -X POST "$API_BASE/users/signatures/mint-proof" \
  "${HDRS[@]}" -H "Authorization: Bearer $JWT" \
  -d "$PAYLOAD" -w $'\n%{http_code}')"
HTTP_CODE="${PROOF##*$'\n'}"
BODY="${PROOF%$'\n'*}"
if [ "$HTTP_CODE" != "200" ]; then
  echo "error: mint-proof request failed (HTTP $HTTP_CODE)" >&2
  echo "$BODY" >&2
  exit 1
fi

SIGNATURE="$(jq -r '.data.signature // empty' <<<"$BODY")"
NONCE="$(jq -r '.data.nonce // empty' <<<"$BODY")"
if [ -z "$SIGNATURE" ] || [ -z "$NONCE" ]; then
  echo "error: could not read signature/nonce from mint-proof response:" >&2
  echo "$BODY" >&2
  exit 1
fi

echo "==> Got signature + nonce; investing..." >&2

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
env \
  SOURCE="$SOURCE" \
  NETWORK="$NETWORK" \
  ${FACTORY_ID:+FACTORY_ID="$FACTORY_ID"} \
  INVESTOR="$INVESTOR" \
  OP_ID="$OP_ID" \
  SHARES="$AMOUNT" \
  NONCE="$NONCE" \
  SIGNATURE="$SIGNATURE" \
  "$SCRIPT_DIR/invest.sh"
