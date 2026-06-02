#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::Address as _, Address, Bytes, BytesN, Env, FromVal, String,
};

use crate::contract::{OpLendToken, OpLendTokenClient};

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
    let mut msg = Bytes::from_slice(e, b"WHITELIST");
    msg.append(&Bytes::from(token.address.to_string()));
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
#[should_panic(expected = "Total supply cap exceeded")]
fn test_supply_cap_enforced() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &(MAX_SUPPLY + 1));
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
#[should_panic(expected = "address is not whitelisted")]
fn test_transfer_to_non_whitelisted_fails() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e); // never whitelisted
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.transfer(&user1, &user2, &600);
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

#[test]
fn test_admin_burn() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    assert_eq!(token.total_supply(), 1000);

    token.admin_burn(&user1, &400);
    assert_eq!(token.balance(&user1), 600);
    assert_eq!(token.total_supply(), 600);
}

#[test]
fn test_burn_decrements_supply() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.burn(&user1, &300);
    assert_eq!(token.balance(&user1), 700);
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
#[should_panic(expected = "nonce already used")]
fn test_whitelist_user_nonce_replay() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    let nonce = String::from_val(&e, &"nonce-1");
    let sig = sign_whitelist(&e, &token, &user, &nonce);

    token.whitelist_user(&user, &nonce, &sig);
    token.whitelist_user(&user, &nonce, &sig);
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

#[test]
#[should_panic(expected = "insufficient balance")]
fn transfer_insufficient_balance() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user1, &1000);
    token.whitelist_user_admin(&user2, &true);
    token.transfer(&user1, &user2, &1001);
}

#[test]
#[should_panic(expected = "insufficient allowance")]
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
    token.transfer_from(&spender, &user1, &user2, &101);
}

#[test]
#[should_panic(expected = "Decimal must not be greater than 6")]
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
