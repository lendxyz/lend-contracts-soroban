use soroban_sdk::{panic_with_error, Address, BytesN, Env, String, Vec};

use crate::crypto::{
    build_fiat_invest_message, build_invest_message, consume_nonce,
    verify_backend_sig,
};
use crate::errors::Error;
use crate::events;
use crate::oracle::{amount_in, amount_out};
use crate::storage as st;
use crate::types::{
    OpData, OpLendClient, F_CANCELED, F_PAUSED, F_PREDEPOSITS, F_STARTED,
};

const MAX_BATCH: u32 = 200;

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

pub fn gift_op_tokens(e: &Env, id: u32, shares_amount: i128, user: Address) {
    let admin = st::require_admin(e);
    st::bump_instance(e);
    st::require_not_blacklisted(e, &user);

    let mut op = load_open(e, id);
    check_fundable(e, &op, shares_amount);

    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    op.funding_progress += shares_amount;
    op.usdc_raised += cost;
    // Fully funding an operation by gifting also starts it.
    if op.fully_funded() {
        op.set_flag(F_STARTED, true);
    }
    st::write_op(e, id, &op);

    let mut pos = st::read_position(e, id, &user);
    pos.invested += cost;
    pos.gifted += shares_amount;
    st::write_position(e, id, &user, &pos);

    st::usdc_client(e).transfer(&admin, e.current_contract_address(), &cost);

    events::Invested {
        investor: user.clone(),
        operation_id: id,
        usdc_amount: cost,
        shares_bought: shares_amount,
    }
    .publish(e);
    events::Gifted {
        investor: user,
        operation_id: id,
        usdc_amount: cost,
        shares_bought: shares_amount,
    }
    .publish(e);

    maybe_finish(e, id, &op);
}

pub fn predeposit(
    e: &Env,
    user: Address,
    id: u32,
    shares_amount: i128,
    nonce: String,
    signature: BytesN<64>,
) {
    user.require_auth();
    st::bump_instance(e);

    let mut op = load_open(e, id);
    st::require_not_blacklisted(e, &user);

    if op.flag(F_STARTED) {
        panic_with_error!(e, Error::OperationAlreadyStarted);
    }

    if !op.flag(F_PREDEPOSITS) {
        panic_with_error!(e, Error::PredepositsClosed);
    }

    check_fundable(e, &op, shares_amount);

    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    let msg = build_invest_message(e, id, &user, shares_amount, &nonce);
    verify_backend_sig(e, &st::read_backend_signer(e), &msg, &signature);
    consume_nonce(e, &nonce);

    op.funding_progress += shares_amount;
    op.usdc_raised += cost;

    if op.fully_funded() {
        op.set_flag(F_STARTED, true);
    }

    st::write_op(e, id, &op);

    let mut pos = st::read_position(e, id, &user);
    pos.invested += cost;
    pos.predeposited += shares_amount;
    st::write_position(e, id, &user, &pos);

    st::usdc_client(e).transfer(&user, e.current_contract_address(), &cost);

    events::Invested {
        investor: user.clone(),
        operation_id: id,
        usdc_amount: cost,
        shares_bought: shares_amount,
    }
    .publish(e);
    events::Predeposit {
        investor: user,
        operation_id: id,
        usdc_amount: cost,
        shares_bought: shares_amount,
    }
    .publish(e);

    maybe_finish(e, id, &op);
}

fn claim_token(
    e: &Env,
    id: u32,
    oplend: &OpLendClient,
    user: &Address,
    dest: &Address,
) {
    st::require_not_blacklisted(e, user);

    let mut pos = st::read_position(e, id, user);
    let amount = pos.claimable();
    if amount == 0 {
        return;
    }

    pos.predeposited = 0;
    pos.gifted = 0;
    st::write_position(e, id, user, &pos);

    oplend.mint(dest, &amount);
    events::ClaimedOpToken {
        investor: user.clone(),
        operation_id: id,
        amount,
    }
    .publish(e);
}

fn require_claimable(e: &Env, id: u32) -> OpData {
    let op = st::read_op(e, id);
    if !op.flag(F_STARTED) {
        panic_with_error!(e, Error::OperationNotStarted);
    }
    if op.flag(F_CANCELED) {
        panic_with_error!(e, Error::OperationCanceled);
    }
    op
}

pub fn claim_op_tokens(e: &Env, id: u32, user: Address) {
    st::bump_instance(e);
    let op = require_claimable(e, id);
    claim_token(e, id, &OpLendClient::new(e, &op.op_token), &user, &user);
}

pub fn claim_op_tokens_batch(e: &Env, id: u32, users: Vec<Address>) {
    if users.len() > MAX_BATCH {
        panic_with_error!(e, Error::BatchTooLarge);
    }
    st::bump_instance(e);
    let op = require_claimable(e, id);
    // One client, one op read for the whole batch.
    let oplend = OpLendClient::new(e, &op.op_token);
    for user in users.iter() {
        claim_token(e, id, &oplend, &user, &user);
    }
}

pub fn get_amount_in(e: &Env, id: u32, shares_amount: i128) -> i128 {
    amount_in(e, st::read_op(e, id).eur_per_shares, shares_amount)
}

pub fn get_amount_out(e: &Env, id: u32, usdc_amount: i128) -> i128 {
    amount_out(e, st::read_op(e, id).eur_per_shares, usdc_amount)
}
