use soroban_sdk::{panic_with_error, Address, Env};

use crate::errors::Error;
use crate::storage_types::{
    Account, DataKey, ACCOUNT_BUMP_AMOUNT, ACCOUNT_LIFETIME_THRESHOLD,
};

#[derive(Copy, Clone, PartialEq)]
pub enum Wl {
    Skip,
    Require,
    Grant,
}

fn read(e: &Env, key: &DataKey) -> Account {
    e.storage().persistent().get(key).unwrap_or(Account::ZERO)
}

fn write(e: &Env, key: &DataKey, account: &Account) {
    e.storage().persistent().set(key, account);
    e.storage().persistent().extend_ttl(
        key,
        ACCOUNT_LIFETIME_THRESHOLD,
        ACCOUNT_BUMP_AMOUNT,
    );
}

pub fn adjust(e: &Env, addr: &Address, delta: i128, wl: Wl) {
    let key = DataKey::Account(addr.clone());
    let mut account = read(e, &key);

    match wl {
        Wl::Require => {
            if !account.whitelisted {
                panic_with_error!(e, Error::NotWhitelisted);
            }
        }
        Wl::Grant => account.whitelisted = true,
        Wl::Skip => {}
    }

    let balance = account.balance + delta;
    if balance < 0 {
        panic_with_error!(e, Error::InsufficientBalance);
    }

    account.balance = balance;
    write(e, &key, &account);
}

pub fn balance(e: &Env, addr: &Address) -> i128 {
    read(e, &DataKey::Account(addr.clone())).balance
}

pub fn is_whitelisted(e: &Env, addr: &Address) -> bool {
    read(e, &DataKey::Account(addr.clone())).whitelisted
}

pub fn set_whitelisted(e: &Env, addr: &Address, state: bool) {
    let key = DataKey::Account(addr.clone());
    let mut account = read(e, &key);
    if account.whitelisted == state {
        return;
    }
    account.whitelisted = state;
    write(e, &key, &account);
}
