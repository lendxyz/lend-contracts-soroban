#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Deployer as _},
    vec, Address, Bytes, BytesN, Env, String, Vec,
};

use crate::contract::{OpLendWallet, OpLendWalletClient};
use crate::errors::Error;
use crate::types::{DataKey, Operation};

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
/// so this is the only way to try an account where a factory is expected.
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

/// Stands in for the factory: serves the backend signer the wallet and the
/// token read (rotatable), and the operations the wallet registers.
#[contract]
pub struct MockFactory;

#[contractimpl]
impl MockFactory {
    pub fn __constructor(e: Env, signer: BytesN<32>) {
        e.storage()
            .instance()
            .set(&symbol_short!("signer"), &signer);
    }
    pub fn set_signer(e: Env, signer: BytesN<32>) {
        e.storage()
            .instance()
            .set(&symbol_short!("signer"), &signer);
    }
    pub fn backend_signer(e: Env) -> BytesN<32> {
        e.storage()
            .instance()
            .get(&symbol_short!("signer"))
            .unwrap()
    }
    pub fn set_operation(e: Env, id: u32, op_token: Address) {
        e.storage().instance().set(&id, &op_token);
    }
    /// Fails for an operation it does not know, like the real factory.
    pub fn get_operation(e: Env, id: u32) -> Operation {
        Operation {
            op_token: e.storage().instance().get(&id).unwrap(),
            total_shares: 0,
            eur_per_share: 0,
            op_name: String::from_str(&e, ""),
        }
    }
}

struct Setup<'a> {
    e: Env,
    admin: Address,
    factory: MockFactoryClient<'a>,
    wallet: OpLendWalletClient<'a>,
    wallet_id: Address,
    op_token: oplend::Client<'a>,
    op_token_id: Address,
}

/// A wallet holding `FUNDED` shares of operation `OP_ID`. The mint whitelists
/// the wallet on the token, which is what lets it transfer out at all. Both
/// read the backend signer from the same (mock) factory.
fn setup<'a>() -> Setup<'a> {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let factory_id = e.register(MockFactory, (pubkey(&e, &signer_key()),));

    let op_token_id = e.register(
        oplend::WASM,
        (
            admin.clone(),
            String::from_str(&e, "Alpha shares"),
            String::from_str(&e, "ALPHA"),
            MAX_SUPPLY,
            factory_id.clone(),
        ),
    );
    let op_token = oplend::Client::new(&e, &op_token_id);

    let wallet_id =
        e.register(OpLendWallet, (admin.clone(), factory_id.clone()));
    let wallet = OpLendWalletClient::new(&e, &wallet_id);
    let factory = MockFactoryClient::new(&e, &factory_id);

    factory.set_operation(&OP_ID, &op_token_id);
    wallet.register_op_lend(&OP_ID);
    op_token.mint(&wallet_id, &FUNDED);

    Setup {
        factory,
        e,
        admin,
        wallet,
        wallet_id,
        op_token,
        op_token_id,
    }
}

/// Puts the wallet in the state the deployed one is in right after its
/// in-place upgrade: its old constructor stored a signer, not a factory.
fn drop_factory(s: &Setup) {
    s.e.as_contract(&s.wallet_id, || {
        s.e.storage().instance().remove(&DataKey::Factory)
    });
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

/// Independent reconstruction of the `VOID_NONCE` payload.
fn sign_void(
    e: &Env,
    contract: &Address,
    nonce: &String,
) -> Option<BytesN<64>> {
    let mut msg = Bytes::from_slice(e, b"VOID_NONCE");
    msg.append(&Bytes::from(contract.to_string()));
    msg.append(&Bytes::from(nonce.clone()));

    let v: std::vec::Vec<u8> = msg.iter().collect();
    Some(BytesN::from_array(e, &signer_key().sign(&v).to_bytes()))
}

#[test]
fn test_void_nonce_blocks_the_redeem_signature() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "voided");
    let sig = sign_redeem(&s, &user, 250, &n);
    let void_sig = sign_void(&s.e, &s.wallet_id, &n);

    s.wallet.void_nonce(&n, &void_sig);
    assert!(s.wallet.nonce_used(&n));

    assert_eq!(
        s.wallet.try_redeem(&OP_ID, &user, &250, &n, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
    assert_eq!(
        s.wallet.try_void_nonce(&n, &void_sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
    assert_eq!(s.op_token.balance(&s.wallet_id), FUNDED);
}

/// Neither the redeem signature nor a void signed for another wallet may burn
/// the nonce, and a rejected void leaves it spendable.
#[test]
fn test_void_nonce_rejects_foreign_signatures() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "not-voided");
    let sig = sign_redeem(&s, &user, 250, &n);
    let other_wallet = Address::generate(&s.e);

    assert!(s.wallet.try_void_nonce(&n, &Some(sig.clone())).is_err());
    assert!(s
        .wallet
        .try_void_nonce(&n, &sign_void(&s.e, &other_wallet, &n))
        .is_err());
    assert!(!s.wallet.nonce_used(&n));

    s.wallet.redeem(&OP_ID, &user, &250, &n, &sig);
    assert_eq!(s.op_token.balance(&user), 250);
}

#[test]
fn test_admin_void_nonce_needs_no_signature() {
    let s = setup();
    let user = whitelisted_user(&s);
    let n = nonce(&s.e, "admin-voided");
    let sig = sign_redeem(&s, &user, 250, &n);

    s.wallet.void_nonce(&n, &None);
    assert!(s.wallet.nonce_used(&n));

    assert_eq!(
        s.wallet.try_redeem(&OP_ID, &user, &250, &n, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );

    // Without a signature, only the admin's auth can void.
    s.e.set_auths(&[]);
    let other = nonce(&s.e, "not-admin");
    assert!(s.wallet.try_void_nonce(&other, &None).is_err());
    assert!(!s.wallet.nonce_used(&other));
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

/// op-lend refuses transfers to an address it has not whitelisted (`#6`,
/// `NotWhitelisted`), which is the entire reason `whitelist_and_redeem` exists.
#[test]
#[should_panic(expected = "Error(Contract, #6)")]
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

/// AUDIT OpLendWallet C4: the registry takes only an operation id and copies
/// the token from the factory, so an operation can never be mapped to another
/// operation's token. An id the factory does not know is refused.
#[test]
fn test_register_op_lend_takes_the_token_from_the_factory() {
    let s = setup();
    let op_3_token = Address::generate(&s.e);
    s.factory.set_operation(&3, &op_3_token);

    s.wallet.register_op_lend(&3);
    assert_eq!(s.wallet.op_lend(&3), op_3_token);

    assert!(s.wallet.try_register_op_lend(&99).is_err());
    assert_eq!(
        s.wallet.try_op_lend(&99),
        Err(Ok(Error::OperationNotRegistered.into()))
    );
}

#[test]
fn test_register_op_lends_writes_every_entry() {
    let s = setup();
    let second = Address::generate(&s.e);
    let third = Address::generate(&s.e);
    s.factory.set_operation(&1, &second);
    s.factory.set_operation(&2, &third);

    s.wallet.register_op_lends(&vec![&s.e, 1, 2]);

    assert_eq!(s.wallet.op_lend(&1), second);
    assert_eq!(s.wallet.op_lend(&2), third);
    // The batch leaves earlier registrations alone.
    assert_eq!(s.wallet.op_lend(&OP_ID), s.op_token_id);

    assert_eq!(
        s.wallet.try_register_op_lends(&Vec::new(&s.e)),
        Err(Ok(Error::EmptyBatch.into()))
    );
    // One unknown id fails the whole batch.
    assert!(s.wallet.try_register_op_lends(&vec![&s.e, 1, 99]).is_err());
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

/// AUDIT OpLendWallet C3: the backend key is read from the factory on every
/// check, so rotating it there rotates it for the wallet (and the token) at
/// once; the old key stops working.
#[test]
fn test_backend_signer_follows_the_factory() {
    let s = setup();
    let user = whitelisted_user(&s);

    s.factory.set_signer(&pubkey(&s.e, &rotated_key()));
    assert_eq!(s.wallet.backend_signer(), pubkey(&s.e, &rotated_key()));

    let n = nonce(&s.e, "rotated");
    let sign = |key: &SigningKey| {
        sign_redeem_with(&s.e, key, &s.wallet_id, OP_ID, &user, 250, &n)
    };
    assert!(s
        .wallet
        .try_redeem(&OP_ID, &user, &250, &n, &sign(&signer_key()))
        .is_err());
    s.wallet
        .redeem(&OP_ID, &user, &250, &n, &sign(&rotated_key()));

    assert_eq!(s.op_token.balance(&user), 250);
}

/// `set_factory` migrates a wallet upgraded from the stored-signer version:
/// it has no factory, so signer reads fail until the admin records one.
/// Admin-only; accounts and contracts serving no backend signer are refused.
#[test]
fn test_set_factory_points_an_upgraded_wallet_at_the_factory() {
    let s = setup();
    let user = whitelisted_user(&s);
    drop_factory(&s);
    assert_eq!(
        s.wallet.try_backend_signer(),
        Err(Ok(Error::NotInitialized.into()))
    );

    let new_factory_id =
        s.e.register(MockFactory, (pubkey(&s.e, &rotated_key()),));
    let new_factory = MockFactoryClient::new(&s.e, &new_factory_id);
    new_factory.set_operation(&OP_ID, &s.op_token_id);

    assert!(s
        .wallet
        .try_set_factory(&Address::from_str(&s.e, ACCOUNT_STRKEY))
        .is_err());
    assert!(s.wallet.try_set_factory(&s.op_token_id).is_err());
    s.e.set_auths(&[]);
    assert!(s.wallet.try_set_factory(&new_factory_id).is_err());
    s.e.mock_all_auths();

    s.wallet.set_factory(&new_factory_id);
    assert_eq!(s.wallet.factory(), new_factory_id);
    assert_eq!(s.wallet.backend_signer(), pubkey(&s.e, &rotated_key()));

    // Registration and redeems now go through the recorded factory.
    s.wallet.register_op_lend(&OP_ID);
    assert_eq!(s.wallet.op_lend(&OP_ID), s.op_token_id);
    let n = nonce(&s.e, "new-factory");
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

/// Once a factory is recorded, by the constructor or by the migration call,
/// `set_factory` is closed for good.
#[test]
fn test_set_factory_is_refused_once_a_factory_is_set() {
    let s = setup();
    let other = s.e.register(MockFactory, (pubkey(&s.e, &rotated_key()),));

    assert_eq!(
        s.wallet.try_set_factory(&other),
        Err(Ok(Error::FactoryAlreadySet.into()))
    );
    assert_eq!(s.wallet.factory(), s.factory.address);

    drop_factory(&s);
    s.wallet.set_factory(&other);
    assert_eq!(
        s.wallet.try_set_factory(&s.factory.address),
        Err(Ok(Error::FactoryAlreadySet.into()))
    );
    assert_eq!(s.wallet.factory(), other);
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

/// AUDIT OpLendWallet C6: the TTL extension in `upgrade` runs after the swap.
/// Instance TTL extension also covers the code entry, so the *new* wasm must
/// come out as long-lived as the instance, not keep its short upload TTL.
#[test]
fn test_upgrade_extends_the_new_code_ttl() {
    let s = setup();
    let other_wasm = s.e.deployer().upload_contract_wasm(oplend::WASM);
    s.wallet.upgrade(&other_wasm);

    let d = s.e.deployer();
    assert_eq!(
        d.get_contract_code_ttl(&s.wallet_id),
        d.get_contract_instance_ttl(&s.wallet_id)
    );
}
