use soroban_sdk::{panic_with_error, Address, Env};

use crate::errors::Error;
use crate::storage_types::DataKey;

pub fn read_administrator(e: &Env) -> Address {
    match e.storage().instance().get(&DataKey::Admin) {
        Some(admin) => admin,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

pub fn write_administrator(e: &Env, id: &Address) {
    e.storage().instance().set(&DataKey::Admin, id);
}
