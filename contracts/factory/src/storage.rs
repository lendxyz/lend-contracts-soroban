use soroban_sdk::{Address, Env};

use crate::types::{DataKey, Operation};

pub fn read_admin(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("not initialized")
}

/// Reads the admin and requires its authorization (owner-only gate).
pub fn require_admin(e: &Env) -> Address {
    let admin = read_admin(e);
    admin.require_auth();
    admin
}

pub fn read_usdc(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::USDC).unwrap()
}

pub fn operation_count(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&DataKey::OperationCount)
        .unwrap_or(0)
}

/// Panics with `OpNotExist`-equivalent if the id is out of range.
pub fn require_op_exists(e: &Env, id: u32) {
    if id == 0 || id > operation_count(e) {
        panic!("operation does not exist");
    }
}

pub fn get_operation(e: &Env, id: u32) -> Operation {
    require_op_exists(e, id);
    e.storage()
        .persistent()
        .get(&DataKey::Operation(id))
        .expect("operation does not exist")
}

pub fn funding_progress(e: &Env, id: u32) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::FundingProgress(id))
        .unwrap_or(0)
}

pub fn set_funding_progress(e: &Env, id: u32, v: i128) {
    e.storage().persistent().set(&DataKey::FundingProgress(id), &v);
}

pub fn usdc_raised(e: &Env, id: u32) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::UsdcRaised(id))
        .unwrap_or(0)
}

pub fn set_usdc_raised(e: &Env, id: u32, v: i128) {
    e.storage().persistent().set(&DataKey::UsdcRaised(id), &v);
}

pub fn operation_started(e: &Env, id: u32) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::OperationStarted(id))
        .unwrap_or(false)
}

pub fn operation_canceled(e: &Env, id: u32) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::OperationCanceled(id))
        .unwrap_or(false)
}

pub fn funding_paused(e: &Env, id: u32) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::FundingPaused(id))
        .unwrap_or(false)
}

pub fn predeposits_open(e: &Env, id: u32) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::PredepositsOpen(id))
        .unwrap_or(false)
}

pub fn usdc_withdrawn(e: &Env, id: u32) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::UsdcWithdrew(id))
        .unwrap_or(false)
}

/// Whether the operation has started and is fully funded.
pub fn is_finished(e: &Env, id: u32) -> bool {
    operation_started(e, id) && funding_progress(e, id) >= get_operation(e, id).total_shares
}

pub fn is_blacklisted(e: &Env, user: &Address) -> bool {
    e.storage()
        .persistent()
        .get(&DataKey::Blacklisted(user.clone()))
        .unwrap_or(false)
}

pub fn require_not_blacklisted(e: &Env, user: &Address) {
    if is_blacklisted(e, user) {
        panic!("user is blacklisted");
    }
}

pub fn predeposits(e: &Env, id: u32, user: &Address) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::Predeposits(id, user.clone()))
        .unwrap_or(0)
}

pub fn set_predeposits(e: &Env, id: u32, user: &Address, v: i128) {
    e.storage()
        .persistent()
        .set(&DataKey::Predeposits(id, user.clone()), &v);
}

pub fn gifted(e: &Env, id: u32, user: &Address) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::Gifted(id, user.clone()))
        .unwrap_or(0)
}

pub fn set_gifted(e: &Env, id: u32, user: &Address, v: i128) {
    e.storage()
        .persistent()
        .set(&DataKey::Gifted(id, user.clone()), &v);
}

pub fn user_invested(e: &Env, id: u32, user: &Address) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::UserInvested(id, user.clone()))
        .unwrap_or(0)
}

pub fn set_user_invested(e: &Env, id: u32, user: &Address, v: i128) {
    e.storage()
        .persistent()
        .set(&DataKey::UserInvested(id, user.clone()), &v);
}
