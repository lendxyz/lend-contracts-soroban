//! DummyUSDC — a testnet stand-in for Circle USDC.
//!
//! Implements the standard SEP-41 token interface with no transfer
//! restrictions. New supply comes from two entry points:
//!   - `mint`   — admin-only, mints to any address.
//!   - `faucet` — open to anyone, so devs can self-serve test tokens.

use soroban_sdk::{
    contract, contractimpl, token::TokenInterface, Address, Env, MuxedAddress, String,
};

use crate::admin::{has_administrator, read_administrator, write_administrator};
use crate::allowance::{read_allowance, spend_allowance, write_allowance};
use crate::balance::{read_balance, receive_balance, spend_balance};
use crate::events;
use crate::metadata::{read_decimal, read_name, read_symbol, write_metadata};
use crate::storage_types::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};

fn check_nonnegative_amount(amount: i128) {
    if amount < 0 {
        panic!("negative amount is not allowed: {}", amount)
    }
}

fn bump_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

#[contract]
pub struct DummyUSDC;

#[contractimpl]
impl DummyUSDC {
    pub fn __constructor(e: Env, admin: Address, decimal: u32, name: String, symbol: String) {
        if has_administrator(&e) {
            panic!("already initialized")
        }
        write_administrator(&e, &admin);
        write_metadata(&e, decimal, name, symbol);
    }

    /// Admin-only: mint `amount` to `to`.
    pub fn mint(e: Env, to: Address, amount: i128) {
        check_nonnegative_amount(amount);
        let admin = read_administrator(&e);
        admin.require_auth();
        bump_instance(&e);

        receive_balance(&e, to.clone(), amount);
        events::mint(&e, admin, to, amount);
    }

    /// Open faucet: anyone can mint `amount` to `to`. Testnet convenience only.
    pub fn faucet(e: Env, to: Address, amount: i128) {
        check_nonnegative_amount(amount);
        bump_instance(&e);

        receive_balance(&e, to.clone(), amount);
        // Reuse the standard `mint` event; `to` doubles as the minter here.
        events::mint(&e, to.clone(), to, amount);
    }

    /// Admin-only: hand the admin role to `new_admin`.
    pub fn set_admin(e: Env, new_admin: Address) {
        let admin = read_administrator(&e);
        admin.require_auth();
        bump_instance(&e);

        write_administrator(&e, &new_admin);
        events::set_admin(&e, admin, new_admin);
    }

    pub fn admin(e: Env) -> Address {
        read_administrator(&e)
    }
}

#[contractimpl]
impl TokenInterface for DummyUSDC {
    fn allowance(e: Env, from: Address, spender: Address) -> i128 {
        bump_instance(&e);
        read_allowance(&e, from, spender).amount
    }

    fn approve(e: Env, from: Address, spender: Address, amount: i128, expiration_ledger: u32) {
        from.require_auth();
        check_nonnegative_amount(amount);
        bump_instance(&e);

        write_allowance(
            &e,
            from.clone(),
            spender.clone(),
            amount,
            expiration_ledger,
        );
        events::approve(&e, from, spender, amount, expiration_ledger);
    }

    fn balance(e: Env, id: Address) -> i128 {
        bump_instance(&e);
        read_balance(&e, id)
    }

    fn transfer(e: Env, from: Address, to: MuxedAddress, amount: i128) {
        from.require_auth();
        check_nonnegative_amount(amount);
        bump_instance(&e);

        let to = to.address();
        spend_balance(&e, from.clone(), amount);
        receive_balance(&e, to.clone(), amount);
        events::transfer(&e, from, to, amount);
    }

    fn transfer_from(e: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        check_nonnegative_amount(amount);
        bump_instance(&e);

        spend_allowance(&e, from.clone(), spender, amount);
        spend_balance(&e, from.clone(), amount);
        receive_balance(&e, to.clone(), amount);
        events::transfer(&e, from, to, amount);
    }

    fn burn(e: Env, from: Address, amount: i128) {
        from.require_auth();
        check_nonnegative_amount(amount);
        bump_instance(&e);

        spend_balance(&e, from.clone(), amount);
        events::burn(&e, from, amount);
    }

    fn burn_from(e: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        check_nonnegative_amount(amount);
        bump_instance(&e);

        spend_allowance(&e, from.clone(), spender, amount);
        spend_balance(&e, from.clone(), amount);
        events::burn(&e, from, amount);
    }

    fn decimals(e: Env) -> u32 {
        read_decimal(&e)
    }

    fn name(e: Env) -> String {
        read_name(&e)
    }

    fn symbol(e: Env) -> String {
        read_symbol(&e)
    }
}
