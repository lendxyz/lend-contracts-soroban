use soroban_sdk::{token, Address, BytesN, Env, String, Vec};

use crate::crypto::{
    build_fiat_invest_message, build_invest_message, consume_nonce,
    read_backend_signer, verify_backend_sig,
};
use crate::events;
use crate::oracle::{amount_in, amount_out};
use crate::storage::{
    funding_progress, get_operation, is_finished, operation_canceled,
    operation_started, predeposits, predeposits_open, require_admin,
    require_not_blacklisted, require_op_exists, set_funding_progress,
    set_gifted, set_predeposits, set_usdc_raised, set_user_invested, gifted,
    read_usdc, usdc_raised, user_invested,
};
use crate::types::{DataKey, OpLendClient, Operation};

const MAX_BATCH: u32 = 200;

/// Shared invest pre-conditions, mirroring the EVM `_invest` guard chain.
/// Returns the operation.
fn invest_guards(e: &Env, id: u32, shares_amount: i128) -> Operation {
    if is_finished(e, id) {
        panic!("operation finished");
    }
    require_op_exists(e, id);
    if !operation_started(e, id) {
        panic!("operation not started");
    }
    let op = get_operation(e, id);
    if funding_progress(e, id) + shares_amount > op.total_shares {
        panic!("too many shares");
    }
    if operation_canceled(e, id) {
        panic!("operation canceled");
    }
    if crate::storage::funding_paused(e, id) {
        panic!("operation paused");
    }
    if shares_amount <= 0 {
        panic!("zero shares");
    }
    op
}

fn record_investment(e: &Env, id: u32, user: &Address, cost: i128, shares: i128) {
    set_funding_progress(e, id, funding_progress(e, id) + shares);
    set_usdc_raised(e, id, usdc_raised(e, id) + cost);
    set_user_invested(e, id, user, user_invested(e, id, user) + cost);
}

fn usdc_client(e: &Env) -> token::Client<'_> {
    token::Client::new(e, &read_usdc(e))
}

fn maybe_finish(e: &Env, id: u32, op: &Operation) {
    if funding_progress(e, id) >= op.total_shares {
        events::OperationFinished {
            operation_id: id,
            amount_raised_euro: op.total_shares * op.eur_per_shares,
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
    require_not_blacklisted(e, &user);

    let op = invest_guards(e, id, shares_amount);
    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    let msg = build_invest_message(e, id, &user, shares_amount, &nonce);
    verify_backend_sig(e, &read_backend_signer(e), &msg, &signature);
    consume_nonce(e, &nonce);

    record_investment(e, id, &user, cost, shares_amount);
    usdc_client(e).transfer(&user, &e.current_contract_address(), &cost);

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
    if is_finished(e, id) {
        panic!("operation finished");
    }
    require_op_exists(e, id);
    let op = get_operation(e, id);
    if funding_progress(e, id) + shares_amount > op.total_shares {
        panic!("too many shares");
    }
    if operation_canceled(e, id) {
        panic!("operation canceled");
    }
    if crate::storage::funding_paused(e, id) {
        panic!("operation paused");
    }
    if shares_amount <= 0 {
        panic!("zero shares");
    }
    require_not_blacklisted(e, &user);
    require_not_blacklisted(e, &oplend_holder);

    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    let msg =
        build_fiat_invest_message(e, id, &user, &oplend_holder, shares_amount, &nonce);
    verify_backend_sig(e, &read_backend_signer(e), &msg, &signature);
    consume_nonce(e, &nonce);

    set_funding_progress(e, id, funding_progress(e, id) + shares_amount);

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
    let admin = require_admin(e);

    require_not_blacklisted(e, &user);
    if is_finished(e, id) {
        panic!("operation finished");
    }
    require_op_exists(e, id);
    let op = get_operation(e, id);
    if funding_progress(e, id) + shares_amount > op.total_shares {
        panic!("too many shares");
    }
    if operation_canceled(e, id) {
        panic!("operation canceled");
    }
    if crate::storage::funding_paused(e, id) {
        panic!("operation paused");
    }
    if shares_amount <= 0 {
        panic!("zero shares");
    }

    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    record_investment(e, id, &user, cost, shares_amount);
    set_gifted(e, id, &user, gifted(e, id, &user) + shares_amount);

    usdc_client(e).transfer(&admin, &e.current_contract_address(), &cost);

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

    if funding_progress(e, id) >= op.total_shares {
        e.storage()
            .persistent()
            .set(&DataKey::OperationStarted(id), &true);
        maybe_finish(e, id, &op);
    }
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

    if is_finished(e, id) {
        panic!("operation finished");
    }
    require_not_blacklisted(e, &user);
    require_op_exists(e, id);
    if operation_started(e, id) {
        panic!("operation already started");
    }
    if !predeposits_open(e, id) {
        panic!("predeposits not open");
    }
    let op = get_operation(e, id);
    if funding_progress(e, id) + shares_amount > op.total_shares {
        panic!("too many shares");
    }
    if operation_canceled(e, id) {
        panic!("operation canceled");
    }
    if crate::storage::funding_paused(e, id) {
        panic!("operation paused");
    }
    if shares_amount <= 0 {
        panic!("zero shares");
    }

    let cost = amount_in(e, op.eur_per_shares, shares_amount);

    let msg = build_invest_message(e, id, &user, shares_amount, &nonce);
    verify_backend_sig(e, &read_backend_signer(e), &msg, &signature);
    consume_nonce(e, &nonce);

    record_investment(e, id, &user, cost, shares_amount);
    set_predeposits(e, id, &user, predeposits(e, id, &user) + shares_amount);

    usdc_client(e).transfer(&user, &e.current_contract_address(), &cost);

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

    if funding_progress(e, id) >= op.total_shares {
        e.storage()
            .persistent()
            .set(&DataKey::OperationStarted(id), &true);
        maybe_finish(e, id, &op);
    }
}

/// Mints any gifted + predeposited shares for `user` to `dest`, zeroing them.
fn claim_token(e: &Env, id: u32, op: &Operation, user: &Address, dest: &Address) {
    require_not_blacklisted(e, user);

    let amount = gifted(e, id, user) + predeposits(e, id, user);
    if predeposits(e, id, user) > 0 {
        set_predeposits(e, id, user, 0);
    }
    if gifted(e, id, user) > 0 {
        set_gifted(e, id, user, 0);
    }

    if amount > 0 {
        OpLendClient::new(e, &op.op_token).mint(dest, &amount);
        events::ClaimedOpToken {
            investor: user.clone(),
            operation_id: id,
            amount,
        }
        .publish(e);
    }
}

fn require_claimable(e: &Env, id: u32) -> Operation {
    require_op_exists(e, id);
    if !operation_started(e, id) {
        panic!("operation not started");
    }
    if operation_canceled(e, id) {
        panic!("operation canceled");
    }
    get_operation(e, id)
}

pub fn claim_op_tokens(e: &Env, id: u32, user: Address) {
    let op = require_claimable(e, id);
    claim_token(e, id, &op, &user, &user);
}

pub fn claim_op_tokens_batch(e: &Env, id: u32, users: Vec<Address>) {
    if users.len() > MAX_BATCH {
        panic!("batch too large");
    }
    let op = require_claimable(e, id);
    for user in users.iter() {
        claim_token(e, id, &op, &user, &user);
    }
}

pub fn get_amount_in(e: &Env, id: u32, shares_amount: i128) -> i128 {
    let op = get_operation(e, id);
    amount_in(e, op.eur_per_shares, shares_amount)
}

pub fn get_amount_out(e: &Env, id: u32, usdc_amount: i128) -> i128 {
    let op = get_operation(e, id);
    amount_out(e, op.eur_per_shares, usdc_amount)
}
