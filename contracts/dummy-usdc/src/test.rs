#![cfg(test)]
extern crate std;

use crate::contract::{DummyUSDC, DummyUSDCClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn create_token<'a>(e: &Env, admin: &Address) -> DummyUSDCClient<'a> {
    let id = e.register(
        DummyUSDC,
        (
            admin.clone(),
            6u32,
            String::from_str(e, "Dummy USD Coin"),
            String::from_str(e, "dUSDC"),
        ),
    );
    DummyUSDCClient::new(e, &id)
}

#[test]
fn test_metadata() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let token = create_token(&e, &admin);

    assert_eq!(token.decimals(), 6);
    assert_eq!(token.name(), String::from_str(&e, "Dummy USD Coin"));
    assert_eq!(token.symbol(), String::from_str(&e, "dUSDC"));
    assert_eq!(token.admin(), admin);
}

#[test]
fn test_mint_by_admin() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user, &1_000_000);
    assert_eq!(token.balance(&user), 1_000_000);
}

#[test]
fn test_faucet_open_to_anyone() {
    let e = Env::default();
    // No auth mocking: faucet must not require any signature.
    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.faucet(&user, &500_000);
    assert_eq!(token.balance(&user), 500_000);
}

#[test]
fn test_transfer_no_restrictions() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&alice, &1_000_000);
    token.transfer(&alice, &bob, &400_000);

    assert_eq!(token.balance(&alice), 600_000);
    assert_eq!(token.balance(&bob), 400_000);
}

#[test]
fn test_approve_and_transfer_from() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let alice = Address::generate(&e);
    let spender = Address::generate(&e);
    let bob = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&alice, &1_000_000);

    let expiration = e.ledger().sequence() + 1000;
    token.approve(&alice, &spender, &300_000, &expiration);
    assert_eq!(token.allowance(&alice, &spender), 300_000);

    token.transfer_from(&spender, &alice, &bob, &200_000);
    assert_eq!(token.balance(&alice), 800_000);
    assert_eq!(token.balance(&bob), 200_000);
    assert_eq!(token.allowance(&alice, &spender), 100_000);
}

#[test]
fn test_burn() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let alice = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&alice, &1_000_000);
    token.burn(&alice, &250_000);
    assert_eq!(token.balance(&alice), 750_000);
}

#[test]
fn test_burn_from() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let alice = Address::generate(&e);
    let spender = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&alice, &1_000_000);

    let expiration = e.ledger().sequence() + 1000;
    token.approve(&alice, &spender, &300_000, &expiration);
    token.burn_from(&spender, &alice, &100_000);

    assert_eq!(token.balance(&alice), 900_000);
    assert_eq!(token.allowance(&alice, &spender), 200_000);
}

#[test]
fn test_set_admin() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let new_admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.set_admin(&new_admin);
    assert_eq!(token.admin(), new_admin);

    // New admin can mint.
    token.mint(&user, &123);
    assert_eq!(token.balance(&user), 123);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_transfer_insufficient_balance() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let alice = Address::generate(&e);
    let bob = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&alice, &100);
    token.transfer(&alice, &bob, &101);
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn test_negative_mint_panics() {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let token = create_token(&e, &admin);

    token.mint(&user, &-1);
}
