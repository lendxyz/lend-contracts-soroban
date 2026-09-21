use soroban_sdk::{panic_with_error, token, Address, BytesN, Env};

use crate::errors::Error;
use crate::types::DataKey;

const DAY_IN_LEDGERS: u32 = 17_280;

const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const REGISTRY_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const REGISTRY_THRESHOLD: u32 = REGISTRY_BUMP - 7 * DAY_IN_LEDGERS;

pub fn bump_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
}

// --- instance config ---

pub fn read_admin(e: &Env) -> Address {
    match e.storage().instance().get(&DataKey::Admin) {
        Some(admin) => admin,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

pub fn write_admin(e: &Env, admin: &Address) {
    e.storage().instance().set(&DataKey::Admin, admin);
}

pub fn require_admin(e: &Env) -> Address {
    let admin = read_admin(e);
    admin.require_auth();
    admin
}

pub fn read_backend_signer(e: &Env) -> BytesN<32> {
    match e.storage().instance().get(&DataKey::BackendSigner) {
        Some(signer) => signer,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

pub fn write_backend_signer(e: &Env, signer: &BytesN<32>) {
    e.storage().instance().set(&DataKey::BackendSigner, signer);
}

// --- registry ---

pub fn read_op_lend(e: &Env, op_id: u32) -> Address {
    match e.storage().persistent().get(&DataKey::OpLend(op_id)) {
        Some(op_lend) => op_lend,
        None => panic_with_error!(e, Error::OperationNotRegistered),
    }
}

pub fn load_op_lend(e: &Env, op_id: u32) -> Address {
    let op_lend = read_op_lend(e, op_id);
    e.storage().persistent().extend_ttl(
        &DataKey::OpLend(op_id),
        REGISTRY_THRESHOLD,
        REGISTRY_BUMP,
    );
    op_lend
}

pub fn write_op_lend(e: &Env, op_id: u32, op_lend: &Address) {
    let key = DataKey::OpLend(op_id);
    e.storage().persistent().set(&key, op_lend);
    e.storage().persistent().extend_ttl(
        &key,
        REGISTRY_THRESHOLD,
        REGISTRY_BUMP,
    );
}

pub fn op_lend_client<'a>(e: &Env, op_lend: &Address) -> token::Client<'a> {
    token::Client::new(e, op_lend)
}
