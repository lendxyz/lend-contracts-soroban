use soroban_sdk::{Address, Env};

use crate::storage as st;
use crate::types::{
    Operation, F_CANCELED, F_PAUSED, F_PREDEPOSITS, F_STARTED, F_WITHDRAWN,
};

pub fn usdc(e: &Env) -> Address {
    st::read_usdc(e)
}

pub fn operation_count(e: &Env) -> u32 {
    st::operation_count(e)
}

pub fn operations(e: &Env, id: u32) -> Operation {
    let op = st::read_op(e, id);
    Operation {
        op_token: op.op_token,
        total_shares: op.total_shares,
        eur_per_shares: op.eur_per_shares,
        op_name: st::read_op_name(e, id),
    }
}

pub fn funding_progress(e: &Env, id: u32) -> i128 {
    st::read_op(e, id).funding_progress
}

pub fn usdc_raised(e: &Env, id: u32) -> i128 {
    st::read_op(e, id).usdc_raised
}

pub fn funding_paused(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_PAUSED)
}

pub fn operation_started(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_STARTED)
}

pub fn usdc_withdrawn(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_WITHDRAWN)
}

pub fn operation_canceled(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_CANCELED)
}

pub fn predeposits_open(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_PREDEPOSITS)
}

pub fn is_operation_finished(e: &Env, id: u32) -> bool {
    st::read_op(e, id).finished()
}

pub fn usdc_raised_per_client(e: &Env, id: u32, user: Address) -> i128 {
    st::require_op_exists(e, id);
    st::read_position(e, id, &user).invested
}

pub fn predeposits(e: &Env, id: u32, user: Address) -> i128 {
    st::require_op_exists(e, id);
    st::read_position(e, id, &user).predeposited
}

pub fn gifted(e: &Env, id: u32, user: Address) -> i128 {
    st::require_op_exists(e, id);
    st::read_position(e, id, &user).gifted
}

pub fn claimable_total(e: &Env, id: u32, user: Address) -> i128 {
    st::require_op_exists(e, id);
    st::read_position(e, id, &user).claimable()
}

pub fn blacklisted(e: &Env, user: Address) -> bool {
    st::is_blacklisted(e, &user)
}
