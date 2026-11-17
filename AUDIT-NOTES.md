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
