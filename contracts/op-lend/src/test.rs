#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Bytes, BytesN, Env, FromVal, IntoVal, String,
};

use crate::contract::{OpLendToken, OpLendTokenClient};
use crate::errors::Error;

const MAX_SUPPLY: i128 = 1_000_000;

fn signer_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn signer_pubkey(e: &Env) -> BytesN<32> {
    BytesN::from_array(e, &signer_key().verifying_key().to_bytes())
}

fn create_token<'a>(e: &Env, admin: &Address) -> OpLendTokenClient<'a> {
    let token_contract = e.register(
        OpLendToken,
        (
            admin,
            6_u32,
            String::from_val(e, &"name"),
            String::from_val(e, &"symbol"),
            MAX_SUPPLY,
            signer_pubkey(e),
        ),
    );
    OpLendTokenClient::new(e, &token_contract)
}

/// Signs the canonical whitelist message the contract expects.
fn sign_whitelist(
    e: &Env,
    token: &OpLendTokenClient,
    user: &Address,
    nonce: &String,
) -> BytesN<64> {
    // Reconstruct the exact message bytes the contract builds.
    let mut msg = Bytes::from(token.address.to_string());
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(nonce.clone()));

    let msg_vec: std::vec::Vec<u8> = msg.iter().collect();
    let sig = signer_key().sign(&msg_vec);
    BytesN::from_array(e, &sig.to_bytes())
}

#[test]
fn test_mint_whitelists_and_tracks_supply() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    assert_eq!(token.balance(&user1), 1000);
    assert_eq!(token.total_supply(), 1000);
    assert!(token.is_whitelisted(&user1));
    assert_eq!(token.max_supply(), MAX_SUPPLY);
}

#[test]
fn test_supply_cap_enforced() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin);

    assert_eq!(
        token.try_mint(&user1, &(MAX_SUPPLY + 1)),
        Err(Ok(Error::SupplyCapExceeded.into()))
    );
}

#[test]
fn test_transfer_requires_whitelist() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000); // user1 auto-whitelisted
    token.whitelist_user_admin(&user2, &true);

    token.transfer(&user1, &user2, &600);
    assert_eq!(token.balance(&user1), 400);
    assert_eq!(token.balance(&user2), 600);
}

#[test]
fn test_transfer_to_non_whitelisted_fails() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e); // never whitelisted
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    assert_eq!(
        token.try_transfer(&user1, &user2, &600),
        Err(Ok(Error::NotWhitelisted.into()))
    );
}

#[test]
fn test_transfer_from_requires_whitelist() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let spender = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.whitelist_user_admin(&user2, &true);
    token.approve(&user1, &spender, &500, &200);

    token.transfer_from(&spender, &user1, &user2, &400);
    assert_eq!(token.balance(&user1), 600);
    assert_eq!(token.balance(&user2), 400);
}

/// AUDIT OpLend C1: the admin burns from any holder; the holder's own auth
/// is not enough.
#[test]
fn test_burn_is_admin_only() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);

    e.mock_auths(&[MockAuth {
        address: &user1,
        invoke: &MockAuthInvoke {
            contract: &token.address,
            fn_name: "burn",
            args: (&user1, 300_i128).into_val(&e),
            sub_invokes: &[],
        },
    }]);
    assert!(token.try_burn(&user1, &300).is_err());
    assert_eq!(token.balance(&user1), 1000);

    e.mock_all_auths();
    token.burn(&user1, &400);
    assert_eq!(e.auths()[0].0, admin);
    assert_eq!(token.balance(&user1), 600);
    assert_eq!(token.total_supply(), 600);
}

/// AUDIT OpLend C1: an allowance lets only the admin burn.
#[test]
fn test_burn_from_is_admin_only() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let spender = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.approve(&user1, &spender, &1000, &1000);
    token.approve(&user1, &admin, &1000, &1000);

    assert_eq!(
        token.try_burn_from(&spender, &user1, &300),
        Err(Ok(Error::NotAdmin.into()))
    );

    token.burn_from(&admin, &user1, &300);
    assert_eq!(token.balance(&user1), 700);
    assert_eq!(token.allowance(&user1, &admin), 700);
    assert_eq!(token.allowance(&user1, &spender), 1000);
    assert_eq!(token.total_supply(), 700);
}

#[test]
fn test_whitelist_user_with_signature() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    let sig = sign_whitelist(&e, &token, &user, &nonce);

    assert!(!token.is_whitelisted(&user));
    token.whitelist_user(&user, &nonce, &sig);
    assert!(token.is_whitelisted(&user));
}

#[test]
fn test_whitelist_user_nonce_replay() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    let sig = sign_whitelist(&e, &token, &user, &nonce);

    token.whitelist_user(&user, &nonce, &sig);
    assert_eq!(
        token.try_whitelist_user(&user, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}

#[test]
#[should_panic]
fn test_whitelist_user_bad_signature() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    // Signature over a different user => verification fails.
    let other = Address::generate(&e);
    let sig = sign_whitelist(&e, &token, &other, &nonce);

    token.whitelist_user(&user, &nonce, &sig);
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

    let msg_vec: std::vec::Vec<u8> = msg.iter().collect();
    Some(BytesN::from_array(
        e,
        &signer_key().sign(&msg_vec).to_bytes(),
    ))
}

#[test]
fn test_void_nonce_blocks_the_whitelist_signature() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    let sig = sign_whitelist(&e, &token, &user, &nonce);
    let void_sig = sign_void(&e, &token.address, &nonce);

    token.void_nonce(&nonce, &void_sig);

    assert_eq!(
        token.try_whitelist_user(&user, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
    assert_eq!(
        token.try_void_nonce(&nonce, &void_sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
    assert!(!token.is_whitelisted(&user));
}

/// Neither the whitelist signature nor a void signed for another contract may
/// burn the nonce, and a rejected void leaves it spendable.
#[test]
fn test_void_nonce_rejects_foreign_signatures() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let other = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    let sig = sign_whitelist(&e, &token, &user, &nonce);

    assert!(token.try_void_nonce(&nonce, &Some(sig.clone())).is_err());
    assert!(token
        .try_void_nonce(&nonce, &sign_void(&e, &other, &nonce))
        .is_err());

    token.whitelist_user(&user, &nonce, &sig);
    assert!(token.is_whitelisted(&user));
}

#[test]
fn test_admin_void_nonce_needs_no_signature() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    let sig = sign_whitelist(&e, &token, &user, &nonce);

    token.void_nonce(&nonce, &None);

    assert_eq!(
        token.try_whitelist_user(&user, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );

    // Without a signature, only the admin's auth can void.
    e.set_auths(&[]);
    let other = String::from_val(&e, &"nonce-2");
    assert!(token.try_void_nonce(&other, &None).is_err());
}

#[test]
fn test_approve_and_allowance() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.approve(&user1, &user2, &500, &200);
    assert_eq!(token.allowance(&user1, &user2), 500);
    token.approve(&user1, &user2, &0, &200);
    assert_eq!(token.allowance(&user1, &user2), 0);
}

#[test]
fn test_set_admin() {
    let e = Env::default();
    e.mock_all_auths();

    let admin1 = Address::generate(&e);
    let admin2 = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin1);

    token.set_admin(&admin2);
    // New admin can mint.
    token.mint(&user1, &10);
    assert_eq!(token.balance(&user1), 10);
}

/// AUDIT OpLend C3: only the admin (the factory in production) can swap the
/// token's code. The hash is a real upload, so the only failure left is auth.
#[test]
fn test_upgrade_is_admin_only() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let token = create_token(&e, &admin);
    let hash = e.deployer().upload_contract_wasm(OWN_WASM);

    assert!(token.try_upgrade(&hash).is_err());

    e.mock_all_auths();
    token.upgrade(&hash);
    assert_eq!(e.auths()[0].0, admin);
}

/// This crate's own built wasm, uploaded as a valid upgrade target.
const OWN_WASM: &[u8] = include_bytes!(
    "../../../target/wasm32v1-none/release/lend_operation_token.wasm"
);

#[test]
fn transfer_insufficient_balance() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.whitelist_user_admin(&user2, &true);
    assert_eq!(
        token.try_transfer(&user1, &user2, &1001),
        Err(Ok(Error::InsufficientBalance.into()))
    );
}

#[test]
fn transfer_from_insufficient_allowance() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let spender = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.whitelist_user_admin(&user2, &true);
    token.approve(&user1, &spender, &100, &200);
    assert_eq!(
        token.try_transfer_from(&spender, &user1, &user2, &101),
        Err(Ok(Error::InsufficientAllowance.into()))
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn decimal_over_six() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let _ = OpLendTokenClient::new(
        &e,
        &e.register(
            OpLendToken,
            (
                admin,
                7_u32,
                String::from_val(&e, &"name"),
                String::from_val(&e, &"symbol"),
                MAX_SUPPLY,
                signer_pubkey(&e),
            ),
        ),
    );
}
