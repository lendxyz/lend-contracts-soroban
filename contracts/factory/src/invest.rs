use soroban_sdk::{panic_with_error, Address, BytesN, Env, String};

use crate::crypto::{
    build_fiat_invest_message, build_invest_message, build_void_nonce_message,
    consume_nonce, verify_backend_sig,
};
use crate::errors::Error;
use crate::events;
use crate::oracle::{amount_in, amount_out};
use crate::storage as st;
use crate::types::{OpData, OpLendClient, F_CANCELED, F_PAUSED, F_STARTED};

fn load_open(e: &Env, id: u32) -> OpData {
    let op = st::read_op(e, id);
    if op.finished() {
        panic_with_error!(e, Error::OperationFinished);
    }
    op
}

fn check_fundable(e: &Env, op: &OpData, shares: i128) {
    if op.funding_progress + shares > op.total_shares {
        panic_with_error!(e, Error::TooManyShares);
    }
    if op.flag(F_CANCELED) {
        panic_with_error!(e, Error::OperationCanceled);
    }
    if op.flag(F_PAUSED) {
        panic_with_error!(e, Error::OperationPaused);
    }
    if shares <= 0 {
        panic_with_error!(e, Error::InvalidShares);
    }
}

fn maybe_finish(e: &Env, id: u32, op: &OpData) {
    if op.fully_funded() {
        events::OperationFinished {
            operation_id: id,
            amount_raised_euro: op.raised_euro(),
        }
        .publish(e);
    }
}

pub fn invest(
    e: &Env,
    user: Address,
    id: u32,
    shares_amount: i128,
    nonce: String,
    signature: BytesN<64>,
) {
    user.require_auth();
    st::bump_instance(e);
    st::require_not_blacklisted(e, &user);

    let mut op = load_open(e, id);
    if !op.flag(F_STARTED) {
        panic_with_error!(e, Error::OperationNotStarted);
    }
    check_fundable(e, &op, shares_amount);

    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    let msg = build_invest_message(e, id, &user, shares_amount, &nonce);
    verify_backend_sig(e, &st::read_backend_signer(e), &msg, &signature);
    consume_nonce(e, &nonce);

    op.funding_progress += shares_amount;
    op.usdc_raised += cost;
    st::write_op(e, id, &op);

    let mut pos = st::read_position(e, id, &user);
    pos.invested += cost;
    st::write_position(e, id, &user, &pos);

    st::usdc_client(e).transfer(&user, e.current_contract_address(), &cost);

    events::Invested {
        investor: user.clone(),
        operation_id: id,
        usdc_amount: cost,
        shares_bought: shares_amount,
    }
    .publish(e);

    OpLendClient::new(e, &op.op_token).mint(&user, &shares_amount);
    maybe_finish(e, id, &op);
}

pub fn fiat_invest(
    e: &Env,
    id: u32,
    shares_amount: i128,
    user: Address,
    oplend_holder: Address,
    nonce: String,
    signature: BytesN<64>,
) {
    st::bump_instance(e);

    let mut op = load_open(e, id);
    check_fundable(e, &op, shares_amount);
    st::require_not_blacklisted(e, &user);
    st::require_not_blacklisted(e, &oplend_holder);

    // Fiat is settled off-chain: shares are minted but no USDC is recorded.
    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    let msg = build_fiat_invest_message(
        e,
        id,
        &user,
        &oplend_holder,
        shares_amount,
        &nonce,
    );
    verify_backend_sig(e, &st::read_backend_signer(e), &msg, &signature);
    consume_nonce(e, &nonce);

    op.funding_progress += shares_amount;
    st::write_op(e, id, &op);

    let oplend = OpLendClient::new(e, &op.op_token);
    oplend.mint(&oplend_holder, &shares_amount);
    oplend.whitelist_user_admin(&user, &true);

    events::Invested {
        investor: user.clone(),
        operation_id: id,
        usdc_amount: cost,
        shares_bought: shares_amount,
    }
    .publish(e);
    events::InvestedFiat {
        investor: user,
        oplend_destination: oplend_holder,
        operation_id: id,
        shares_bought: shares_amount,
    }
    .publish(e);

    maybe_finish(e, id, &op);
}

/// Burns an unspent `nonce` so the invest signature carrying it can never be
/// used. With a `signature`, authorized by the backend signing the void
/// message (not the original signature, which binds fields this call does not
/// take); without one, by the admin's auth.
pub fn void_nonce(e: &Env, nonce: String, signature: Option<BytesN<64>>) {
    match signature {
        Some(signature) => {
            let msg = build_void_nonce_message(e, &nonce);
            verify_backend_sig(
                e,
                &st::read_backend_signer(e),
                &msg,
                &signature,
            );
        }
        None => {
            st::require_admin(e);
        }
    }
    st::bump_instance(e);
    consume_nonce(e, &nonce);

    events::NonceVoided { nonce }.publish(e);
}

pub fn get_amount_in(e: &Env, id: u32, shares_amount: i128) -> i128 {
    amount_in(e, st::read_op(e, id).eur_per_shares, shares_amount)
}

pub fn get_amount_out(e: &Env, id: u32, usdc_amount: i128) -> i128 {
    amount_out(e, st::read_op(e, id).eur_per_shares, usdc_amount)
}
