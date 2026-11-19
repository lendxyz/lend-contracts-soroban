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
- Errors removed: `OperationAlreadyStarted`, `PredepositsClosed`,
  `BatchTooLarge`.
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
- Error `AlreadyInitialized` is removed: a constructor cannot run twice.
- New `first_op_id`: the first operation gets this id, so the redeployed
  factory continues the old one's numbering (op ids key the upgraded op-lend
  wallet registry and rewards roots). `0` fails with `InvalidOperationId`.
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
  address in the list. Error `InvalidBatchLen` is removed.
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
- Errors `NoParticipation` and `NoOpLendBalance` are removed.

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
    takes only the new key, and so does an existing share token (it reads the
    signer from the factory since OpLend C4).

## OpLend / OpLendWallet: backend signer read from the factory

> Addresses **OpLend C4** and **OpLendWallet C3**

- The backend signer is stored only on the factory, which exposes a new
  read-only `backend_signer()`. Op-lend tokens and the op-lend wallet store
  the factory address instead of a key and read the signer from it on every
  signature check, so `factory.update_backend_signer` rotates it everywhere
  at once.
- op-lend: constructor takes `factory` (the factory passes itself) instead of
  `backend_signer`; `update_backend_signer` is removed, and so is the
  factory's `oplend_update_backend_signer` proxy. The factory is fixed at
  deploy: no function can change it, so nothing can repoint a token's signer.
  New `factory()` getter.
- op-lend-wallet: constructor takes `factory` instead of `backend_signer`;
  `update_backend_signer` (and its `BackendSignerUpdated` event) is replaced by
  `set_factory(factory)` (admin; the address must answer `backend_signer()`, emits
  `FactoryUpdated`). `backend_signer()` now reads through the factory; new
  `factory()` getter.
- Upgrading the deployed wallet: its constructor does not run again, so until
  `set_factory` is called it has no factory and redeems fail
  (`NotInitialized`). Run `scripts/wallet-set-factory.sh` (or
  `make wallet-set-factory`) right after the upgrade, pointing at the new
  factory. `deploy-op-lend-wallet.sh` now passes `FACTORY_ID`.
- Each signature check now costs one extra cross-contract read.

## OpLend: share token decimals must be 6

> Addresses **OpLend C5**

- The constructor no longer takes `decimal` (it accepted any value up to 6):
  the token always reports 6, the unit the factory prices shares in, so there
  is no argument to get wrong. The factory stops passing it.
- Error `DecimalTooLarge` is removed.

## OpLend: un-whitelist path tested

> Addresses **OpLend T1**

- New `test_whitelist_user_admin_revokes_and_mint_restores`: mints to a
  whitelisted user, revokes him with `whitelist_user_admin(user, false)`, then
  checks `is_whitelisted` is false, his balance is unchanged, and his transfer
  to a whitelisted user fails with `NotWhitelisted`; a later mint restores the
  right and the transfer succeeds.

## OpLendWallet: registry tokens come from the factory

> Addresses **OpLendWallet C4**

- `register_op_lend(op_id)` and `register_op_lends(op_ids)` take only
  operation ids and copy each token from the factory's `get_operation(id)`,
  instead of trusting an address passed in. An operation can no longer be
  mapped to another operation's token, and an id the factory does not know is
  refused (one unknown id fails the whole batch). `OpLendEntry` is removed.
- The wallet mirrors the factory's `Operation` struct to decode that record;
  `test_wallet_reads_operations_and_signer_from_the_factory` (factory tests)
  runs the real wallet wasm against the real factory, so a change to the
  factory's struct breaks a test rather than registration on chain.
- `require_contract` is removed, as recommended. `set_factory` and the
  constructor now check the factory by calling its `backend_signer()`, which
  also refuses a contract that is not a factory. Error `NotAContract` (3) is
  retired (code not reused).
- `scripts/wallet-register-op-lend.sh` takes `OP_ID`, or `OP_IDS='[1,2]'` for
  a batch (was `OPLEND` / `ENTRIES`).

## OpLendWallet: `MAX_BATCH` cap removed

> Addresses **OpLendWallet C5**

- `register_op_lends` no longer caps the batch at 200: an oversized batch
  already fails on the network's resource limits (the event limit alone stops
  it near 105 entries). `MAX_BATCH`, its check and error `BatchTooLarge` (7,
  code not reused) are removed; an empty list is still refused (`EmptyBatch`).

## Rewards: batch claims refuse a non-positive epoch

> Addresses **Rewards C1**

- `claim_op_epochs` never checked the sign of each epoch's amount: a negative
  entry (an admin mistake in a tree) was netted against the user's other
  epochs, which were then marked claimed, losing the difference for good.
- The check (`InvalidClaimBalance` for zero or negative) now lives in the
  shared `settle_claim`, so it applies to every epoch of a batch and to
  `claim_op_epoch` alike; one bad entry rejects the whole batch before
  anything is marked claimed or paid.

## Rewards: reward token fixed at deployment

> Addresses **Rewards C2**

- `set_reward_token` (and its `RewardTokenUpdated` event) is removed: the
  token is written once by the constructor. Tree leaves carry only amounts,
  so a switch changed the meaning of every unclaimed amount, could block
  claims until the new token was funded, and freed the old token for
  `emergency_withdraw`. Another reward token means another rewards contract.
- The deployed rewards contract is upgraded in place: its stored token stays
  as it is and simply can no longer change.

## Rewards: multi-operation, multi-user test

> Addresses **Rewards T1**

- New `test_claims_are_separate_per_operation_and_user`: two operations with
  different amounts, two users each claiming epoch 1 on both. Every claim
  succeeds and pays its own amount (130 and 240 in total), all four are
  recorded as claimed, an operation-A proof is refused on operation B, and a
  repeated claim is refused.

## Factory and OpLend: error codes renumbered

- Both contracts are redeployed, so the codes of removed errors are no longer
  reserved: each `Error` enum is renumbered from 1 with no gaps, in the same
  order. Clients decoding `Error(Contract, #n)` from these contracts must use
  the new codes.
- Factory (old → new): `NotInitialized` 2 → 1, `OperationNotFound` 3 → 2,
  `OperationFinished` 4 → 3, `OperationNotStarted` 5 → 4,
  `OperationCanceled` 7 → 5, `OperationPaused` 8 → 6, `TooManyShares` 10 → 7,
  `InvalidShares` 11 → 8, `InvalidAmount` 12 → 9, `Blacklisted` 13 → 10,
  `NonceAlreadyUsed` 14 → 11, `NothingToWithdraw` 18 → 12, and every code
  from `SelfTargetNotAllowed` (21) to `InsufficientShareBalance` (34) drops
  by 8 (13 to 26).
- OpLend: `NotInitialized` stays 1; every code from `NegativeAmount` (3) to
  `NotAdmin` (10) drops by 1 (2 to 9).
- OpLendWallet is upgraded, not redeployed: its retired code 3
  (`NotAContract`) stays reserved.
