# Deploy scripts

Bash helpers around the `stellar` CLI. Require `stellar` (v26+) installed and an
identity created (`stellar keys generate <name> --network testnet --fund`).

## `deploy.sh`

Builds the wasms, uploads the op-lend wasm, deploys the factory, and calls
`initialize`.

```bash
SOURCE=alice \
BACKEND_SIGNER=ab12..\  # backend ed25519 pubkey, 64 hex chars (32 bytes)
NETWORK=testnet      \  # optional, default testnet
./scripts/deploy.sh
```

`USDC` and `ORACLE` default per `NETWORK` (see [Network addresses](#network-addresses)); set them to override. `ADMIN` defaults to the `SOURCE` address. Prints `FACTORY_ID` and `OPLEND_WASM_HASH` on success.

The op-lend wasm is uploaded once; every operation the factory creates is a new
op-lend instance deployed from that hash.

## `create-operation.sh`

Admin-only. Deploys + registers a new operation (and its op-lend token).

```bash
SOURCE=alice \
FACTORY_ID=CC... \
OP_NAME="Alpha Fund" \
TOTAL_SHARES=1000000 \    # supply cap, 6 decimals
EUR_PER_SHARES=1000000 \  # 1 EUR per share, 6 decimals
./scripts/create-operation.sh
```

Prints the deployed op-lend token address.

## `start-operation.sh`

Admin-only. Flips an operation to started so it can accept `invest` calls.

```bash
SOURCE=alice \
FACTORY_ID=CC... \
OP_ID=0 \
./scripts/start-operation.sh
```

## `invest.sh`

Invest in a started operation. `SOURCE` is the investor (signs the tx and pays
USDC); the backend signature + nonce are supplied by the caller.

```bash
SOURCE=alice \
FACTORY_ID=CC... \
OP_ID=0 \
SHARES=100 \                 # shares to buy, 6 decimals
NONCE=abc \                  # must match what the signature was built with
SIGNATURE=deadbeef... \      # backend ed25519 sig, 64-byte hex
INVESTOR=G... \              # optional, defaults to `stellar keys address $SOURCE`
./scripts/invest.sh
```

The signature must cover the contract's `build_invest_message`:
`"ONCHAIN_INVEST" || factory_addr || id(u32 BE) || user_addr || shares(i128 BE) || nonce`,
signed by the backend signer ed25519 key (see `contracts/factory/src/crypto.rs`).

## `update-backend-signer.sh`

Admin-only. Updates the factory's backend signer — the ed25519 key whose
signatures `invest` / `predeposit` / `fiat_invest` verify against. Use this when
the signer the API holds (`STELLAR_SIGNER_PRIVATE_KEY` in `lend-api`) differs
from what the factory was deployed with (`ED25519 verification` failures).

```bash
SOURCE=alice \
FACTORY_ID=CC... \
BACKEND_SIGNER=GAOQ67SJ... \  # 64 hex chars or a G... strkey
./scripts/update-backend-signer.sh
```

Accepts a `G...` strkey (decoded to the raw 32-byte pubkey) or 64 hex chars,
same as `deploy-factory.sh`.

## `deploy-rewards.sh`

Deploys the `LendRewards` merkle reward-distribution contract (constructor takes
admin + reward token).

```bash
SOURCE=alice \
NETWORK=testnet \  # optional, default testnet
./scripts/deploy-rewards.sh
```

`REWARD_TOKEN` defaults to the network's USDC (see
[Network addresses](#network-addresses)); set it to override. `ADMIN` defaults
to the `SOURCE` address. Prints `REWARDS_ID` on success.

Merkle leaves are `keccak256(user_strkey_bytes ++ balance_i128_be)`, internal
nodes sorted-pair keccak256 (OZ-compatible). The backend tree builder must match
— see `contracts/rewards/src/merkle.rs`.

## `distribute-op-rewards.sh`

Admin-only. Builds a merkle tree for a round of operation rewards and
distributes it on `LendRewards` for a given `OP_ID` / `EPOCH`. Wraps
`build-merkle-tree.js`, approves the reward token, then calls
`distribute_op_rewards(op_id, epoch, merkle_root, total_allocation)`.

```bash
SOURCE=admin \             # rewards-contract ADMIN; signs + funds the round
REWARDS_ID=C... \          # deployed LendRewards contract
OP_ID=1 \                  # operation id (u32)
EPOCH=3 \                  # reward epoch (u32)
RECIPIENTS=./round3.json \ # recipients file (see below)
NETWORK=testnet \          # optional, default testnet
./scripts/distribute-op-rewards.sh
```

`RECIPIENTS` is JSON, either an object `{ "G...": "1000000", ... }` or an array
`[ { "address": "G...", "balance": "1000000" }, ... ]`; balances are reward-token
base units (integers). Optional: `OUT` (proofs file the tree is written to;
default `./rewards-op<OP_ID>-epoch<EPOCH>.json`), `TOTAL_ALLOCATION` (default =
sum of balances; must be >= the sum), `REWARD_TOKEN` (default read from the
contract), `APPROVE=0` to skip the allowance step, `EXPIRATION_LEDGER` to
override the approval expiry.

The merkle tree is built (via `build-merkle-tree.js`) as the first step of the
same command — generation and distribution happen together, so `OUT` is written
fresh from `RECIPIENTS` on every run. It carries `root`, `total_allocation`, and
per-recipient `{ address, balance, proof }` — the `proof` + `balance` are what
each user later passes to
`claim_op_epoch(op_id, user, epoch, claimed_balance, merkle_proof)`.

Via make — `RECIPIENTS` defaults to `scripts/recipients.json`, `OUT` to
`scripts/rewards-op<OP_ID>-epoch<EPOCH>.json`:

```bash
make distribute-op-rewards REWARDS_ID=C... OP_ID=1 EPOCH=3
# or with an explicit recipients file:
make distribute-op-rewards REWARDS_ID=C... OP_ID=1 EPOCH=3 RECIPIENTS=./round3.json
```

## `build-merkle-tree.js`

Pure-Node (no deps) merkle-tree builder used by `distribute-op-rewards.sh`; run
it directly to compute a root + proofs without distributing.

```bash
node scripts/build-merkle-tree.js recipients.json [out.json]
```

Leaves are `keccak256(user_strkey_ascii ++ balance_i128_be16)`, internal nodes
sorted-pair keccak256 (OZ `MerkleProof._hashPair`) — verified byte-for-byte
against the contract's own leaf output. It self-verifies every proof against the
root before emitting. Matches `contracts/rewards/src/merkle.rs`.

## `deploy-dummy-usdc.sh`

Deploys `DummyUSDC`, a testnet stand-in for Circle USDC. Standard SEP-41 token
with **no transfer restrictions**; constructor takes admin + metadata.

```bash
SOURCE=alice \
NETWORK=testnet \  # optional, default testnet
./scripts/deploy-dummy-usdc.sh
```

`ADMIN` defaults to the `SOURCE` address. `DECIMAL` / `NAME` / `SYMBOL` default
to `6` / `"Dummy USD Coin"` / `"dUSDC"`. Prints `DUMMY_USDC_ID` on success.

After deploy it mints **10M tokens** (`10_000_000 * 10^DECIMAL` base units) to
the admin. The seed mint is signed by `SOURCE`, so it only works when `ADMIN`
is the `SOURCE` address (the default).

- `mint(to, amount)` —— **open to anyone**, so devs can self-serve test
  tokens.

Use this when you want a USDC-like token you fully control on testnet instead of
the shared Circle SAC.

## `fund-dummy-usdc.sh`

Mints DummyUSDC to any address. `mint` is open to anyone, so any `SOURCE`
identity can top up any address.

```bash
SOURCE=alice \
TO=G... \
DUMMY_USDC_ID=CC... \  # optional, defaults per NETWORK (testnet below)
AMOUNT_WHOLE=10000 \   # optional, whole tokens (default 10000), scaled by DECIMAL
DECIMAL=6 \            # optional, default 6
./scripts/fund-dummy-usdc.sh
```

`DUMMY_USDC_ID` defaults to `CCO56ZVZPLGELBZGAVLTNC5GPZUIF4SIAIGPYNHWBRUSKBLC7HPF5QPN`
on testnet; set it to override.

## Network addresses

`deploy.sh` fills these in by `NETWORK` unless you override `USDC` / `ORACLE`.
Verified 2026-06-02 (on-chain + Circle/Stellar docs).

| Asset | testnet | mainnet |
|---|---|---|
| **USDC** (SAC) | `CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA` | `CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75` |
| **Reflector FX oracle** | `CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W` | `CBKGPWGKSKZF52CFHMTRR23TBWTPMRDIYZ4O2P5VS65BMHYH4DXMCJZC` |

- **Reflector FX oracle** = the fiat/forex feed (one of Reflector's three
  oracles). Base asset `USD`, `decimals() = 14`, quotes EUR via
  `lastprice(Asset::Other("EUR"))`. Returned ≈1.1646 USD/EUR on both networks.
- **USDC issuers**: mainnet `GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN`,
  testnet `GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5` (per Circle).
  SAC IDs are deterministic — re-derive if your test setup uses a different
  issuer:
  ```bash
  stellar contract id asset --asset USDC:<ISSUER> \
    --network-passphrase "Test SDF Network ; September 2015"
  ```
- Testnet's FX feed set is smaller than mainnet's and differs (e.g. testnet has
  CHF, mainnet has many more currencies) — EUR is on both. Read `decimals()` /
  `assets()` per network rather than assuming.

## Notes

- `BACKEND_SIGNER` is the ed25519 **public** key the backend signs invest /
  whitelist messages with. The message format the backend must reproduce is in
  `contracts/factory/src/crypto.rs` and `contracts/op-lend/src/crypto.rs`.
