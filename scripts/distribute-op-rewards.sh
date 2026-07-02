#!/usr/bin/env bash
#
# Build a merkle tree for a round of operation rewards and distribute it on the
# LendRewards contract for a given (OP_ID, EPOCH).
#
# Steps:
#   1. build the merkle tree from RECIPIENTS (address -> balance) with
#      scripts/build-merkle-tree.js  (leaf/pair layout mirrors merkle.rs)
#   2. approve the reward token so the contract can pull TOTAL_ALLOCATION
#      (skip with APPROVE=0 if the admin already granted an allowance)
#   3. call distribute_op_rewards(op_id, epoch, merkle_root, total_allocation)
#
# The generated proofs file (OUT) is what users later feed to claim_op_epoch:
# for each claim it carries { address, balance, proof }.
#
# Required env vars:
#   SOURCE        Stellar CLI identity of the rewards-contract ADMIN (signs the
#                 tx and funds TOTAL_ALLOCATION of reward token).
#   REWARDS_ID    Deployed LendRewards contract address (C...).
#   OP_ID         Operation id (u32).
#   EPOCH         Reward epoch (u32).
#   RECIPIENTS    Path to a JSON file of recipients. Either
#                   { "G...": "1000000", ... }  or
#                   [ { "address": "G...", "balance": "1000000" }, ... ]
#                 Balances are reward-token base units (integers).
#
# Optional env vars:
#   NETWORK           testnet | mainnet (default: testnet).
#   OUT               Where to write the proofs JSON (default:
#                     ./merkle.json).
#   TOTAL_ALLOCATION  Override the funded amount (default: sum of balances).
#                     Must be >= the sum, or claims will run the contract dry.
#   REWARD_TOKEN      Reward token contract for the approval step. Default:
#                     read from the contract via `reward_token`.
#   APPROVE           1 (default) to approve the token allowance first; 0 to
#                     skip (e.g. you funded the contract another way).
#   EXPIRATION_LEDGER Approval expiration ledger (default: current + ~30 days).
#
# Usage:
#   SOURCE=admin REWARDS_ID=C... OP_ID=1 EPOCH=3 \
#   RECIPIENTS=./round3.json ./scripts/distribute-op-rewards.sh
#
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
APPROVE="${APPROVE:-1}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

req() { [ -n "${!1:-}" ] || { echo "error: \$$1 is required" >&2; exit 1; }; }
req SOURCE
req REWARDS_ID
req OP_ID
req EPOCH
req RECIPIENTS

[ -f "$RECIPIENTS" ] || { echo "error: RECIPIENTS file not found: $RECIPIENTS" >&2; exit 1; }
for bin in node jq stellar; do
  command -v "$bin" >/dev/null || { echo "error: $bin is required" >&2; exit 1; }
done

OUT="${OUT:-./merkle.json}"

echo "==> Network:   $NETWORK" >&2
echo "==> Rewards:   $REWARDS_ID" >&2
echo "==> Op / epoch: $OP_ID / $EPOCH" >&2
echo "==> Recipients: $RECIPIENTS" >&2

# 1. Build the tree (writes OUT, echoes JSON to stdout which we capture).
echo "==> Building merkle tree..." >&2
TREE="$(node "$SCRIPT_DIR/build-merkle-tree.js" "$RECIPIENTS" "$OUT")"

MERKLE_ROOT="$(jq -r '.root' <<<"$TREE")"
MERKLE_ROOT_HEX="${MERKLE_ROOT#0x}"
SUM="$(jq -r '.total_allocation' <<<"$TREE")"
COUNT="$(jq -r '.count' <<<"$TREE")"
TOTAL_ALLOCATION="${TOTAL_ALLOCATION:-$SUM}"

echo "==> Recipients:       $COUNT" >&2
echo "==> Merkle root:      $MERKLE_ROOT" >&2
echo "==> Sum of balances:  $SUM" >&2
echo "==> Total allocation: $TOTAL_ALLOCATION" >&2
echo "==> Proofs written:   $OUT" >&2

ADMIN="$(stellar keys address "$SOURCE")"

# 2. Approve the reward token so distribute can transfer_from the admin.
if [ "$APPROVE" = "1" ]; then
  REWARD_TOKEN="${REWARD_TOKEN:-$(stellar contract invoke \
    --id "$REWARDS_ID" --source "$SOURCE" --network "$NETWORK" \
    -- reward_token | tr -d '"')}"
  # Approval expires ~30 days out (5s ledger cadence => 518400 ledgers).
  if [ -z "${EXPIRATION_LEDGER:-}" ]; then
    command -v curl >/dev/null || { echo "error: curl is required to derive EXPIRATION_LEDGER (or set it)" >&2; exit 1; }
    case "$NETWORK" in
      testnet) RPC_URL="https://soroban-testnet.stellar.org" ;;
      mainnet|pubnet|public) RPC_URL="https://mainnet.sorobanrpc.com" ;;
      *) echo "error: set EXPIRATION_LEDGER for custom network '$NETWORK'" >&2; exit 1 ;;
    esac
    LATEST="$(curl -sS -X POST "$RPC_URL" \
      -H 'Content-Type: application/json' \
      -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger"}' \
      | jq -r '.result.sequence')"
    [ -n "$LATEST" ] && [ "$LATEST" != "null" ] || { echo "error: could not read latest ledger from $RPC_URL" >&2; exit 1; }
    EXPIRATION_LEDGER="$((LATEST + 518400))"
  fi
  echo "==> Approving $TOTAL_ALLOCATION of $REWARD_TOKEN (expires ledger $EXPIRATION_LEDGER)..." >&2
  stellar contract invoke \
    --id "$REWARD_TOKEN" \
    --source "$SOURCE" \
    --network "$NETWORK" \
    -- approve \
    --from "$ADMIN" \
    --spender "$REWARDS_ID" \
    --amount "$TOTAL_ALLOCATION" \
    --expiration_ledger "$EXPIRATION_LEDGER"
fi

# 3. Distribute: sets the root and pulls TOTAL_ALLOCATION into the contract.
echo "==> Distributing op rewards..." >&2
stellar contract invoke \
  --id "$REWARDS_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- distribute_op_rewards \
  --op_id "$OP_ID" \
  --epoch "$EPOCH" \
  --merkle_root "$MERKLE_ROOT_HEX" \
  --total_allocation "$TOTAL_ALLOCATION"

echo "" >&2
echo "==> Done. Distributed round for op $OP_ID epoch $EPOCH." >&2
echo "    root=$MERKLE_ROOT total=$TOTAL_ALLOCATION proofs=$OUT" >&2
