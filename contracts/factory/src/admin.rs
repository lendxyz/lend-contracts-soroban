use soroban_sdk::{panic_with_error, token, Address, BytesN, Env, Vec};

use crate::errors::Error;
use crate::events;
use crate::oracle::set_oracle;
use crate::storage as st;
use crate::types::{DataKey, OpData, OpLendClient, F_STARTED, F_WITHDRAWN};

fn refund_one(
    e: &Env,
    id: u32,
    op: &mut OpData,
    oplend: &OpLendClient,
    usdc: &token::Client,
    user: Address,
) {
    let mut pos = st::read_position(e, id, &user);
    let invested = pos.invested;
    let oplend_balance = oplend.balance(&user);

    if invested == 0 {
        panic_with_error!(e, Error::NoParticipation);
    }
    if oplend_balance == 0 {
        panic_with_error!(e, Error::NoOpLendBalance);
    }

    op.funding_progress -= oplend_balance;
    op.usdc_raised -= invested;
    pos.invested = 0;
    st::write_position(e, id, &user, &pos);

    oplend.admin_burn(&user, &oplend_balance);
    usdc.transfer(&e.current_contract_address(), &user, &invested);

    events::Refunded {
        investor: user,
        operation_id: id,
        usdc_amount: invested,
        shares_refunded: oplend_balance,
    }
    .publish(e);
}

pub fn refund_user(e: &Env, id: u32, user: Address) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut op = st::read_op(e, id);
    let oplend = OpLendClient::new(e, &op.op_token);
    refund_one(e, id, &mut op, &oplend, &st::usdc_client(e), user);
    st::write_op(e, id, &op);
}

pub fn batch_refund_users(e: &Env, id: u32, users: Vec<Address>, len: u32) {
    st::require_admin(e);
    st::bump_instance(e);
    if len == 0 || len > users.len() {
        panic_with_error!(e, Error::InvalidBatchLen);
    }

    let mut op = st::read_op(e, id);
    let oplend = OpLendClient::new(e, &op.op_token);
    let usdc = st::usdc_client(e);
    for user in users.iter().take(len as usize) {
        refund_one(e, id, &mut op, &oplend, &usdc, user);
    }
    st::write_op(e, id, &op);
}

pub fn update_oracle_address(e: &Env, new_oracle: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    set_oracle(e, &new_oracle);
}

pub fn update_backend_signer(e: &Env, new_signer: BytesN<32>) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage()
        .instance()
        .set(&DataKey::BackendSigner, &new_signer);
}

pub fn blacklist(e: &Env, user: Address, state: bool) {
    st::require_admin(e);
    st::bump_instance(e);
    if user == e.current_contract_address() {
        panic_with_error!(e, Error::SelfTargetNotAllowed);
    }
    st::set_blacklisted(e, &user, state);
}

pub fn oplend_whitelist_user(e: &Env, op_id: u32, user: Address, state: bool) {
    st::require_admin(e);
    st::bump_instance(e);
    if user == e.current_contract_address() {
        panic_with_error!(e, Error::SelfTargetNotAllowed);
    }
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).whitelist_user_admin(&user, &state);
}

pub fn oplend_update_backend_signer(
    e: &Env,
    op_id: u32,
    new_signer: BytesN<32>,
) {
    st::require_admin(e);
    st::bump_instance(e);
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).update_backend_signer(&new_signer);
}

pub fn oplend_admin_burn(e: &Env, op_id: u32, user: Address, value: i128) {
    st::require_admin(e);
    st::bump_instance(e);
    if value <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).admin_burn(&user, &value);
}

pub fn withdraw_usdc(e: &Env, id: u32, destination: Address) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut op = st::read_op(e, id);
    if !op.flag(F_STARTED) || !op.fully_funded() {
        panic_with_error!(e, Error::OperationNotFinished);
    }
    if op.flag(F_WITHDRAWN) {
        panic_with_error!(e, Error::AlreadyWithdrawn);
    }
    op.set_flag(F_WITHDRAWN, true);
    st::write_op(e, id, &op);

    st::usdc_client(e).transfer(
        &e.current_contract_address(),
        &destination,
        &op.usdc_raised,
    );
}

pub fn transfer_ownership(e: &Env, new_admin: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage().instance().set(&DataKey::Admin, &new_admin);
}
