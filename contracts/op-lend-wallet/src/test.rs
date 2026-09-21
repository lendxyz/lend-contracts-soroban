#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::Address as _, vec, Address, Bytes, BytesN, Env, String, Vec,
};

use crate::contract::{OpLendWallet, OpLendWalletClient};
use crate::errors::Error;
use crate::types::OpLendEntry;

// Op-lend wasm, built by `stellar contract build` (see the Makefile). The
// wallet's whole job is moving these tokens, and op-lend's whitelist rule is
// the reason `whitelist_and_redeem` exists, so the tests run against the real
// token rather than a mock.
mod oplend {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/lend_operation_token.wasm"
    );
}

const OP_ID: u32 = 7;
const MAX_SUPPLY: i128 = 1_000_000;
const FUNDED: i128 = 1_000;

/// A real, checksum-valid account (`G...`) strkey — the network's testnet
/// backend signer. `Address::generate` only ever hands out contract addresses,
/// so this is the only way to exercise the `NotAContract` guard.
const ACCOUNT_STRKEY: &str =
    "GAOQ67SJWIJSKZXKZTPWIQTRI6EGTDVDLRXSWUZHMMPGS3MVNGCOVEMA";

fn signer_key() -> SigningKey {
    SigningKey::from_bytes(&[11u8; 32])
}

fn rotated_key() -> SigningKey {
    SigningKey::from_bytes(&[12u8; 32])
}

fn pubkey(e: &Env, key: &SigningKey) -> BytesN<32> {
    BytesN::from_array(e, &key.verifying_key().to_bytes())
}

struct Setup<'a> {
    e: Env,
    admin: Address,
    wallet: OpLendWalletClient<'a>,
    wallet_id: Address,
    op_token: oplend::Client<'a>,
    op_token_id: Address,
}

/// A wallet holding `FUNDED` shares of operation `OP_ID`. The mint whitelists
/// the wallet on the token, which is what lets it transfer out at all.
fn setup<'a>() -> Setup<'a> {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let signer = pubkey(&e, &signer_key());

    let op_token_id = e.register(
        oplend::WASM,
        (
            admin.clone(),
            6u32,
            String::from_str(&e, "Alpha shares"),
            String::from_str(&e, "ALPHA"),
            MAX_SUPPLY,
            signer.clone(),
        ),
    );
    let op_token = oplend::Client::new(&e, &op_token_id);

    let wallet_id = e.register(OpLendWallet, (admin.clone(), signer));
    let wallet = OpLendWalletClient::new(&e, &wallet_id);

    wallet.register_op_lend(&OP_ID, &op_token_id);
    op_token.mint(&wallet_id, &FUNDED);

    Setup {
        e,
        admin,
        wallet,
        wallet_id,
        op_token,
        op_token_id,
    }
}

/// Independent reconstruction of the `OP_REDEEM` payload. Deliberately naive
/// appends rather than the contract's buffer layout, so an off-by-one in
/// `build_redeem_message` fails here instead of rejecting every backend
/// signature in production.
fn sign_redeem_with(
    e: &Env,
    key: &SigningKey,
    wallet_id: &Address,
    op_id: u32,
    destination: &Address,
    amount: i128,
    nonce: &String,
) -> BytesN<64> {
    let mut msg = Bytes::from_slice(e, b"OP_REDEEM");
    msg.append(&Bytes::from(wallet_id.to_string()));
    msg.append(&Bytes::from_slice(e, &op_id.to_be_bytes()));
    msg.append(&Bytes::from(destination.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));

    let v: std::vec::Vec<u8> = msg.iter().collect();
    BytesN::from_array(e, &key.sign(&v).to_bytes())
}

fn sign_redeem(
    s: &Setup,
    destination: &Address,
    amount: i128,
    nonce: &String,
) -> BytesN<64> {
    sign_redeem_with(
        &s.e,
        &signer_key(),
        &s.wallet_id,
        OP_ID,
        destination,
        amount,
        nonce,
    )
}

/// The op-lend token's own whitelist authorization: it signs
/// (token, user, nonce) and verifies it itself, so the wallet only forwards.
fn sign_whitelist(s: &Setup, user: &Address, nonce: &String) -> BytesN<64> {
    let mut msg = Bytes::from(s.op_token_id.to_string());
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(nonce.clone()));

    let v: std::vec::Vec<u8> = msg.iter().collect();
    BytesN::from_array(&s.e, &signer_key().sign(&v).to_bytes())
}

/// A destination the token already knows, so `redeem` alone can pay it.
fn whitelisted_user(s: &Setup) -> Address {
    let user = Address::generate(&s.e);
    s.op_token.whitelist_user_admin(&user, &true);
    user
}

fn nonce(e: &Env, s: &str) -> String {
    String::from_str(e, s)
}

#[test]
fn test_redeem_pays_the_destination_and_spends_the_nonce() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "redeem-1");
    let sig = sign_redeem(&s, &user, 250, &n);

    s.wallet.redeem(&OP_ID, &user, &250, &n, &sig);

    assert_eq!(s.op_token.balance(&user), 250);
    assert_eq!(s.op_token.balance(&s.wallet_id), FUNDED - 250);
    assert_eq!(s.wallet.op_lend_balance(&OP_ID), FUNDED - 250);
    assert!(s.wallet.nonce_used(&n));

    // Same signature, same nonce: the redeem is not replayable.
    assert_eq!(
        s.wallet.try_redeem(&OP_ID, &user, &250, &n, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}

#[test]
#[should_panic(expected = "Error(Crypto, InvalidInput)")]
fn test_redeem_rejects_a_signature_from_another_key() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "forged");
    let sig = sign_redeem_with(
        &s.e,
        &rotated_key(),
        &s.wallet_id,
        OP_ID,
        &user,
        250,
        &n,
    );

    s.wallet.redeem(&OP_ID, &user, &250, &n, &sig);
}

/// Every field is inside the signed bytes, so a relayer cannot re-point or
/// inflate an authorization it was handed.
#[test]
#[should_panic(expected = "Error(Crypto, InvalidInput)")]
fn test_redeem_amount_is_bound_to_the_signature() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "amount-swap");
    let sig = sign_redeem(&s, &user, 250, &n);

    s.wallet.redeem(&OP_ID, &user, &500, &n, &sig);
}

#[test]
#[should_panic(expected = "Error(Crypto, InvalidInput)")]
fn test_redeem_destination_is_bound_to_the_signature() {
    let s = setup();
    let user = whitelisted_user(&s);
    let attacker = whitelisted_user(&s);
    let n = nonce(&s.e, "dest-swap");
    let sig = sign_redeem(&s, &user, 250, &n);

    s.wallet.redeem(&OP_ID, &attacker, &250, &n, &sig);
}

/// The wallet address is in the payload, so an authorization minted for one
/// deployment cannot be replayed against another.
#[test]
#[should_panic(expected = "Error(Crypto, InvalidInput)")]
fn test_redeem_signature_does_not_cross_wallets() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "other-wallet");
    let other_wallet = Address::generate(&s.e);
    let sig = sign_redeem_with(
        &s.e,
        &signer_key(),
        &other_wallet,
        OP_ID,
        &user,
        250,
        &n,
    );

    s.wallet.redeem(&OP_ID, &user, &250, &n, &sig);
}

#[test]
fn test_redeem_for_an_unregistered_operation_fails() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "unregistered");
    let sig = sign_redeem(&s, &user, 250, &n);

    assert_eq!(
        s.wallet.try_redeem(&99, &user, &250, &n, &sig),
        Err(Ok(Error::OperationNotRegistered.into()))
    );
    // The failed call must not have burned the nonce.
    assert!(!s.wallet.nonce_used(&n));
}

#[test]
fn test_redeem_rejects_a_non_positive_amount() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "zero");
    let sig = sign_redeem(&s, &user, 0, &n);

    assert_eq!(
        s.wallet.try_redeem(&OP_ID, &user, &0, &n, &sig),
        Err(Ok(Error::InvalidAmount.into()))
    );
}

#[test]
fn test_redeem_to_the_wallet_itself_fails() {
    let s = setup();
    let n = nonce(&s.e, "self");
    let wallet_id = s.wallet_id.clone();
    let sig = sign_redeem(&s, &wallet_id, 250, &n);

    assert_eq!(
        s.wallet.try_redeem(&OP_ID, &wallet_id, &250, &n, &sig),
        Err(Ok(Error::SelfTargetNotAllowed.into()))
    );
}

/// op-lend refuses transfers to an address it has not whitelisted (`#7`,
/// `NotWhitelisted`), which is the entire reason `whitelist_and_redeem` exists.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_redeem_to_an_unknown_wallet_is_rejected_by_the_token() {
    let s = setup();
    let fresh = Address::generate(&s.e);
    let n = nonce(&s.e, "not-whitelisted");
    let sig = sign_redeem(&s, &fresh, 250, &n);

    s.wallet.redeem(&OP_ID, &fresh, &250, &n, &sig);
}

#[test]
fn test_whitelist_and_redeem_onboards_a_fresh_wallet() {
    let s = setup();
    let fresh = Address::generate(&s.e);
    assert!(!s.op_token.is_whitelisted(&fresh));

    let t_nonce = nonce(&s.e, "wl-1");
    let r_nonce = nonce(&s.e, "redeem-2");
    let t_sig = sign_whitelist(&s, &fresh, &t_nonce);
    let r_sig = sign_redeem(&s, &fresh, 400, &r_nonce);

    s.wallet.whitelist_and_redeem(
        &OP_ID, &fresh, &400, &t_nonce, &t_sig, &r_nonce, &r_sig,
    );

    assert!(s.op_token.is_whitelisted(&fresh));
    assert_eq!(s.op_token.balance(&fresh), 400);
    assert_eq!(s.wallet.op_lend_balance(&OP_ID), FUNDED - 400);
    assert!(s.wallet.nonce_used(&r_nonce));
}

/// The two nonces live in different contracts' storage, so the backend may
/// reuse one string for both legs of the same release.
#[test]
fn test_whitelist_and_redeem_nonces_are_namespaced_per_contract() {
    let s = setup();
    let fresh = Address::generate(&s.e);
    let n = nonce(&s.e, "shared-nonce");
    let t_sig = sign_whitelist(&s, &fresh, &n);
    let r_sig = sign_redeem(&s, &fresh, 100, &n);

    s.wallet
        .whitelist_and_redeem(&OP_ID, &fresh, &100, &n, &t_sig, &n, &r_sig);

    assert_eq!(s.op_token.balance(&fresh), 100);
    assert!(s.wallet.nonce_used(&n));
}

#[test]
fn test_admin_redeem_needs_no_signature() {
    let s = setup();
    let user = whitelisted_user(&s);

    s.wallet.admin_redeem(&OP_ID, &user, &600);

    assert_eq!(s.op_token.balance(&user), 600);
    assert_eq!(s.wallet.op_lend_balance(&OP_ID), FUNDED - 600);
}

/// `admin_redeem` moves custodied shares with no signature, so the admin auth
/// is the only thing standing between a relayer and the wallet's balance.
#[test]
#[should_panic(expected = "InvalidAction")]
fn test_admin_redeem_requires_admin_auth() {
    let s = setup();
    let user = whitelisted_user(&s);

    // Enforcing mode with no authorizations at all: `require_auth` now fails.
    s.e.set_auths(&[]);
    s.wallet.admin_redeem(&OP_ID, &user, &600);
}

#[test]
fn test_register_op_lend_rejects_an_account_address() {
    let s = setup();
    let account = Address::from_str(&s.e, ACCOUNT_STRKEY);

    assert_eq!(
        s.wallet.try_register_op_lend(&42, &account),
        Err(Ok(Error::NotAContract.into()))
    );
}

#[test]
fn test_register_op_lends_writes_every_entry() {
    let s = setup();
    let second = Address::generate(&s.e);
    let third = Address::generate(&s.e);

    let entries: Vec<OpLendEntry> = vec![
        &s.e,
        OpLendEntry {
            op_id: 1,
            op_lend: second.clone(),
        },
        OpLendEntry {
            op_id: 2,
            op_lend: third.clone(),
        },
    ];
    s.wallet.register_op_lends(&entries);

    assert_eq!(s.wallet.op_lend(&1), second);
    assert_eq!(s.wallet.op_lend(&2), third);
    // The batch leaves earlier registrations alone.
    assert_eq!(s.wallet.op_lend(&OP_ID), s.op_token_id);

    assert_eq!(
        s.wallet.try_register_op_lends(&Vec::new(&s.e)),
        Err(Ok(Error::EmptyBatch.into()))
    );
}

#[test]
fn test_register_op_lend_overwrites_a_stale_mapping() {
    let s = setup();
    let replacement = Address::generate(&s.e);

    s.wallet.register_op_lend(&OP_ID, &replacement);

    assert_eq!(s.wallet.op_lend(&OP_ID), replacement);
}

#[test]
fn test_op_lend_getters_fail_for_an_unregistered_operation() {
    let s = setup();

    assert_eq!(
        s.wallet.try_op_lend(&99),
        Err(Ok(Error::OperationNotRegistered.into()))
    );
    assert_eq!(
        s.wallet.try_op_lend_balance(&99),
        Err(Ok(Error::OperationNotRegistered.into()))
    );
}

#[test]
fn test_update_backend_signer_rotates_authorization() {
    let s = setup();
    let user = whitelisted_user(&s);

    s.wallet
        .update_backend_signer(&pubkey(&s.e, &rotated_key()));
    assert_eq!(s.wallet.backend_signer(), pubkey(&s.e, &rotated_key()));

    let n = nonce(&s.e, "rotated");
    let sig = sign_redeem_with(
        &s.e,
        &rotated_key(),
        &s.wallet_id,
        OP_ID,
        &user,
        250,
        &n,
    );
    s.wallet.redeem(&OP_ID, &user, &250, &n, &sig);

    assert_eq!(s.op_token.balance(&user), 250);
}

#[test]
fn test_set_admin_hands_over_the_admin_paths() {
    let s = setup();
    let new_admin = Address::generate(&s.e);

    s.wallet.set_admin(&new_admin);

    assert_eq!(s.wallet.admin(), new_admin);
    assert_ne!(s.wallet.admin(), s.admin);
}

/// `upgrade` really swaps the executable: after pointing the wallet at the
/// op-lend wasm, the instance stops answering wallet calls. Nothing else could
/// catch a no-op upgrade, since the contract id never changes.
#[test]
fn test_upgrade_replaces_the_executable() {
    let s = setup();
    assert_eq!(s.wallet.op_lend_balance(&OP_ID), FUNDED);

    let other_wasm = s.e.deployer().upload_contract_wasm(oplend::WASM);
    s.wallet.upgrade(&other_wasm);

    // State survived, but the code behind it is an op-lend token now, which
    // has no `op_lend_balance`.
    assert!(s.wallet.try_op_lend_balance(&OP_ID).is_err());
}
