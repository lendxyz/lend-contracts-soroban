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

## `fiat-invest.sh`

Settles an off-chain (fiat) investment on chain: mints op-lend shares to a
holder, records no USDC. `fiat_invest` has no `require_auth` and no admin check
— the backend ed25519 signature is the whole authorisation — so this script
signs the payload locally with the backend signer key held in the Stellar CLI
keystore (`BACKEND_SIGNER_KEY`, default `lend-testnet-signer` /
`lend-mainnet-signer`) and submits the call in one step. No API involved.

```bash
NETWORK=mainnet \
OP_ID=1 \             # required, operation id
SHARES=1000000 \      # shares to mint, 6 decimals
INVESTOR=G... \       # the fiat investor: named in the events, whitelisted
HOLDER=G... \         # optional, receives the shares; defaults to $FIAT_HOLDER
NONCE=fiat-1 \        # optional, defaults to fiat-<op>-<ts>-<rand>
DRY_RUN=1 \           # optional, simulate only (--send=no)
./scripts/fiat-invest.sh
```

`SOURCE` only pays the fee; the contract never checks it. `$INVESTOR` gets no
`Position`, so a fiat participant is not refundable if the operation is
cancelled (see `docs/threat-model-stride.md` DoS.1) — and the nonce is consumed
**permanently**, so a failed submission needs a fresh one. Run `DRY_RUN=1`
first: simulation runs the same signature check and funding guards as the real
call without touching the nonce. (`common.sh` still prints its Ledger notice on
mainnet; with `--send=no` nothing is signed.)

`OP_ID`, `SHARES` and `INVESTOR` are required; a missing one aborts before
anything is signed. `HOLDER` defaults to the network's `FIAT_HOLDER` in
`common.sh` (`GDTFJFH2…`), the custodial address holding shares for investors
who settled in EUR and have no wallet of their own.

## `sign-fiat-invest.js`

The signer behind `fiat-invest.sh`; run it directly to get a signature without
submitting. Pure Node (no deps), reads everything from the environment so the
secret never appears in `ps`, and emits `{ nonce, signature, signer_hex,
message_hex }`.

```bash
SIGNER_SECRET="$(stellar keys secret lend-mainnet-signer)" \
FACTORY_ID=C... OP_ID=1 SHARES=1000000 INVESTOR=G... HOLDER=G... \
node scripts/sign-fiat-invest.js
```

The message mirrors `build_fiat_invest_message`:
`"FIAT_INVEST" || factory_addr || id(u32 BE) || user_addr || holder_addr ||
shares(i128 BE) || nonce` — fixed-width, so every address must be a 56-char
strkey. Verified byte-for-byte against the contract's own builder.

## `deploy-op-lend-wallet.sh`

Builds the wasms and deploys `OpLendWallet`, the custodial contract that holds
op-lend shares for investors who settled in EUR and have no wallet of their own.
Constructor takes admin + backend signer.

```bash
SOURCE=alice \
NETWORK=testnet \  # optional, default testnet
./scripts/deploy-op-lend-wallet.sh
```

`ADMIN` defaults to the `SOURCE` address. `BACKEND_SIGNER` defaults per
`NETWORK` from `common.sh` and is accepted as a `G...` strkey (decoded to the
raw 32-byte pubkey) or as 64 hex chars, same as `deploy-factory.sh`. Prints
`OPLEND_WALLET_ID` on success — record it in `common.sh` and
[`DEPLOYMENTS.md`](../DEPLOYMENTS.md).

One wallet serves every operation: point it at each operation's op-lend token
afterwards with `wallet-register-op-lend.sh`. It is the address you pass as `HOLDER` to
`fiat-invest.sh` once deployed.

## `wallet-register-op-lend.sh`

Admin-only. Maps an operation id to its op-lend token address inside the wallet,
so a redeem only has to name the `OP_ID`.

```bash
SOURCE=admin \        # wallet ADMIN
OPLEND_WALLET_ID=C... \
OP_ID=1 \             # operation id (u32)
OPLEND=C... \         # that operation's op-lend token
./scripts/wallet-register-op-lend.sh
```

Or register several in one transaction with `ENTRIES` (JSON, handed straight to
`register_op_lends`):

```bash
SOURCE=admin \
OPLEND_WALLET_ID=C... \
ENTRIES='[{"op_id":1,"op_lend":"C..."},{"op_id":2,"op_lend":"C..."}]' \
./scripts/wallet-register-op-lend.sh
```

Setting `ENTRIES` takes the batch path and ignores `OP_ID` / `OPLEND`. Only
contract (`C...`) addresses are accepted on chain — a `G...` account is not a
token and the call fails. Read a mapping back with `op_lend(op_id)`, and the
wallet's own holding for an operation with `op_lend_balance(op_id)`.

## `wallet-redeem.sh`

Releases shares the wallet holds for a fiat investor to that investor's own
wallet. `redeem` has no `require_auth` and no admin check — the backend ed25519
signature over the `OP_REDEEM` payload plus a single-use nonce is the whole
authorisation, exactly like `fiat_invest` — so this script signs the payload
locally with the backend signer key held in the Stellar CLI keystore
(`BACKEND_SIGNER_KEY`, default `lend-testnet-signer` / `lend-mainnet-signer`)
and submits the call in one step. No API involved.

```bash
NETWORK=mainnet \
OP_ID=1 \             # required, operation id
DESTINATION=G... \    # required, the user wallet receiving the shares
AMOUNT=1000000 \      # required, shares to release, 6 decimals
NONCE=redeem-1 \      # optional, defaults to redeem-<op>-<ts>-<rand>
WHITELIST=1 \         # optional, whitelist DESTINATION in the same tx
WL_NONCE=wl-1 \       # optional, nonce for the whitelist signature
DRY_RUN=1 \           # optional, simulate only (--send=no)
./scripts/wallet-redeem.sh
```

`SOURCE` only pays the fee; the contract never checks it, so a relayer identity
is enough. `OP_ID`, `DESTINATION` and `AMOUNT` are required; a missing one
aborts before anything is signed, and the operation must already be registered
(`wallet-register-op-lend.sh`).

The nonce is consumed **permanently**, so a failed submission needs a fresh one.
Run `DRY_RUN=1` first: simulation runs the same signature check and transfer
guards as the real call without touching the nonce. (`common.sh` still prints
its Ledger notice on mainnet; with `--send=no` nothing is signed.)

`WHITELIST=1` switches the call to `whitelist_and_redeem`, which whitelists
`DESTINATION` on the op-lend token before transferring. Use it for any
destination the token has never seen: op-lend `transfer` requires **both** sides
whitelisted, and a fresh user wallet that never received a mint is not. The
wallet itself was whitelisted when the factory minted to it. `WL_NONCE` is that
second signature's nonce, consumed by the op-lend token rather than by the
wallet, and defaults alongside `NONCE`.

## `sign-redeem.js`

The signer behind `wallet-redeem.sh`; run it directly to get a signature without
submitting. Pure Node (no deps), reads everything from the environment so the
secret never appears in `ps`, and emits `{ nonce, signature, signer_hex,
message_hex }` — plus a `whitelist` object carrying its own `nonce`,
`signature` and `message_hex` when `OPLEND_ID` is set (`null` otherwise).

```bash
SIGNER_SECRET="$(stellar keys secret lend-mainnet-signer)" \
WALLET_ID=C... OP_ID=1 DESTINATION=G... AMOUNT=1000000 \
node scripts/sign-redeem.js
```

`SIGNER_SECRET`, `WALLET_ID`, `OP_ID`, `DESTINATION` and `AMOUNT` are required;
`NONCE` (generated otherwise), `OPLEND_ID` (the op-lend token — set it to also
get the whitelist signature) and `WL_NONCE` are optional.

Two messages, each a raw ed25519 signature over the concatenated bytes (no
hashing envelope), fixed-width, so every address must be a 56-char strkey:

- `OP_REDEEM`, verified by the wallet — `"OP_REDEEM" || wallet_addr ||
  op_id(u32 BE) || destination_addr || amount(i128 BE) || nonce`
- whitelist, verified by the op-lend token itself (unchanged, see
  `contracts/op-lend/src/crypto.rs`) — `oplend_addr || user_addr || nonce`

Both verified byte-for-byte against the contracts' own builders. Nonces are
namespaced per contract — the wallet's redeem nonces and the op-lend token's
whitelist nonces are separate stores — so `NONCE` and `WL_NONCE` never collide.

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
- `OPLEND_WALLET_ID` is the deployed `OpLendWallet` (`wallet-register-op-lend.sh`,
  `wallet-redeem.sh`, `upgrade-contract.sh CONTRACT=wallet`). It lives in `common.sh`
  alongside `FACTORY_ID` / `REWARDS_ID`: set for testnet, and **empty on
  mainnet until the first `deploy-op-lend-wallet.sh`** — until then `req`
  aborts the scripts that need it, so record the new id there (and in
  [`DEPLOYMENTS.md`](../DEPLOYMENTS.md)) right after deploying.
