use crate::storage_types::DataKey;
use soroban_sdk::{Address, Env};

pub fn is_whitelisted(e: &Env, addr: &Address) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::Whitelisted(addr.clone()))
        .unwrap_or(false)
}

pub fn set_whitelisted(e: &Env, addr: &Address, state: bool) {
    e.storage()
        .persistent()
        .set(&DataKey::Whitelisted(addr.clone()), &state);
}

pub fn require_whitelisted(e: &Env, addr: &Address) {
    if !is_whitelisted(e, addr) {
        panic!("address is not whitelisted");
    }
}
