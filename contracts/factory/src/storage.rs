use soroban_sdk::{panic_with_error, token, Address, BytesN, Env, String};

use crate::errors::Error;
use crate::types::{DataKey, OpData, Position};

const DAY_IN_LEDGERS: u32 = 17_280;

const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;

const OP_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const OP_THRESHOLD: u32 = OP_BUMP - 7 * DAY_IN_LEDGERS;

/// Extends the instance (and code) TTL. Called by every state-changing entry
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

pub fn require_admin(e: &Env) -> Address {
    let admin = read_admin(e);
    admin.require_auth();
    admin
}

pub fn read_usdc(e: &Env) -> Address {
    match e.storage().instance().get(&DataKey::Usdc) {
        Some(usdc) => usdc,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

pub fn usdc_client(e: &Env) -> token::Client<'_> {
    token::Client::new(e, &read_usdc(e))
}

pub fn read_backend_signer(e: &Env) -> BytesN<32> {
    match e.storage().instance().get(&DataKey::BackendSigner) {
        Some(signer) => signer,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

pub fn operation_count(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&DataKey::OperationCount)
        .unwrap_or(0)
}

pub fn set_operation_count(e: &Env, count: u32) {
    e.storage().instance().set(&DataKey::OperationCount, &count);
}

pub fn require_op_exists(e: &Env, id: u32) {
    if !e.storage().persistent().has(&DataKey::Op(id)) {
        panic_with_error!(e, Error::OperationNotFound);
    }
}

// --- operations ---

pub fn read_op(e: &Env, id: u32) -> OpData {
    match e.storage().persistent().get(&DataKey::Op(id)) {
        Some(op) => op,
        None => panic_with_error!(e, Error::OperationNotFound),
    }
}

pub fn write_op(e: &Env, id: u32, op: &OpData) {
    let key = DataKey::Op(id);
    e.storage().persistent().set(&key, op);
    e.storage()
        .persistent()
        .extend_ttl(&key, OP_THRESHOLD, OP_BUMP);
}

pub fn read_op_name(e: &Env, id: u32) -> String {
    match e.storage().persistent().get(&DataKey::OpName(id)) {
        Some(name) => name,
        None => panic_with_error!(e, Error::OperationNotFound),
    }
}

pub fn write_op_name(e: &Env, id: u32, name: &String) {
    let key = DataKey::OpName(id);
    e.storage().persistent().set(&key, name);
    e.storage()
        .persistent()
        .extend_ttl(&key, OP_THRESHOLD, OP_BUMP);
}

// --- positions ---

pub fn read_position(e: &Env, id: u32, user: &Address) -> Position {
    e.storage()
        .persistent()
        .get(&DataKey::Position(id, user.clone()))
        .unwrap_or(Position::ZERO)
}

pub fn write_position(e: &Env, id: u32, user: &Address, pos: &Position) {
    let key = DataKey::Position(id, user.clone());
    e.storage().persistent().set(&key, pos);
    e.storage()
        .persistent()
        .extend_ttl(&key, OP_THRESHOLD, OP_BUMP);
}

// --- blacklist ---

pub fn is_blacklisted(e: &Env, user: &Address) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::Blacklisted(user.clone()))
        .unwrap_or(false)
}

pub fn set_blacklisted(e: &Env, user: &Address, state: bool) {
    let key = DataKey::Blacklisted(user.clone());
    e.storage().persistent().set(&key, &state);
    e.storage()
        .persistent()
        .extend_ttl(&key, OP_THRESHOLD, OP_BUMP);
}

pub fn require_not_blacklisted(e: &Env, user: &Address) {
    if is_blacklisted(e, user) {
        panic_with_error!(e, Error::Blacklisted);
    }
}
