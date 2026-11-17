# Audit notes

Here is the changelog after audit recommendations fixes

# Upgrade path:

- `Factory` and `OpLend` will be redeployed on a fresh instance
- `Rewards` and `OpLendWallet` will be upgraded

# Changelog:

## `void_nonce` on factory, op-lend and op-lend-wallet

> Addresses **Factory C2**, **OpLend C2** and **OpLendWallet C1**

`void_nonce(nonce, signature: Option<BytesN<64>>)` burns an unspent nonce so
the backend signature carrying it can never be used.

- `Some(sig)`: callable by anyone; `sig` must be the backend signer's ed25519
  signature over `"VOID_NONCE" || <contract strkey, 56 bytes> || nonce`. The
  original signature cannot be checked here: it also binds user / amount / op
  id, which this call does not take.
- `None`: requires the contract admin's auth. op-lend's admin is the factory,
  so its admin path is `factory.oplend_void_nonce(op_id, nonce)`.
- Fails with `NonceAlreadyUsed` if the nonce is already spent or voided.
- Emits `NonceVoided { nonce }` (nonce as topic).
- op-lend `whitelist_user` now verifies the signature before the nonce check
  (shared `consume_nonce` with `void_nonce`): a bad signature on a spent nonce
  now fails with the crypto error instead of `NonceAlreadyUsed`.

## OpLend: admin-only burning

> Addresses **OpLend C1**

- `burn` and `burn_from` stay (op-lend still implements SEP-41
  `TokenInterface`) but are admin-only:
  - `burn(from, amount)` requires the admin's auth instead of `from`'s, and
    burns from any holder without an allowance.
  - `burn_from(spender, from, amount)` panics with `NotAdmin` (new error 10)
    unless `spender` is the admin; it requires the admin's auth and spends the
    allowance as usual.
- `admin_burn` is removed; `burn` replaces it. The factory's refunds and
  `oplend_admin_burn` now call `burn`.

## Factory: predeposit / gift / claim removed

> Addresses **Factory C3**, **Factory C6** and **Factory C7**

The deprecated pre-deposit, gift and claim feature is removed from the factory:

- Entry points: `predeposit`, `gift_op_tokens`, `claim_op_tokens`,
  `claim_op_tokens_batch`, `set_predeposits`, and the getters
  `predeposits_open`, `predeposits`, `gifted`, `claimable_total`.
- Events: `Predeposit`, `Gifted`, `ClaimedOpToken`, `PredepositsOpen`,
  `PredepositsClosed`.
- Errors retired (codes not reused): `OperationAlreadyStarted` (6),
  `PredepositsClosed` (9), `BatchTooLarge` (15).
- Storage: the `F_PREDEPOSITS` flag bit, and `Position.predeposited` /
  `Position.gifted` (`Position` now only holds `invested`). Fresh deployment,
  so no stored data needs migrating.
- Operations now start only through `start_operation`; filling one no longer
  starts it automatically.

## Factory: setup moved to `__constructor`

> Addresses **Factory C17**

- `initialize(...)` is replaced by `__constructor(admin, usdc, oracle,
  backend_signer, oplend_wasm_hash, first_op_id)`, which runs inside the deploy
  transaction. There is no longer a window between deploy and setup in which
  anyone could call `initialize` first and take the admin role.
- Error `AlreadyInitialized` (1) is retired (code not reused): a constructor
  cannot run twice.
- New `first_op_id`: the first operation gets this id, so the redeployed
  factory continues the old one's numbering (op ids key the upgraded op-lend
  wallet registry and rewards roots). `0` fails with `InvalidOperationId` (26).
- `scripts/deploy-factory.sh` passes the setup as constructor args (requires
  `FIRST_OP_ID`), and its deterministic deploy salt now includes the factory
  wasm hash: the old salt derived the existing factory id on both networks, so
  a fresh deploy would have been skipped as "already deployed".

## Factory: `OperationFinished.amount_raised_euro` in 6 decimals

> Addresses **Factory C8**

- `amount_raised_euro` was `total_shares * eur_per_shares`: two 6-decimal
  values, so a 12-decimal amount (2 shares at 1 EUR emitted `2_000_000_000_000`).
- It is now divided by `PRICE_PRECISION` (1e6), the same rescale `amount_in`
  uses to price shares, so it is 6-decimal EUR like every other internal amount
  (`2_000_000` = 2 EUR).
- Indexers that read this field must treat it as 6-decimal.

## Factory: `fiat_invest` requires a started operation

> Addresses **Factory C9**

- `fiat_invest` minted shares on operations that were not started yet. It now
  fails with `OperationNotStarted`, like `invest`.
- Both paths share one guard (`load_started`: not finished, then started), so
  they cannot drift apart again.

## OpLend: upgradable through the factory

> Addresses **OpLend C3**

- op-lend gains `upgrade(new_wasm_hash)`, admin-only like the factory and
  wallet ones. Its admin is the factory, so the call goes through the new
  `factory.oplend_upgrade(op_id, new_wasm_hash)`.
- `oplend_upgrade` also stores the hash as the code later `create_operation`
  calls deploy from (same slot as `set_oplend_wasm_hash`). If the token upgrade
  fails the whole call reverts, so the stored hash never points at code that
  did not install. New getter: `oplend_wasm_hash()`.
- `scripts/upgrade-contract.sh` supports `CONTRACT=oplend OP_ID=<id>`: it finds
  the token through `get_operation`, upgrades it via the factory, then checks
  both the token's live code and the factory's stored hash.
- Tokens created by the old factory run op-lend code without `upgrade()` and
  stay frozen; only operations from the fresh deployment can be upgraded.

## `upgrade` extends the TTL of the new code, not the replaced one

> Addresses **Factory C19** and **OpLendWallet C6**

- `instance().extend_ttl` extends both the contract instance and its code
  entry. `upgrade` called it *before* `update_current_contract_wasm`, so it
  kept the outgoing wasm alive and left the incoming one on its upload TTL.
- The factory and wallet `upgrade` now swap the code first, then bump.
- Same fix applied where no ticket covers it: op-lend `upgrade` (its
  `require_admin` bumps before the swap, so it bumps again after) and rewards
  `upgrade` (which did not bump at all).
- Regression tests in all four crates check that after an upgrade the code's
  TTL matches the instance's; they fail with the old order.

## Factory: `create_operation` validates the price and total raise

> Addresses **Factory C16**

- A zero or negative `eur_per_shares` is rejected with `InvalidPrice` (27).
- `total_shares * eur_per_shares` is checked with `checked_mul`; if it does not
  fit in an `i128`, creation fails with `RaiseOverflow` (28). Before, the
  operation was created and the overflow surfaced only when its last share
  sold (computing `amount_raised_euro`), so it could never finish.

## Factory: `withdraw_usdc` rejects stranding destinations

> Addresses **Factory C18**

- Withdrawing to the factory itself now fails with `SelfTargetNotAllowed`
  (21). Before, it succeeded: the amount counted as withdrawn but the USDC
  stayed in the shared balance, outside every operation's escrow.
- Stellar has no `address(0)`; the nearest equivalents are the all-zero
  account key (`GAAA…AWHF`) and contract id (`CAAA…BSC4`), valid addresses no
  one controls. Both now fail with the new `ZeroAddressNotAllowed` (29).
- Checked before any state change.

## Factory: unnecessary or misleading code removed

> Addresses **Factory C14**

- `SHARE_PRECISION` (1e12) is deleted: its only use, `amount_out`, now
  divides with `PRICE_PRECISION * PRICE_PRECISION`, as `amount_in` already did.
  Same value, so quotes are unchanged.
- `batch_refund_users(id, users)` drops its `len` argument and refunds every
  address in the list. Error `InvalidBatchLen` (16) is retired (code not
  reused).
- `MAX_BATCH` / `claim_op_tokens_batch`: already removed with the pre-deposit
  and gift feature (Factory C3).
- `read_usdc_withdrawn` drops the fallback for an operation flagged withdrawn
  without a stored amount (a state only the previous code could produce); it
  reads the stored amount, defaulting to zero. Its legacy test is deleted.
- Comments fixed: `bump_instance` (was cut mid-sentence), USDC decimals (the
  testnet DummyUSDC has 7, not 6), and `amount_in` ("shares of an operation").
- Not done: renaming `eur_per_shares` to `eur_per_share`. The name is part of
  the public interface (`create_operation` argument, `Operation` field from
  `get_operation`) that `lend-app` and the scripts read, so it is kept to avoid
  breaking them.

## Factory: no silent default for cached decimals

> Addresses **Factory C12**

- `usdc_decimals` and the oracle decimals read in `get_eur_usd_price` fell
  back to a hard-coded scale when their cached value was missing, so pricing
  would run against a guessed scale instead of failing.
- Both now fail with `NotInitialized`, like the other config reads. The
  constructor always writes both (and `update_oracle_address` rewrites the
  oracle one), so only a broken setup can reach this.

## Factory: start / cancel / pause check the current state

> Addresses **Factory C15**

- `start_operation`, `cancel_operation` and `pause_funding` now fail with the
  new `StateUnchanged` (30) when the operation is already in the requested
  state (already started, already canceled, already paused, or resuming one
  that is not paused). Before, they succeeded and emitted
  `OperationStarted` / `OperationCanceled` / `OperationPaused` /
  `OperationResumed` for a change that never happened.
- The check lives in the shared `set_flag`, which already detected the no-op
  but returned a `bool` every caller ignored.

## Factory: `amount_in` / `amount_out` fail on a non-positive result

> Addresses **Factory C13**

- Both returned `1` when the computed amount was zero or negative, so an order
  too small to cost one USDC base unit was sold for 1 base unit (and its
  shares minted). They now fail with the new `AmountTooSmall` (31); this
  covers `invest`, `fiat_invest`, `get_amount_in` and `get_amount_out`.
- A non-positive share price or EUR/USD rate can no longer reach them
  (`InvalidPrice` at `create_operation`, `OracleInvalidPrice` on the oracle
  read), so in practice the result is only non-positive for an order that
  rounds to zero.

## Factory: USDC purchases refused while USDC is off its peg

> Addresses **Factory C4**

- The factory prices shares as if 1 USDC were 1 USD. It now stores a second
  Reflector feed, `usdc_oracle`, and `invest` fails with the new `UsdcOffPeg`
  (32) while USDC/USD is outside [0.98, 1.02] (bounds included as on-peg).
- The feed is Reflector's CEX & DEX oracle, queried for `Other("USDC")`: the
  forex feed used for EUR has no USDC price. Verified on-chain 2026-11-17:
  14 decimals, base USD, ≈1.0007 on both networks (mainnet
  `CAFJZQWSED6YAWZU3GWRTOCNPPCGBN32L7QV43XX5LZLFTK6JLN34DLN`, testnet
  `CCYOZJCOPG34LLQQ7N24YXBM7LL62R7ONMZ3G6WZAAYPB5OYKOMJRN63`).
- The price goes through the same checks as the EUR rate (missing, non-positive
  or older than 24 h fails) via a shared `read_price`.
- New constructor argument `usdc_oracle` (after `oracle`) and admin setter
  `update_usdc_oracle_address`. `scripts/deploy-factory.sh` passes
  `USDC_ORACLE`, defaulted per network in `scripts/common.sh`.
- Only `invest` is gated: `fiat_invest` is settled in euros off-chain, refunds
  return the exact USDC paid, and the quote getters keep answering.
  `predeposit` and `gift_op_tokens`, also named in the finding, were removed
  under Factory C3.

## Factory: fiat refunds recorded on-chain

> Addresses **Factory C5**

- New admin-only `fiat_refund(id, user, oplend_holder, shares_amount)`: burns
  `shares_amount` shares from `oplend_holder` (normally the op-lend wallet that
  custodies euro buyers' shares) and lowers `funding_progress` by the same
  amount, so the shares are back on sale (a finished operation reopens).
- The factory now records each investor's euro-bought shares
  (`Position.fiat_shares`, incremented by `fiat_invest`, readable with
  `fiat_shares(id, user)`); `fiat_refund` cannot exceed it
  (`InsufficientFiatShares`, 33), so it cannot unwind USDC purchases.
- Emits `Refunded` with `usdc_amount: 0` plus a new `RefundedFiat`, mirroring
  `Invested` + `InvestedFiat` on the way in.

## Factory: refunds take admin-decided shares and USDC

> Addresses **Factory C1**

- `refund_one` repaid the whole recorded `invested` USDC and burned the
  user's whole share balance, two unrelated amounts: shares move between
  whitelisted holders, and euro buyers have no USDC recorded. A user could
  move his shares away just before the refund, get all his USDC back and keep
  the shares.
- `refund_user(id, user, shares_to_burn, usdc_to_refund)` and
  `batch_refund_users(id, entries: Vec<RefundEntry>)` now take both amounts
  from the admin.
- Burning more than the user holds fails with the new
  `InsufficientShareBalance` (34), before any transfer; the USDC is still
  capped by the operation's escrow (`InsufficientEscrow`). Negative amounts,
  or both zero, are rejected.
- `Position.invested` is no longer reduced: it stays the record of USDC paid
  in, which `usdc_to_refund` need not match.
- Errors `NoParticipation` (19) and `NoOpLendBalance` (20) are retired (codes
  not reused).

## Factory: missing tests added

> Addresses **Factory T1**

- The two fixed oracle mocks are replaced by one `MockFeed` that answers only
  for its own symbol and can drop, change or backdate its price; every
  existing test now also checks the factory asks for `EUR` / `USDC`.
- New tests in `contracts/factory/src/test.rs`:
  - `test_non_round_rate_is_charged_as_quoted`: 1.08345678912345 USD/EUR;
    quotes, charges and the round trip match values computed independently.
    (A purchase rounding to zero: `test_order_rounding_to_zero_fails`, C13.)
  - `test_oracle_failures_block_purchases_and_quotes`: no price, zero,
    negative, older than 24 h on the EUR feed (`invest`, `fiat_invest`,
    `get_amount_in`, `get_amount_out`) and on the USDC feed (`invest`);
    exactly 24 h is accepted.
  - `test_invest_rejects_tampered_signatures` /
    `test_fiat_invest_rejects_tampered_signatures`: another key, or the right
    signature with a different amount, user, operation or holder; the nonce
    stays usable.
  - `test_refunds_after_cancel`: started, partly sold, canceled; `invest` and
    `fiat_invest` fail, `refund_user` + `batch_refund_users` return all USDC.
  - `test_backend_key_rotation`: after `update_backend_signer` the factory
    takes only the new key while share tokens keep theirs until
    `oplend_update_backend_signer`.
