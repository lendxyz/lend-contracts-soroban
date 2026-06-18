# Lend Contracts — Soroban

Soroban smart contracts powering the **Lend** protocol: an on-chain fundraising
and tokenized-securities platform on Stellar. The core of the system is a
`Factory` that runs the full lifecycle of a tokenized funding operation and
deploys a restricted `OpLend` token per operation.

The repository also ships two supporting contracts: `LendRewards` (merkle-based
reward distribution) and `DummyUSDC` (a testnet-only USDC stand-in).

---

## Table of contents

- [Architecture](#architecture)
- [Contracts](#contracts)
  - [Factory](#factory)
  - [OpLend token](#oplend-token)
  - [LendRewards](#lendrewards)
  - [DummyUSDC](#dummyusdc)
- [Backend signature scheme](#backend-signature-scheme)
- [Oracle adapter](#oracle-adapter)
- [Function signatures](#function-signatures)
- [Building](#building)
- [Testing](#testing)
- [Deploying](#deploying)
- [Project structure](#project-structure)

---

## Architecture

```
                         ┌──────────────────────────────┐
                         │            Factory            │
                         │  (operation + funding mgmt)   │
                         │                               │
   investor ── invest ──►│  • verifies backend signature │
   (USDC) ───────────────│  • prices shares via oracle   │
                         │  • escrows USDC               │
                         │  • mints OpLend tokens ───────┼──► OpLend (op #N)
                         │  • deploys 1 OpLend / op      │      restricted token
                         └───────────────┬───────────────┘
                                         │ reads EUR/USD
                                         ▼
                                 Reflector oracle
                                  (SEP-40, mocked)
```

- The **Factory** is the single entry point for operators and investors. It
  owns each operation's escrowed USDC and is the **admin** of every `OpLend`
  token it deploys, so all minting, whitelisting and burning flow through it.
- Each operation gets its own **OpLend** token (one ERC-20-equivalent per
  raise), deployed deterministically by the Factory from an uploaded WASM hash.
- Investments are authorized off-chain (after KYC) by a **backend signer**; the
  Factory verifies an ed25519 signature on-chain before accepting funds.
- Share pricing uses a **Reflector (SEP-40)** oracle adapter for the EUR/USD
  rate. The interface is defined in [`oracle.rs`](contracts/factory/src/oracle.rs)
  and exercised against a mock in tests.

### Operation lifecycle

```
create_operation ──► [created] ──► start_operation ──► [open] ──► funding fills ──► [finished] ──► withdraw_usdc
                          │                               │
                          │                               ├─ pause_funding(true/false)
                          │                               └─ invest / fiat_invest / gift_op_tokens
                          │
                          ├─ set_predeposits(true) ──► predeposit ... ──► (auto-start when filled) ──► claim_op_tokens
                          │
                          └─ cancel_operation ──► [canceled] ──► refund_user / batch_refund_users
```

An operation is **finished** when `funding_progress >= total_shares`. Funds can
only be withdrawn once finished, and only once.

---

## Contracts

### Factory

[`contracts/factory`](contracts/factory) — `LendFactory`.

Responsibilities:

- **Operation management**: create, start, pause/resume, open/close
  pre-deposits, and cancel operations.
- **Investment processing**: `invest` (on-chain USDC), `fiat_invest` (USDC
  settled off-chain, tokens minted to a holder), `gift_op_tokens` (admin gift),
  and `predeposit` (commit before an operation officially starts). All
  signature-gated paths verify a backend ed25519 signature and consume a
  single-use nonce.
- **Claiming**: `claim_op_tokens` / `claim_op_tokens_batch` mint the OpLend
  tokens accumulated through pre-deposits and gifts.
- **Refunds & withdrawal**: `refund_user` / `batch_refund_users` burn OpLend and
  return escrowed USDC; `withdraw_usdc` releases raised funds once an operation
  is finished.
- **OpLend admin proxy**: because the Factory is the OpLend admin, it exposes
  `oplend_whitelist_user`, `oplend_update_backend_signer`, and
  `oplend_admin_burn`.
- **Pricing helpers**: `get_amount_in` / `get_amount_out` quote shares ↔ USDC.

Key invariants enforced on every invest path (`invest_guards`): operation
exists, is started, not canceled, not paused, not finished, the caller is not
blacklisted, `shares_amount > 0`, and the new total does not exceed
`total_shares`.

### OpLend token

[`contracts/op-lend`](contracts/op-lend) — `OpLendToken`.

A SEP-41 token (the standard Soroban `TokenInterface`) with three additions that
enforce the legal framework for tokenized securities:

- **Transfer restrictions** — `transfer` and `transfer_from` require **both**
  sender and recipient to be whitelisted (`require_whitelisted`).
- **Whitelist / blacklist management** — `whitelist_user_admin` (admin-set),
  `whitelist_user` (anyone, given a valid backend signature + unused nonce), and
  automatic whitelisting of mint recipients. Minting whitelists `to`.
- **Capped supply** — `mint` panics if `total_supply + amount > max_supply`.
- **Admin controls** — `admin_burn` (burn from any holder, no allowance),
  `set_admin`, `update_backend_signer`.

Constructor parameters: `admin`, `decimal` (≤ 6), `name`, `symbol`,
`max_supply`, `backend_signer`. In production the Factory is the `admin` and the
`max_supply` is the operation's `total_shares`.

### LendRewards

[`contracts/rewards`](contracts/rewards) — `LendRewards`.

Merkle-based reward distribution for operation rewards and referral rewards,
indexed by epoch. The admin publishes a merkle root and funds the allocation;
users claim with a merkle proof. Roots are immutable once set, claims are
idempotent per `(scope, epoch, user)`, and the contract is upgradeable
(admin-gated `upgrade`).

### DummyUSDC

[`contracts/dummy-usdc`](contracts/dummy-usdc) — testnet-only SEP-41 token that
emulates Circle USDC: **open `mint`** and **no transfer restrictions**. Use it
when you want a USDC-like token you fully control on testnet instead of the
shared Circle SAC.

---

## Backend signature scheme

Investments and signature-gated whitelisting are authorized off-chain after KYC,
then verified on-chain via **ed25519**. See
[`crypto.rs`](contracts/factory/src/crypto.rs).

The Factory stores a 32-byte backend public key (`BackendSigner`). For each
signature-gated call it:

1. Rebuilds the canonical message from on-chain values.
2. Verifies the signature with `env.crypto().ed25519_verify` (panics if invalid).
3. Consumes the `nonce` (panics on replay).

Canonical messages (byte-concatenated):

| Call           | Message |
| -------------- | ------- |
| `invest` / `predeposit` | `"ONCHAIN_INVEST"` ‖ contract_address ‖ `id` (BE u32) ‖ user ‖ `amount` (BE i128) ‖ `nonce` |
| `fiat_invest`  | `"FIAT_INVEST"` ‖ contract_address ‖ `id` ‖ user ‖ oplend_holder ‖ `amount` ‖ `nonce` |
| OpLend `whitelist_user` | contract_address ‖ user ‖ `nonce` |

The contract address domain-separates signatures per deployment (replacing the
EVM `chainid`). The matching off-chain signer lives in the `lend-worker-stellar`
backend.

---

## Oracle adapter

[`oracle.rs`](contracts/factory/src/oracle.rs) defines the **Reflector (SEP-40)**
interface used to price shares:

```rust
pub trait OracleInterface {
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
    fn decimals(env: Env) -> u32;
}
```

`get_eur_usd_price` reads the `EUR` price, rejects non-positive or stale prices
(> 24h old), and scales it to 6 decimals. Pricing math:

- `amount_in(eur_per_shares, shares)` → USDC cost (`PRICE_PRECISION = 1e6`).
- `amount_out(eur_per_shares, usdc)` → shares (`SHARE_PRECISION = 1e12`).

Tests inject a mock oracle implementing this interface; production points at the
deployed Reflector feed via `update_oracle_address`.

---

## Function signatures

### Factory (`LendFactory`)

```rust
// Setup
fn initialize(admin, usdc, oracle, backend_signer: BytesN<32>, oplend_wasm_hash: BytesN<32>)
fn set_oplend_wasm_hash(oplend_wasm_hash: BytesN<32>)

// Operations
fn create_operation(op_name: String, total_shares: i128, eur_per_shares: i128) -> Address
fn start_operation(id: u32)
fn cancel_operation(id: u32)
fn pause_funding(id: u32, state: bool)
fn set_predeposits(id: u32, state: bool)

// Invest
fn invest(user, id: u32, shares_amount: i128, nonce: String, signature: BytesN<64>)
fn fiat_invest(id: u32, shares_amount: i128, user, oplend_holder, nonce: String, signature: BytesN<64>)
fn gift_op_tokens(id: u32, shares_amount: i128, user)
fn predeposit(user, id: u32, shares_amount: i128, nonce: String, signature: BytesN<64>)
fn claim_op_tokens(id: u32, user)
fn claim_op_tokens_batch(id: u32, users: Vec<Address>)
fn get_amount_in(id: u32, shares_amount: i128) -> i128
fn get_amount_out(id: u32, usdc_amount: i128) -> i128

// Admin
fn refund_user(id: u32, user)
fn batch_refund_users(id: u32, users: Vec<Address>, len: u32)
fn withdraw_usdc(id: u32, destination)
fn update_oracle_address(new_oracle)
fn update_backend_signer(new_signer: BytesN<32>)
fn blacklist(user, state: bool)
fn oplend_whitelist_user(op_id: u32, user, state: bool)
fn oplend_update_backend_signer(op_id: u32, new_signer: BytesN<32>)
fn oplend_admin_burn(op_id: u32, user, value: i128)
fn transfer_ownership(new_admin)

// Getters
fn operation_count() -> u32
fn operations(id) / get_operation(id) -> Operation
fn is_operation_finished(id) -> bool
fn funding_progress(id) -> i128
fn usdc_raised(id) -> i128
fn funding_paused(id) -> bool
fn operation_started(id) -> bool
fn operation_canceled(id) -> bool
fn usdc_withdrawn(id) -> bool
fn predeposits_open(id) -> bool
fn usdc_raised_per_client(id, user) -> i128
fn predeposits(id, user) -> i128
fn gifted(id, user) -> i128
fn claimable_total(id, user) -> i128
fn blacklisted(user) -> bool
fn usdc() -> Address
```

### OpLend token (`OpLendToken`)

```rust
fn __constructor(admin, decimal: u32, name: String, symbol: String, max_supply: i128, backend_signer: BytesN<32>)

// SEP-41 / TokenInterface
fn allowance(from, spender) -> i128
fn approve(from, spender, amount: i128, expiration_ledger: u32)
fn balance(id) -> i128
fn transfer(from, to: MuxedAddress, amount: i128)          // both parties must be whitelisted
fn transfer_from(spender, from, to, amount: i128)          // both parties must be whitelisted
fn burn(from, amount: i128)
fn burn_from(spender, from, amount: i128)
fn decimals() -> u32
fn name() -> String
fn symbol() -> String

// Extensions
fn mint(to, amount: i128)                                  // admin; caps at max_supply; whitelists `to`
fn admin_burn(user, amount: i128)                          // admin; no allowance needed
fn whitelist_user_admin(user, state: bool)                 // admin
fn whitelist_user(user, nonce: String, signature: BytesN<64>)
fn update_backend_signer(new_signer: BytesN<32>)           // admin
fn set_admin(new_admin)                                    // admin
fn is_whitelisted(user) -> bool
fn total_supply() -> i128
fn max_supply() -> i128
fn get_allowance(from, spender) -> Option<AllowanceValue>
```

---

## Building

Requires the [Stellar CLI](https://developers.stellar.org/docs/tools/cli) and a
Rust toolchain with the `wasm32v1-none` target.

```sh
make build          # stellar contract build  (+ prints WASM sizes)
```

This builds every workspace contract to `target/wasm32v1-none/release/*.wasm`.
Both core contracts compile to WASM well under the 64KB limit. Build before
testing — the Factory integration tests `contractimport!` the OpLend WASM.

```sh
make fmt            # cargo fmt --all
make clean          # cargo clean
```

## Testing

```sh
make test           # build, then cargo test
# or directly:
cargo test
cargo test -p factory          # a single contract's suite
```

Tests run against the Soroban test host (no network needed) and cover:

- **Operation lifecycle** — create, start, pause/resume, pre-deposit toggling,
  cancellation, finish detection.
- **Investment flow** — `invest`, `fiat_invest`, gifting, share-cap enforcement,
  blacklist/guard rejections.
- **Backend signature verification** — valid signatures accepted, tampered
  signatures and replayed nonces rejected.
- **Pre-deposit + claim** — committing before start, auto-start on fill,
  claiming accumulated tokens (single + batch).
- **Cancellation + refund** — single and batch refunds restore USDC and burn
  OpLend.
- **Fund withdrawal** — only after finish, only once.
- **OpLend token** — minting, supply-cap enforcement, transfer restrictions,
  whitelist (admin + signature) and blacklist behavior.
- **Oracle pricing** — via a mock oracle implementing the SEP-40 interface.
- **Rewards** — merkle proof verification, idempotent claims, immutable roots.

### Coverage

```sh
cargo install cargo-llvm-cov     # once
cargo llvm-cov --workspace       # line/region coverage report
```

## Deploying

Deployment is scripted under [`scripts/`](scripts/README.md) and wrapped by the
Makefile (testnet defaults shown in the [`Makefile`](Makefile)):

```sh
make deploy-factory
make deploy-rewards
make deploy-dummy-usdc
make create-operation OP_NAME="Alpha" TOTAL_SHARES=1000000 EUR_PER_SHARES=1000000
make start-operation OP_ID=0
make invest OP_ID=0 SHARES=100 NONCE=abc SIGNATURE=deadbeef...
```

Current testnet addresses are tracked in [`DEPLOYMENTS.md`](DEPLOYMENTS.md). See
[`scripts/README.md`](scripts/README.md) for the full variable list per script.

## Project structure

```
contracts/
  factory/      LendFactory — operation & funding lifecycle, OpLend deployer
  op-lend/      OpLendToken — SEP-41 token with transfer restrictions + cap
  rewards/      LendRewards — merkle-based reward distribution
  dummy-usdc/   DummyUSDC   — testnet-only open-mint USDC stand-in
scripts/        deploy + interaction scripts (see scripts/README.md)
Makefile        build / test / deploy targets
DEPLOYMENTS.md  deployed contract addresses
```

Each contract has its own `Cargo.toml` and relies on the top-level workspace
`Cargo.toml` for shared dependencies.
```
