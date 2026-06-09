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

Two ways to create supply:

- `mint(to, amount)` — **admin only** (`admin.require_auth()`).
- `faucet(to, amount)` — **open to anyone**, so devs can self-serve test
  tokens.

Use this when you want a USDC-like token you fully control on testnet instead of
the shared Circle SAC.

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
