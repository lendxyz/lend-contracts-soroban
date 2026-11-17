use soroban_sdk::{
    panic_with_error, vec, Address, BytesN, Env, IntoVal, String, Val, Vec,
};

use crate::errors::Error;
use crate::events;
use crate::storage as st;
use crate::types::{OpData, F_CANCELED, F_PAUSED, F_STARTED};
use crate::utils::{op_symbol, prefixed};

pub fn create_operation(
    e: &Env,
    op_name: String,
    total_shares: i128,
    eur_per_shares: i128,
) -> Address {
    st::require_admin(e);
    st::bump_instance(e);

    if total_shares <= 0 {
        panic_with_error!(e, Error::InvalidShares);
    }
    if eur_per_shares <= 0 {
        panic_with_error!(e, Error::InvalidPrice);
    }
    // The operation's EUR value (`OpData::raised_euro`) is computed from this
    // product when the last share sells; if it overflowed there, the
    // operation could never finish.
    if total_shares.checked_mul(eur_per_shares).is_none() {
        panic_with_error!(e, Error::RaiseOverflow);
    }

    let id = st::operation_count(e) + 1;

    // op-lend __constructor(admin, decimal, name, symbol, max_supply, factory):
    // the factory is both its admin and where it reads the backend signer.
    let constructor_args: Vec<Val> = vec![
        e,
        e.current_contract_address().into_val(e),
        6u32.into_val(e),
        prefixed(e, b"Lend Operation - ", &op_name).into_val(e),
        op_symbol(e, id).into_val(e),
        total_shares.into_val(e),
        e.current_contract_address().into_val(e),
    ];

    let mut salt = [0u8; 32];
    salt[28..32].copy_from_slice(&id.to_be_bytes());

    let wasm_hash = st::read_oplend_wasm_hash(e);
    let op_token = e
        .deployer()
        .with_current_contract(BytesN::from_array(e, &salt))
        .deploy_v2(wasm_hash, constructor_args);

    st::write_op(
        e,
        id,
        &OpData {
            op_token: op_token.clone(),
            total_shares,
            eur_per_shares,
            funding_progress: 0,
            usdc_raised: 0,
            flags: 0,
        },
    );
    st::write_op_name(e, id, &op_name);
    st::set_operation_count(e, id);

    events::OperationCreated {
        op_token: op_token.clone(),
        operation_id: id,
        total_shares,
    }
    .publish(e);

    op_token
}

/// Sets an operation flag, failing if it already has the requested value: a
/// repeated start / cancel / pause / resume is a mistake, and would otherwise
/// emit an event for a change that never happened.
fn set_flag(e: &Env, id: u32, flag: u32, state: bool) {
    st::require_admin(e);
    st::bump_instance(e);
    let mut op = st::read_op(e, id);
    if op.flag(flag) == state {
        panic_with_error!(e, Error::StateUnchanged);
    }
    op.set_flag(flag, state);
    st::write_op(e, id, &op);
}

pub fn cancel_operation(e: &Env, id: u32) {
    set_flag(e, id, F_CANCELED, true);
    events::OperationCanceled { operation_id: id }.publish(e);
}

pub fn start_operation(e: &Env, id: u32) {
    set_flag(e, id, F_STARTED, true);
    events::OperationStarted { operation_id: id }.publish(e);
}

pub fn pause_funding(e: &Env, id: u32, state: bool) {
    set_flag(e, id, F_PAUSED, state);
    if state {
        events::OperationPaused { operation_id: id }.publish(e);
    } else {
        events::OperationResumed { operation_id: id }.publish(e);
    }
}
