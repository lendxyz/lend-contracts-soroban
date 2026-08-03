use crate::errors::Error;
use crate::storage_types::{AllowanceDataKey, AllowanceValue, DataKey};
use soroban_sdk::{panic_with_error, Address, Env};

fn key(from: Address, spender: Address) -> DataKey {
    DataKey::Allowance(AllowanceDataKey { from, spender })
}

fn read(e: &Env, key: &DataKey) -> AllowanceValue {
    match e.storage().temporary().get::<_, AllowanceValue>(key) {
        Some(allowance)
            if allowance.expiration_ledger >= e.ledger().sequence() =>
        {
            allowance
        }
        Some(allowance) => AllowanceValue {
            amount: 0,
            expiration_ledger: allowance.expiration_ledger,
        },
        None => AllowanceValue {
            amount: 0,
            expiration_ledger: 0,
        },
    }
}

fn write(e: &Env, key: &DataKey, amount: i128, expiration_ledger: u32) {
    let sequence = e.ledger().sequence();
    if amount > 0 && expiration_ledger < sequence {
        panic_with_error!(e, Error::BadAllowanceExpiration);
    }

    e.storage().temporary().set(
        key,
        &AllowanceValue {
            amount,
            expiration_ledger,
        },
    );

    if amount > 0 {
        let live_for = expiration_ledger - sequence;
        e.storage().temporary().extend_ttl(key, live_for, live_for);
    }
}

pub fn read_allowance(e: &Env, from: Address, spender: Address) -> i128 {
    read(e, &key(from, spender)).amount
}

pub fn write_allowance(
    e: &Env,
    from: Address,
    spender: Address,
    amount: i128,
    expiration_ledger: u32,
) {
    write(e, &key(from, spender), amount, expiration_ledger);
}

pub fn spend_allowance(e: &Env, from: Address, spender: Address, amount: i128) {
    let key = key(from, spender);
    let allowance = read(e, &key);

    if allowance.amount < amount {
        panic_with_error!(e, Error::InsufficientAllowance);
    }

    if amount > 0 {
        write(
            e,
            &key,
            allowance.amount - amount,
            allowance.expiration_ledger,
        );
    }
}
