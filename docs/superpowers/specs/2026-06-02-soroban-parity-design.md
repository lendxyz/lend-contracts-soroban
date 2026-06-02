# Soroban Parity Design — EVM → Soroban Translation

Date: 2026-06-02

Translate the Solidity EVM contracts (`evm-reference/`) into Soroban, reaching feature
parity except for LayerZero / OFT cross-chain features, which Soroban does not support.

Two contracts:

- **op-lend** (`contracts/op-lend`) — the `LendOperation` token. Already a working SEP-41
  token; needs the access-control + supply-cap + backend-signature features added.
- **factory** (`contracts/factory`) — the EVM Diamond (`DiamondProxy` + facets). Soroban has
  no facets/delegatecall, so all facets collapse into one `LendFactory` contract.

## Locked decisions

1. **Backend auth → ed25519 + nonce.** Backend signer is an ed25519 public key
   (`BytesN<32>`). Gated calls take a `signature: BytesN<64>` and a `nonce: String`, verified
   with `env.crypto().ed25519_verify`. Used nonces are persisted to prevent replay.
2. **Oracle → Reflector (SEP-40).** Factory stores a Reflector oracle `Address`, set at
   `initialize`, changeable by admin. EUR/USD read via cross-contract `lastprice`.
3. **Scope → full parity, both contracts**, minus LZ/OFT.

## Cross-cutting conventions

- **Amounts are `i128`** everywhere (Soroban token + oracle convention), with non-negative
  guards. The existing factory `u128` amounts are migrated to `i128`.
- **Ownership** = stored `Admin` `Address` + `require_auth`. `transfer_ownership(new_admin)`
  added for `IERC173` parity. (No Diamond loupe/cut — not meaningful on Soroban.)
- **`chainid` dropped** from signed messages: each Soroban contract has a unique address, so
  including the contract address in the signed message already prevents cross-deploy replay.
- **Errors**: `Events.sol` custom errors become a `contracterror` enum (`FactoryError`) used
  via `panic_with_error!`, so revert reasons survive translation.

## op-lend token

Constructor (changed):

```
__constructor(admin: Address, decimal: u32, name: String, symbol: String,
              max_supply: i128, backend_signer: BytesN<32>)
```

`decimal` cap stays 6 (EVM `DECIMALS = 6`; existing test panic string updated to match).

New storage (`storage_types.rs`):

- `TotalSupply -> i128` (was untracked; needed for the cap)
- `MaxSupply -> i128`
- `BackendSigner -> BytesN<32>`
- `Whitelisted(Address) -> bool`
- `UsedNonce(String) -> bool`

Behavior:

- `mint(to, amount)` — admin only; `total_supply + amount <= max_supply` else panic;
  sets `whitelisted[to] = true`; increments `TotalSupply`.
- `admin_burn(user, amount)` — admin only; burns without allowance; decrements `TotalSupply`.
- `burn` / `burn_from` — also decrement `TotalSupply`.
- `transfer` / `transfer_from` — require `from` AND `to` whitelisted, else panic.
- `whitelist_user_admin(user, state)` — admin only.
- `whitelist_user(user, nonce, signature)` — anyone; backend-sig verified (domain
  `"WHITELIST"`). On success sets `whitelisted[user] = true`.
- `update_backend_signer(new)` — admin only.

New files: `whitelist.rs`, `crypto.rs` (shared ed25519 helper). Skipped (LZ/OFT):
`send`, `_debit`, `_credit`, `setPeer`, `batchSetPeers`, `token`, `sharedDecimals`,
`approvalRequired`.

## Signature scheme

ed25519 signs raw bytes (no pre-hash). The backend signs a canonical message built by
appending, in order, into a `Bytes`:

1. domain tag bytes — `ONCHAIN_INVEST` | `FIAT_INVEST` | `WHITELIST`
2. contract address (its XDR/string bytes)
3. op id (for factory messages)
4. user address bytes (+ `opLendHolder` for fiat)
5. amount (its bytes)
6. nonce string bytes

`crypto::verify_backend_sig(env, signer, msg, sig)` calls `ed25519_verify` (panics on bad
sig). Caller checks/sets the nonce. Lives in each contract's `crypto.rs`.

## Oracle (Reflector / SEP-40)

`oracle.rs` defines a Reflector client and `get_eur_usd_price(env) -> i128` scaled to `1e6`:

- `lastprice(Asset::Other(Symbol::new(env, "EUR"))) -> Option<PriceData { price: i128, timestamp: u64 }>`
- `decimals() -> u32`

Checks: price > 0; `now - timestamp <= 24h` (86400 s) else panic (EVM used 25h; rounded to
24h). Scale `price` from oracle decimals to 6 decimals (multiply/divide by `10^|d-6|`).

`PRICE_PRECISION = 1_000_000`, `SHARE_PRECISION = 1_000_000_000_000`.

- `get_amount_in(id, shares) -> i128`: `eur_cost = eur_per_shares * shares / 1e6`;
  `usdc = eur_cost * eur_usd / 1e6`; floor at 1.
- `get_amount_out(id, usdc) -> i128`: `shares = usdc * 1e12 / (eur_per_shares * eur_usd)`;
  floor at 1.

## Factory parity

All facets → methods on `LendFactory`. New storage keys (`types.rs` `DataKey`):
`Oracle`, `OperationCanceled(u32)`, `FundingPaused(u32)`, `PredepositsOpen(u32)`,
`Predeposits(u32, Address)`, `Gifted(u32, Address)`, `Blacklisted(Address)`.
(`UserInvested` already exists = `usdcRaisedPerClient`.)

`initialize` gains `oracle: Address`.

### Operations (`operations.rs`)

`create_operation` (update: pass `max_supply` + `backend_signer` to op-lend constructor),
`get_operation`, `is_operation_finished`, `cancel_operation`, `start_operation`,
`pause_funding`, `set_predeposits`.

### Invest (`invest.rs`)

Shared internal `_invest` guard chain (op exists / started / not finished / not canceled /
not paused / shares>0 / progress+shares<=total). Backend-sig + nonce check.

- `invest(user, id, shares, nonce, signature)` — blacklist check, `_invest`, mint to user.
- `fiat_invest(id, shares, user, oplend_holder, nonce, signature)` — admin-gated mint to
  holder + whitelist user; `FIAT_INVEST` domain.
- `gift_op_tokens(id, shares, user)` — admin only; records `Gifted`.
- `predeposit(id, user, shares, nonce, signature)` — requires predeposits open, op not
  started; records `Predeposits`.
- `claim_op_tokens(id, user)` / `claim_op_tokens_batch(id, users)` — mint gifted+predeposit,
  zero them. `MAX_BATCH = 200`.
- `get_amount_in` / `get_amount_out` (see Oracle).

Skipped (LZ): `invest_and_bridge`, `claim_op_tokens_and_bridge`, `_bridge`.

### Admin (`admin.rs`)

`refund_user`, `batch_refund_users`, `update_oracle_address`, `update_backend_signer`,
`blacklist`, `oplend_whitelist_user`, `oplend_update_backend_signer`, `oplend_admin_burn`,
`transfer_ownership`. Skipped (LZ): `set_oplend_peer`, `batch_set_oplend_peers`.

### Getters (`getters.rs`)

`usdc`, `operation_count`, `operations`/`get_operation`, `funding_progress`, `usdc_raised`,
`funding_paused`, `operation_started`, `usdc_withdrawn`, `operation_canceled`,
`usdc_raised_per_client`, `predeposits`, `gifted`, `claimable_total`, `predeposits_open`,
`blacklisted`. Non-existent op ids panic (matches EVM `OpNotExist`).

### Events (`events.rs`)

Full `Events.sol` set as `#[contractevent]`, minus `OpLendPeerAdded`: `ClaimedOpToken`,
`OperationStarted`, `OperationCreated`, `OperationPaused`, `OperationResumed`,
`OperationCanceled`, `OperationFinished`, `Refunded`, `InvestedFiat`, `Invested`, `Gifted`,
`Predeposit`, `PredepositsOpen`, `PredepositsClosed`.

## File layout

```
contracts/op-lend/src/
  contract.rs      # #[contractimpl] surface
  whitelist.rs     # whitelist read/write + sig-gated whitelist_user
  crypto.rs        # ed25519 verify helper
  storage_types.rs # extended DataKey + consts
  ... (admin/allowance/balance/metadata unchanged or lightly extended)

contracts/factory/src/
  contract.rs      # #[contractimpl] surface + initialize
  operations.rs
  invest.rs
  admin.rs
  getters.rs
  oracle.rs        # Reflector client + price math
  crypto.rs        # ed25519 verify helper
  events.rs
  types.rs         # DataKey, Operation, client traits
  create_oplend.rs # unchanged deploy logic
  utils.rs         # concat_str, u32_to_string
```

## Testing

- op-lend: extend existing tests — supply cap, whitelist gating on transfer, admin_burn,
  whitelist_user happy/replay path (mock ed25519 via generated keypair in tests).
- factory: build a test harness registering factory + op-lend wasm + a mock Reflector oracle
  contract + a mock USDC token. Cover create/start/invest/predeposit/claim/refund/cancel/
  pause/blacklist and getAmountIn/Out. Sign messages in-test with an ed25519 keypair.

## Out of scope (LZ/OFT, explicitly skipped)

op-lend: `send`, `_debit`, `_credit`, `setPeer`, `batchSetPeers`, `token`, `sharedDecimals`,
`approvalRequired`. factory: `investAndBridge`, `claimOpTokensAndBridge`, `_bridge`,
`setOpLendPeer`, `batchSetOpLendPeers`. Diamond: `DiamondCut`/`DiamondLoupe`/`IERC165`.
