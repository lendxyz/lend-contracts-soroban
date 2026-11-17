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
