# Audit notes

Here will be the changelog after audit recommendations fixes

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
