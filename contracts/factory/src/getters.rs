use soroban_sdk::{Address, Env};

use crate::storage;
use crate::types::Operation;

pub fn usdc(e: &Env) -> Address {
    storage::read_usdc(e)
}

pub fn operation_count(e: &Env) -> u32 {
    storage::operation_count(e)
}

pub fn operations(e: &Env, id: u32) -> Operation {
    storage::get_operation(e, id)
}

pub fn funding_progress(e: &Env, id: u32) -> i128 {
    storage::require_op_exists(e, id);
    storage::funding_progress(e, id)
}

pub fn usdc_raised(e: &Env, id: u32) -> i128 {
    storage::require_op_exists(e, id);
    storage::usdc_raised(e, id)
}

pub fn funding_paused(e: &Env, id: u32) -> bool {
    storage::require_op_exists(e, id);
    storage::funding_paused(e, id)
}

pub fn operation_started(e: &Env, id: u32) -> bool {
    storage::require_op_exists(e, id);
    storage::operation_started(e, id)
}

pub fn usdc_withdrawn(e: &Env, id: u32) -> bool {
    storage::require_op_exists(e, id);
    storage::usdc_withdrawn(e, id)
}

pub fn operation_canceled(e: &Env, id: u32) -> bool {
    storage::require_op_exists(e, id);
    storage::operation_canceled(e, id)
}

pub fn is_operation_finished(e: &Env, id: u32) -> bool {
    storage::require_op_exists(e, id);
    storage::is_finished(e, id)
}

pub fn usdc_raised_per_client(e: &Env, id: u32, user: Address) -> i128 {
    storage::require_op_exists(e, id);
    storage::user_invested(e, id, &user)
}

pub fn predeposits(e: &Env, id: u32, user: Address) -> i128 {
    storage::require_op_exists(e, id);
    storage::predeposits(e, id, &user)
}

pub fn gifted(e: &Env, id: u32, user: Address) -> i128 {
    storage::require_op_exists(e, id);
    storage::gifted(e, id, &user)
}

pub fn claimable_total(e: &Env, id: u32, user: Address) -> i128 {
    storage::require_op_exists(e, id);
    storage::gifted(e, id, &user) + storage::predeposits(e, id, &user)
}

pub fn predeposits_open(e: &Env, id: u32) -> bool {
    storage::require_op_exists(e, id);
    storage::predeposits_open(e, id)
}

pub fn blacklisted(e: &Env, user: Address) -> bool {
    storage::is_blacklisted(e, &user)
}
