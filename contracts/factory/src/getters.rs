use soroban_sdk::{Address, BytesN, Env};

use crate::storage as st;
use crate::types::{Coin, Operation, F_CANCELED, F_PAUSED, F_STARTED};

pub fn token(e: &Env, coin: Coin) -> Address {
    st::read_token(e, coin)
}

pub fn oplend_wasm_hash(e: &Env) -> BytesN<32> {
    st::read_oplend_wasm_hash(e)
}

pub fn backend_signer(e: &Env) -> BytesN<32> {
    st::read_backend_signer(e)
}

pub fn operation_count(e: &Env) -> u32 {
    st::operation_count(e)
}

pub fn operations(e: &Env, id: u32) -> Operation {
    let op = st::read_op(e, id);
    Operation {
        op_token: op.op_token,
        total_shares: op.total_shares,
        eur_per_share: op.eur_per_share,
        op_name: st::read_op_name(e, id),
    }
}

pub fn funding_progress(e: &Env, id: u32) -> i128 {
    st::read_op(e, id).funding_progress
}

pub fn raised(e: &Env, coin: Coin, id: u32) -> i128 {
    st::read_op(e, id).escrow(coin).raised
}

pub fn funding_paused(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_PAUSED)
}

pub fn operation_started(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_STARTED)
}

pub fn withdrawn_amount(e: &Env, coin: Coin, id: u32) -> i128 {
    st::read_op(e, id).escrow(coin).withdrawn
}

/// Whether any of the operation's funds, in either coin, has been withdrawn.
pub fn funds_withdrawn(e: &Env, id: u32) -> bool {
    let op = st::read_op(e, id);
    Coin::ALL
        .into_iter()
        .any(|coin| op.escrow(coin).withdrawn > 0)
}

pub fn operation_canceled(e: &Env, id: u32) -> bool {
    st::read_op(e, id).flag(F_CANCELED)
}

pub fn is_operation_finished(e: &Env, id: u32) -> bool {
    st::read_op(e, id).finished()
}

pub fn raised_per_client(e: &Env, coin: Coin, id: u32, user: Address) -> i128 {
    st::require_op_exists(e, id);
    st::read_position(e, id, &user).invested(coin)
}

pub fn blacklisted(e: &Env, user: Address) -> bool {
    st::is_blacklisted(e, &user)
}
