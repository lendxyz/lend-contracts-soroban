# Contract interface changes: `main` → `develop`

What changes for app integrations when the contracts move from `main`
(`ae3a308`) to `develop` (`5d0d7a6`).

The function, type, error and event lists below are produced by diffing the
contract specs embedded in the wasm of both branches (`stellar contract info
interface`), so they are complete. The behavior notes (signed messages, auth,
units, semantics) come from the source diff and `AUDIT-NOTES.md`.

## Deployment plan

| Contract | What happens | Address |
| --- | --- | --- |
| Factory (`LendFactory`) | Redeployed on a fresh instance | **New** |
| OpLend (`OpLendToken`) | New code; every operation created by the new factory deploys a new token | **New per operation** |
| OpLendWallet | Upgraded in place | Unchanged |
| Rewards (`LendRewards`) | Upgraded in place | Unchanged |
| DummyUSDC (testnet only) | No interface change | Unchanged |

What is live today:

- **Mainnet**: the factory, wallet and rewards run exactly the `main`
  interface; the op-lend code new operations deploy from differs from `main`
  only in doc strings.
- **Testnet**: wallet and rewards run the `main` interface. The factory runs
  `main` **plus an early EURC preview** (`invest_eur`, `get_amount_in_eur`,
  `eurc`, `eurc_raised`, `eurc_raised_per_client`, `eurc_withdrawn_amount`,
  `set_eurc`, and `eurc_amount` in `FundingWithdrawn`). Everything below is
  relative to `main`; on testnet the EURC functions already exist, but
  `set_eurc` is **removed** in `develop` (EURC is now a constructor argument).

## Integration checklist

1. Point the app at the **new factory address**. New operations get new op-lend
   token addresses (read them from `get_operation(id).op_token`).
2. Operation ids continue the old factory's numbering: the first one is
   `first_op_id` (old `operation_count` + 1; **18 on testnet**). Operations
   created by the old factory are **not** visible in the new one
   (`get_operation` fails with `OperationNotFound`); see
   [Legacy operations](#legacy-operations).
3. Re-map **every factory and op-lend error code** (all renumbered), and handle
   the new ones. Wallet and rewards codes are unchanged except the wallet's
   new `FactoryAlreadySet` (9).
4. Decode `Operation.eur_per_share` (was `eur_per_shares`).
5. Update callers of the renamed or reshaped functions: `withdraw_usdc` →
   `withdraw_funds`, `usdc_withdrawn` → `funds_withdrawn`, `refund_user`,
   `batch_refund_users`, `create_operation` (argument name), wallet
   `register_op_lend` / `register_op_lends`.
6. Update event decoders: `Refunded` and `FundingWithdrawn` gained
   `eurc_amount`; `OperationFinished.amount_raised_euro` is now 6-decimal;
   new `RefundedFiat` and `NonceVoided`; removed pre-deposit / gift events.
7. Sign invest messages against the **new factory address**, and op-lend
   whitelist messages against the **new token addresses**. Message layouts are
   unchanged.
8. Right after upgrading the wallet, call `set_factory(new_factory)` once:
   until then every signature path of the wallet fails with `NotInitialized`.

---

## Factory (`LendFactory`)

### Setup

| `main` | `develop` |
| --- | --- |
| `initialize(admin, usdc, oracle, backend_signer: BytesN<32>, oplend_wasm_hash: BytesN<32>)`, called after deploy | `__constructor(admin, usdc, eurc, oracle, stablecoin_oracle, backend_signer: BytesN<32>, oplend_wasm_hash: BytesN<32>, first_op_id: u32)`, runs inside the deploy transaction |

- `eurc`: EURC token, fixed at deploy (its decimals are cached).
- `stablecoin_oracle`: Reflector CEX & DEX feed pricing `USDC` and `EURC` in
  USD, for the depeg checks.
- `first_op_id`: id of the first operation; `0` fails with
  `InvalidOperationId`.

### Removed functions

| Function | Replacement |
| --- | --- |
| `initialize(admin, usdc, oracle, backend_signer, oplend_wasm_hash)` | `__constructor` |
| `predeposit(user, id, shares_amount, nonce, signature)` | none (feature removed) |
| `gift_op_tokens(id, shares_amount, user)` | none |
| `claim_op_tokens(id, user)` | none |
| `claim_op_tokens_batch(id, users: Vec<Address>)` | none |
| `set_predeposits(id, state: bool)` | none |
| `predeposits_open(id) -> bool` | none |
| `predeposits(id, user) -> i128` | none |
| `gifted(id, user) -> i128` | none |
| `claimable_total(id, user) -> i128` | none |
| `oplend_update_backend_signer(op_id, new_signer)` | none: tokens read the signer from the factory, so `update_backend_signer` rotates it everywhere |
| `withdraw_usdc(id, destination)` | `withdraw_funds(id, destination)` |
| `usdc_withdrawn(id) -> bool` | `funds_withdrawn(id) -> bool` |

### Added functions

| Function | Purpose |
| --- | --- |
| `invest_eur(user, id, shares_amount, nonce, signature: BytesN<64>)` | Buy shares with EURC; same signature as `invest` |
| `get_amount_in_eur(id, shares_amount) -> i128` | EURC cost of shares, in EURC base units |
| `get_amount_out_eur(id, eurc_amount) -> i128` | Shares for an EURC amount |
| `eurc() -> Address` | EURC token |
| `eurc_raised(id) -> i128` | EURC raised, net of refunds |
| `eurc_withdrawn_amount(id) -> i128` | Cumulative EURC withdrawn |
| `eurc_raised_per_client(id, user) -> i128` | EURC paid in by `user` (a record, not reduced by refunds) |
| `funds_withdrawn(id) -> bool` | True once any coin has been withdrawn (replaces `usdc_withdrawn`) |
| `withdraw_funds(id, destination)` | Sends the USDC **and** EURC raised since the previous withdrawal (replaces `withdraw_usdc`) |
| `fiat_refund(id, user, oplend_holder, shares_amount)` | Admin: mirrors an off-chain euro refund by burning `shares_amount` from `oplend_holder` and reopening them for sale |
| `void_nonce(nonce, signature: Option<BytesN<64>>)` | Burns an unspent invest / fiat-invest nonce (see [Signed messages](#signed-messages)) |
| `oplend_void_nonce(op_id, nonce)` | Admin: voids a whitelist nonce on an operation's token |
| `oplend_upgrade(op_id, new_wasm_hash: BytesN<32>)` | Admin: upgrades an operation's token, and makes that code the one new operations deploy from |
| `oplend_wasm_hash() -> BytesN<32>` | Code new operations deploy from |
| `backend_signer() -> BytesN<32>` | The backend key (op-lend tokens and the wallet read it from here) |
| `update_stablecoin_oracle_address(new_oracle)` | Admin: changes the stablecoin feed |

### Changed signatures

| Function | `main` | `develop` |
| --- | --- | --- |
| `create_operation` | `(op_name, total_shares, eur_per_shares) -> Address` | `(op_name, total_shares, eur_per_share) -> Address` (argument renamed; matters for named-argument callers such as the CLI: `--eur_per_share`) |
| `refund_user` | `(id, user)` | `(id, user, shares_to_burn: i128, usdc_to_refund: i128, eurc_to_refund: i128)` |
| `batch_refund_users` | `(id, users: Vec<Address>, len: u32)` | `(id, entries: Vec<RefundEntry>)` |

### Types

| Type | Change |
| --- | --- |
| `Operation` | Field `eur_per_shares` renamed `eur_per_share`. Fields now: `op_token: Address`, `total_shares: i128`, `eur_per_share: i128`, `op_name: String` |
| `RefundEntry` | **New**: `{ user: Address, shares_to_burn: i128, usdc_to_refund: i128, eurc_to_refund: i128 }` |

### Errors

Every code changed: removed errors no longer leave gaps.

| Error | `main` | `develop` | Change |
| --- | ---: | ---: | --- |
| `NotInitialized` | 2 | 1 | renumbered |
| `OperationNotFound` | 3 | 2 | renumbered |
| `OperationFinished` | 4 | 3 | renumbered |
| `OperationNotStarted` | 5 | 4 | renumbered |
| `OperationCanceled` | 7 | 5 | renumbered |
| `OperationPaused` | 8 | 6 | renumbered |
| `TooManyShares` | 10 | 7 | renumbered |
| `InvalidShares` | 11 | 8 | renumbered |
| `InvalidAmount` | 12 | 9 | renumbered |
| `Blacklisted` | 13 | 10 | renumbered |
| `NonceAlreadyUsed` | 14 | 11 | renumbered |
| `NothingToWithdraw` | 18 | 12 | renumbered |
| `SelfTargetNotAllowed` | 21 | 13 | renumbered |
| `OracleNoPrice` | 22 | 14 | renumbered |
| `OracleInvalidPrice` | 23 | 15 | renumbered |
| `OracleStalePrice` | 24 | 16 | renumbered |
| `InsufficientEscrow` | 25 | 17 | renumbered |
| `InvalidOperationId` | — | 18 | new: `first_op_id` is 0 |
| `InvalidPrice` | — | 19 | new: `eur_per_share <= 0` |
| `RaiseOverflow` | — | 20 | new: `total_shares * eur_per_share` overflows |
| `ZeroAddressNotAllowed` | — | 21 | new: withdrawal to the all-zero account / contract id |
| `StateUnchanged` | — | 22 | new: start / cancel / pause / resume that changes nothing |
| `AmountTooSmall` | — | 23 | new: an order or quote that rounds to zero |
| `UsdcOffPeg` | — | 24 | new: `invest` while USDC/USD is outside [0.98, 1.02] |
| `InsufficientShareBalance` | — | 25 | new: burning more shares than the holder has |
| `EurcOffPeg` | — | 26 | new: `invest_eur` while EURC/EUR is outside [0.98, 1.02] |
| `AlreadyInitialized` | 1 | — | removed |
| `OperationAlreadyStarted` | 6 | — | removed |
| `PredepositsClosed` | 9 | — | removed |
| `BatchTooLarge` | 15 | — | removed |
| `InvalidBatchLen` | 16 | — | removed |
| `NoParticipation` | 19 | — | removed |
| `NoOpLendBalance` | 20 | — | removed |

### Events

| Event | Change |
| --- | --- |
| `Refunded` | **Changed**: data gains `eurc_amount`. Topics `["refunded", investor, operation_id]`, data `{ usdc_amount, eurc_amount, shares_refunded }` |
| `FundingWithdrawn` | **Changed**: data gains `eurc_amount`. Topics `["funding_withdrawn", operation_id]`, data `{ usdc_amount, eurc_amount }` |
| `RefundedFiat` | **New**: topics `["refunded_fiat", investor, oplend_holder, operation_id]`, data `{ shares_refunded }`. Emitted by `fiat_refund`, together with a `Refunded` carrying zero USDC and EURC |
| `NonceVoided` | **New**: topics `["nonce_voided", nonce]`, no data |
| `Predeposit`, `Gifted`, `ClaimedOpToken`, `PredepositsOpen`, `PredepositsClosed` | **Removed** |
| `OperationFinished` | Same shape, **new unit**: `amount_raised_euro` is 6-decimal EUR (was 12-decimal; 2 EUR is now `2_000_000`) |
| `Invested` | Same shape. `usdc_amount` is the USDC paid for `invest`, and for `invest_eur` / `fiat_invest` the USD value of the purchase at the current EUR/USD rate, in USDC base units |

Unchanged: `OperationCreated`, `OperationStarted`, `OperationCanceled`,
`OperationPaused`, `OperationResumed`, `InvestedFiat`.

### Behavior changes behind unchanged signatures

**Investing**
- `invest` fails with `UsdcOffPeg` while USDC/USD is outside [0.98, 1.02]
  (bounds accepted). `invest_eur` fails with `EurcOffPeg` while EURC/EUR is
  outside the same band. Both can also fail with the oracle errors when the
  stablecoin feed has no price, a non-positive one, or one older than 24 h.
- `fiat_invest` now requires a started operation (`OperationNotStarted`), like
  `invest`.
- An order too small to cost one base unit fails with `AmountTooSmall`; before
  it was sold for 1 base unit. The same applies to `get_amount_in` and
  `get_amount_out`.
- Filling an operation no longer starts it: operations start only through
  `start_operation`.
- One `ONCHAIN_INVEST` signature pays with either coin, once: `invest` and
  `invest_eur` share nonces.

**Operations**
- `create_operation` rejects `eur_per_share <= 0` (`InvalidPrice`) and a raise
  that overflows (`RaiseOverflow`).
- `start_operation`, `cancel_operation` and `pause_funding` fail with
  `StateUnchanged` when nothing would change, instead of succeeding and
  emitting an event.

**Refunds** (`refund_user`, `batch_refund_users`)
- The admin decides the shares to burn and the USDC and EURC to repay; they are
  no longer derived from the position. Negative amounts, or all three zero,
  fail with `InvalidShares` / `InvalidAmount`. Burning more than the user holds
  fails with `InsufficientShareBalance`.
- For each coin, the operation's escrow pays if it holds the full amount;
  otherwise the **admin's own wallet** pays the full amount (the admin's
  signature must cover that nested token transfer, so simulate before
  signing); `InsufficientEscrow` only when neither covers it.
- `usdc_raised_per_client` / `eurc_raised_per_client` are a record of what was
  paid in and are **no longer reset by a refund** (on `main` a refund set them
  to 0). `usdc_raised` / `eurc_raised` still drop by what the escrow refunds.

**Withdrawals** (`withdraw_funds`)
- Sends both coins; fails with `NothingToWithdraw` only when both pending
  amounts are zero.
- Refuses the factory itself as destination (`SelfTargetNotAllowed`) and the
  all-zero account / contract ids (`ZeroAddressNotAllowed`).
- `funds_withdrawn` becomes true after any withdrawal, whichever coin moved.

**Other**
- `update_backend_signer` now also rotates the key used by the new op-lend
  tokens and by the wallet (they read it from the factory).
- `oplend_upgrade` also sets the code future operations deploy from.

### Legacy operations

- Operations created by the old factory stay on the old factory: their ids are
  unknown to the new one, and their op-lend tokens keep the `main` token code
  (no `upgrade`, signer stored in the token and rotated only through the old
  factory's `oplend_update_backend_signer`). Apps that still show or refund
  them need the old factory address.
- The upgraded wallet registers operations by id through the **new** factory,
  so a legacy operation can no longer be added to its registry. Register any
  legacy operation the wallet must custody **before** upgrading the wallet
  (old interface: `register_op_lend(op_id, op_lend)`). Existing registrations
  are kept by the upgrade.

---

## OpLend (`OpLendToken`)

### Setup

| `main` | `develop` |
| --- | --- |
| `__constructor(admin, decimal: u32, name, symbol, max_supply, backend_signer: BytesN<32>)` | `__constructor(admin, name, symbol, max_supply, factory: Address)` |

The factory deploys tokens, so apps don't call this. `decimals()` is always 6.

### Removed functions

| Function | Replacement |
| --- | --- |
| `admin_burn(user, amount)` | `burn(from, amount)` (now admin-only) |
| `update_backend_signer(new_signer)` | none: the signer is read from the factory |

### Added functions

| Function | Purpose |
| --- | --- |
| `factory() -> Address` | Factory the token reads the backend signer from |
| `void_nonce(nonce, signature: Option<BytesN<64>>)` | Burns an unspent whitelist nonce; without a signature it needs the admin (the factory: use `factory.oplend_void_nonce`) |
| `upgrade(new_wasm_hash)` | Admin (the factory: use `factory.oplend_upgrade`) |

### Errors

| Error | `main` | `develop` | Change |
| --- | ---: | ---: | --- |
| `NotInitialized` | 1 | 1 | unchanged |
| `NegativeAmount` | 3 | 2 | renumbered |
| `SupplyCapExceeded` | 4 | 3 | renumbered |
| `InsufficientBalance` | 5 | 4 | renumbered |
| `InsufficientAllowance` | 6 | 5 | renumbered |
| `NotWhitelisted` | 7 | 6 | renumbered |
| `NonceAlreadyUsed` | 8 | 7 | renumbered |
| `BadAllowanceExpiration` | 9 | 8 | renumbered |
| `NotAdmin` | — | 9 | new: `burn_from` by anyone but the admin |
| `DecimalTooLarge` | 2 | — | removed |

### Events

| Event | Change |
| --- | --- |
| `NonceVoided` | **New**: topics `["nonce_voided", nonce]`, no data |

All SEP-41 events are unchanged.

### Behavior changes behind unchanged signatures

- `burn(from, amount)` needs the **admin's** auth (the factory), not `from`'s,
  and burns without an allowance. Holders can no longer burn their own shares.
- `burn_from(spender, from, amount)` fails with `NotAdmin` unless `spender` is
  the admin.
- `whitelist_user` checks the signature before the nonce: a bad signature on a
  spent nonce now fails with the signature error instead of
  `NonceAlreadyUsed`. The signer is the factory's current `backend_signer()`.

---

## OpLendWallet

Upgraded in place: same address, registry and used nonces kept.

### Setup

| `main` | `develop` |
| --- | --- |
| `__constructor(admin, backend_signer: BytesN<32>)` | `__constructor(admin, factory: Address)` (fresh deployments only) |

### Removed functions

| Function | Replacement |
| --- | --- |
| `update_backend_signer(new_signer)` | `set_factory(factory)` once, then the factory's `update_backend_signer` |

### Added functions

| Function | Purpose |
| --- | --- |
| `set_factory(factory)` | Admin, **one-shot**: records the factory the backend signer is read from. Fails with `FactoryAlreadySet` once a factory is recorded |
| `factory() -> Address` | The recorded factory |
| `void_nonce(nonce, signature: Option<BytesN<64>>)` | Burns an unspent redeem nonce; without a signature it needs the admin |

### Changed signatures

| Function | `main` | `develop` |
| --- | --- | --- |
| `register_op_lend` | `(op_id, op_lend: Address)` | `(op_id)`: the token is read from `factory.get_operation(op_id)` |
| `register_op_lends` | `(entries: Vec<OpLendEntry>)` | `(op_ids: Vec<u32>)` |

### Types

| Type | Change |
| --- | --- |
| `OpLendEntry` | **Removed** |

### Errors

| Error | `main` | `develop` | Change |
| --- | ---: | ---: | --- |
| `NotInitialized` | 1 | 1 | unchanged |
| `OperationNotRegistered` | 2 | 2 | unchanged |
| `InvalidAmount` | 4 | 4 | unchanged |
| `NonceAlreadyUsed` | 5 | 5 | unchanged |
| `EmptyBatch` | 6 | 6 | unchanged |
| `SelfTargetNotAllowed` | 8 | 8 | unchanged |
| `FactoryAlreadySet` | — | 9 | new |
| `NotAContract` | 3 | — | removed (code stays reserved) |
| `BatchTooLarge` | 7 | — | removed (code stays reserved) |

### Events

| Event | Change |
| --- | --- |
| `BackendSignerUpdated` | **Removed** |
| `FactoryUpdated` | **New**: topics `["factory_updated"]`, data `{ factory }` |
| `NonceVoided` | **New**: topics `["nonce_voided", nonce]`, no data |

Unchanged: `Redeemed`, `AdminRedeemed`, `OpLendRegistered`, `AdminUpdated`.

### Behavior changes behind unchanged signatures

- `backend_signer()` reads the key from the factory. Between the upgrade and
  `set_factory`, `backend_signer()`, `factory()`, `redeem`,
  `whitelist_and_redeem` and a signed `void_nonce` fail with `NotInitialized`
  (1).
- `register_op_lend(s)` refuses an id the factory does not know (one unknown id
  fails the whole batch). The 200-entry batch cap is removed; an empty batch
  still fails with `EmptyBatch`.

---

## Rewards (`LendRewards`)

Upgraded in place: same address, roots and claims kept. Error codes are
unchanged.

| Change | Detail |
| --- | --- |
| `set_reward_token(new_token)` | **Removed**: the reward token is fixed at deployment |
| `RewardTokenUpdated` event | **Removed** |
| `claim_op_epochs` | A zero or negative amount on **any** epoch of the batch now fails the whole batch with `InvalidClaimBalance` (3), before anything is paid or marked claimed. `claim_op_epoch` already refused it |

---

## Signed messages

All backend signatures are ed25519 over the raw message bytes. Strkeys are
the 56-byte ASCII `G...` / `C...` form, integers are big-endian, and the nonce
is the UTF-8 string bytes.

| Message | Layout | Status |
| --- | --- | --- |
| Invest (`invest`, `invest_eur`) | `"ONCHAIN_INVEST"` ‖ factory ‖ `id` (u32) ‖ user ‖ `shares_amount` (i128) ‖ nonce | Unchanged layout. Sign against the **new factory address**. Now also valid for `invest_eur` |
| Fiat invest | `"FIAT_INVEST"` ‖ factory ‖ `id` (u32) ‖ user ‖ oplend_holder ‖ `shares_amount` (i128) ‖ nonce | Unchanged layout; **new factory address** |
| Op-lend whitelist (`whitelist_user`) | token ‖ user ‖ nonce | Unchanged layout; **new token addresses** |
| Wallet redeem (`redeem`, `whitelist_and_redeem`) | `"OP_REDEEM"` ‖ wallet ‖ `op_id` (u32) ‖ destination ‖ `amount` (i128) ‖ nonce | Unchanged (same wallet address) |
| Void nonce (`void_nonce` on factory, op-lend, wallet) | `"VOID_NONCE"` ‖ contract ‖ nonce | **New**. `contract` is the contract whose nonce is voided |

The signing key is the factory's `backend_signer()` for all of them. The new
tokens and the upgraded wallet read it from the factory, so one
`update_backend_signer` rotates it everywhere.

---

## Appendix: full `develop` interfaces

`// NEW` and `// CHANGED` mark differences from `main`.

### Factory

```rust
fn __constructor(admin: Address, usdc: Address, eurc: Address, oracle: Address, stablecoin_oracle: Address, backend_signer: BytesN<32>, oplend_wasm_hash: BytesN<32>, first_op_id: u32)  // NEW
fn backend_signer() -> BytesN<32>  // NEW
fn batch_refund_users(id: u32, entries: Vec<RefundEntry>)  // CHANGED
fn blacklist(user: Address, state: bool)
fn blacklisted(user: Address) -> bool
fn cancel_operation(id: u32)
fn create_operation(op_name: String, total_shares: i128, eur_per_share: i128) -> Address  // CHANGED
fn eurc() -> Address  // NEW
fn eurc_raised(id: u32) -> i128  // NEW
fn eurc_raised_per_client(id: u32, user: Address) -> i128  // NEW
fn eurc_withdrawn_amount(id: u32) -> i128  // NEW
fn fiat_invest(id: u32, shares_amount: i128, user: Address, oplend_holder: Address, nonce: String, signature: BytesN<64>)
fn fiat_refund(id: u32, user: Address, oplend_holder: Address, shares_amount: i128)  // NEW
fn funding_paused(id: u32) -> bool
fn funding_progress(id: u32) -> i128
fn funds_withdrawn(id: u32) -> bool  // NEW
fn get_amount_in(id: u32, shares_amount: i128) -> i128
fn get_amount_in_eur(id: u32, shares_amount: i128) -> i128  // NEW
fn get_amount_out(id: u32, usdc_amount: i128) -> i128
fn get_amount_out_eur(id: u32, eurc_amount: i128) -> i128  // NEW
fn get_operation(id: u32) -> Operation
fn invest(user: Address, id: u32, shares_amount: i128, nonce: String, signature: BytesN<64>)
fn invest_eur(user: Address, id: u32, shares_amount: i128, nonce: String, signature: BytesN<64>)  // NEW
fn is_operation_finished(id: u32) -> bool
fn operation_canceled(id: u32) -> bool
fn operation_count() -> u32
fn operation_started(id: u32) -> bool
fn operations(id: u32) -> Operation
fn oplend_admin_burn(op_id: u32, user: Address, value: i128)
fn oplend_upgrade(op_id: u32, new_wasm_hash: BytesN<32>)  // NEW
fn oplend_void_nonce(op_id: u32, nonce: String)  // NEW
fn oplend_wasm_hash() -> BytesN<32>  // NEW
fn oplend_whitelist_user(op_id: u32, user: Address, state: bool)
fn pause_funding(id: u32, state: bool)
fn refund_user(id: u32, user: Address, shares_to_burn: i128, usdc_to_refund: i128, eurc_to_refund: i128)  // CHANGED
fn set_oplend_wasm_hash(oplend_wasm_hash: BytesN<32>)
fn start_operation(id: u32)
fn transfer_ownership(new_admin: Address)
fn update_backend_signer(new_signer: BytesN<32>)
fn update_oracle_address(new_oracle: Address)
fn update_stablecoin_oracle_address(new_oracle: Address)  // NEW
fn upgrade(new_wasm_hash: BytesN<32>)
fn usdc() -> Address
fn usdc_raised(id: u32) -> i128
fn usdc_raised_per_client(id: u32, user: Address) -> i128
fn usdc_withdrawn_amount(id: u32) -> i128
fn void_nonce(nonce: String, signature: Option<BytesN<64>>)  // NEW
fn withdraw_funds(id: u32, destination: Address)  // NEW

struct Operation { op_token: Address, total_shares: i128, eur_per_share: i128, op_name: String }  // CHANGED
struct RefundEntry { user: Address, shares_to_burn: i128, usdc_to_refund: i128, eurc_to_refund: i128 }  // NEW

event FundingWithdrawn   topics ["funding_withdrawn", operation_id: u32]                                     data { usdc_amount: i128, eurc_amount: i128 }  // CHANGED
event Invested           topics ["invested", investor: Address, operation_id: u32]                           data { usdc_amount: i128, shares_bought: i128 }
event InvestedFiat       topics ["invested_fiat", investor: Address, oplend_destination: Address, operation_id: u32]  data { shares_bought: i128 }
event NonceVoided        topics ["nonce_voided", nonce: String]                                               data {}  // NEW
event OperationCanceled  topics ["operation_canceled", operation_id: u32]                                    data {}
event OperationCreated   topics ["operation_created", op_token: Address, operation_id: u32]                  data { total_shares: i128 }
event OperationFinished  topics ["operation_finished", operation_id: u32]                                    data { amount_raised_euro: i128 }  // unit changed
event OperationPaused    topics ["operation_paused", operation_id: u32]                                      data {}
event OperationResumed   topics ["operation_resumed", operation_id: u32]                                     data {}
event OperationStarted   topics ["operation_started", operation_id: u32]                                     data {}
event Refunded           topics ["refunded", investor: Address, operation_id: u32]                           data { usdc_amount: i128, eurc_amount: i128, shares_refunded: i128 }  // CHANGED
event RefundedFiat       topics ["refunded_fiat", investor: Address, oplend_holder: Address, operation_id: u32]  data { shares_refunded: i128 }  // NEW
```

### OpLend

```rust
fn __constructor(admin: Address, name: String, symbol: String, max_supply: i128, factory: Address)  // CHANGED
fn allowance(from: Address, spender: Address) -> i128
fn approve(from: Address, spender: Address, amount: i128, expiration_ledger: u32)
fn balance(id: Address) -> i128
fn burn(from: Address, amount: i128)  // admin-only now
fn burn_from(spender: Address, from: Address, amount: i128)  // admin-only now
fn decimals() -> u32
fn factory() -> Address  // NEW
fn get_allowance(from: Address, spender: Address) -> Option<AllowanceValue>
fn is_whitelisted(user: Address) -> bool
fn max_supply() -> i128
fn mint(to: Address, amount: i128)
fn name() -> String
fn set_admin(new_admin: Address)
fn symbol() -> String
fn total_supply() -> i128
fn transfer(from: Address, to_muxed: MuxedAddress, amount: i128)
fn transfer_from(spender: Address, from: Address, to: Address, amount: i128)
fn upgrade(new_wasm_hash: BytesN<32>)  // NEW
fn void_nonce(nonce: String, signature: Option<BytesN<64>>)  // NEW
fn whitelist_user(user: Address, nonce: String, signature: BytesN<64>)
fn whitelist_user_admin(user: Address, state: bool)

event NonceVoided  topics ["nonce_voided", nonce: String]  data {}  // NEW
// SEP-41 events unchanged: Approve, Burn, Clawback, Mint, MintWithAmountOnly,
// SetAdmin, Transfer, TransferWithAmountOnly
```

### OpLendWallet

```rust
fn __constructor(admin: Address, factory: Address)  // CHANGED
fn admin() -> Address
fn admin_redeem(op_id: u32, destination: Address, amount: i128)
fn backend_signer() -> BytesN<32>  // now read from the factory
fn factory() -> Address  // NEW
fn nonce_used(nonce: String) -> bool
fn op_lend(op_id: u32) -> Address
fn op_lend_balance(op_id: u32) -> i128
fn redeem(op_id: u32, destination: Address, amount: i128, nonce: String, signature: BytesN<64>)
fn register_op_lend(op_id: u32)  // CHANGED
fn register_op_lends(op_ids: Vec<u32>)  // CHANGED
fn set_admin(new_admin: Address)
fn set_factory(factory: Address)  // NEW
fn upgrade(new_wasm_hash: BytesN<32>)
fn void_nonce(nonce: String, signature: Option<BytesN<64>>)  // NEW
fn whitelist_and_redeem(op_id: u32, destination: Address, amount: i128, t_nonce: String, t_signature: BytesN<64>, r_nonce: String, r_signature: BytesN<64>)

event AdminRedeemed     topics ["admin_redeemed", destination: Address, operation_id: u32]  data { amount: i128 }
event AdminUpdated      topics ["admin_updated", admin: Address]                            data { new_admin: Address }
event FactoryUpdated    topics ["factory_updated"]                                          data { factory: Address }  // NEW
event NonceVoided       topics ["nonce_voided", nonce: String]                              data {}  // NEW
event OpLendRegistered  topics ["op_lend_registered", operation_id: u32]                    data { op_lend: Address }
event Redeemed          topics ["redeemed", destination: Address, operation_id: u32]        data { amount: i128 }
```

### Rewards

```rust
fn __constructor(admin: Address, reward_token: Address)
fn claim_op_epoch(op_id: u32, user: Address, epoch: u32, claimed_balance: i128, merkle_proof: Vec<BytesN<32>>)
fn claim_op_epochs(op_id: u32, user: Address, claims: Vec<ClaimData>)
fn distribute_op_rewards(op_id: u32, epoch: u32, merkle_root: BytesN<32>, total_allocation: i128)
fn emergency_withdraw(token_addr: Address)
fn op_claimed(op_id: u32, epoch: u32, user: Address) -> bool
fn op_merkle_root(op_id: u32, epoch: u32) -> BytesN<32>
fn reward_token() -> Address
fn set_admin(new_admin: Address)
fn upgrade(new_wasm_hash: BytesN<32>)
fn verify_op_claim(op_id: u32, user: Address, epoch: u32, claimed_balance: i128, merkle_proof: Vec<BytesN<32>>) -> bool

struct ClaimData { epoch: u32, balance: i128, merkle_proof: Vec<BytesN<32>> }

event Claimed             topics ["claimed", op_id: u32, user: Address]          data { balance: i128 }
event EmergencyWithdrawn  topics ["emergency_withdrawn", token: Address]          data { amount: i128 }
event RewardsDistributed  topics ["rewards_distributed", op_id: u32, epoch: u32]  data { amount: i128 }
// removed: fn set_reward_token(new_token: Address), event RewardTokenUpdated
```
