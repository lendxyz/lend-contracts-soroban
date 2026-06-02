use soroban_sdk::{token, Address, BytesN, Env, Vec};

use crate::events;
use crate::storage::{
    funding_progress, get_operation, read_usdc, require_admin,
    require_op_exists, set_funding_progress, set_usdc_raised, set_user_invested,
    usdc_raised, user_invested,
};
use crate::types::{DataKey, OpLendClient};

pub fn refund_user(e: &Env, id: u32, user: Address) {
    require_admin(e);
    let op = get_operation(e, id);

    let oplend = OpLendClient::new(e, &op.op_token);
    let user_invest_amount = user_invested(e, id, &user);
    let oplend_balance = oplend.balance(&user);

    if user_invest_amount == 0 {
        panic!("user did not participate");
    }
    if oplend_balance == 0 {
        panic!("no oplend balance");
    }

    set_funding_progress(e, id, funding_progress(e, id) - oplend_balance);
    set_usdc_raised(e, id, usdc_raised(e, id) - user_invest_amount);
    set_user_invested(e, id, &user, 0);

    oplend.admin_burn(&user, &oplend_balance);
    token::Client::new(e, &read_usdc(e)).transfer(
        &e.current_contract_address(),
        &user,
        &user_invest_amount,
    );

    events::Refunded {
        investor: user,
        operation_id: id,
        usdc_amount: user_invest_amount,
        shares_refunded: oplend_balance,
    }
    .publish(e);
}

pub fn batch_refund_users(e: &Env, id: u32, users: Vec<Address>, len: u32) {
    require_admin(e);
    if len == 0 || len > users.len() {
        panic!("invalid len");
    }
    for i in 0..len {
        refund_user(e, id, users.get(i).unwrap());
    }
}

pub fn update_oracle_address(e: &Env, new_oracle: Address) {
    require_admin(e);
    e.storage().instance().set(&DataKey::Oracle, &new_oracle);
}

pub fn update_backend_signer(e: &Env, new_signer: BytesN<32>) {
    require_admin(e);
    e.storage()
        .instance()
        .set(&DataKey::BackendSigner, &new_signer);
}

pub fn blacklist(e: &Env, user: Address, state: bool) {
    require_admin(e);
    if user == e.current_contract_address() {
        panic!("cannot blacklist factory");
    }
    e.storage()
        .persistent()
        .set(&DataKey::Blacklisted(user), &state);
}

pub fn oplend_whitelist_user(e: &Env, op_id: u32, user: Address, state: bool) {
    require_admin(e);
    if user == e.current_contract_address() {
        panic!("cannot whitelist factory");
    }
    let op = get_operation(e, op_id);
    OpLendClient::new(e, &op.op_token).whitelist_user_admin(&user, &state);
}

pub fn oplend_update_backend_signer(e: &Env, op_id: u32, new_signer: BytesN<32>) {
    require_admin(e);
    let op = get_operation(e, op_id);
    OpLendClient::new(e, &op.op_token).update_backend_signer(&new_signer);
}

pub fn oplend_admin_burn(e: &Env, op_id: u32, user: Address, value: i128) {
    require_admin(e);
    if value <= 0 {
        panic!("no amount specified");
    }
    let op = get_operation(e, op_id);
    OpLendClient::new(e, &op.op_token).admin_burn(&user, &value);
}

pub fn withdraw_usdc(e: &Env, id: u32, destination: Address) {
    require_admin(e);
    require_op_exists(e, id);

    if !crate::storage::is_finished(e, id) {
        panic!("operation not finished");
    }
    if crate::storage::usdc_withdrawn(e, id) {
        panic!("already withdrawn");
    }

    e.storage()
        .persistent()
        .set(&DataKey::UsdcWithdrew(id), &true);

    let raised = usdc_raised(e, id);
    token::Client::new(e, &read_usdc(e)).transfer(
        &e.current_contract_address(),
        &destination,
        &raised,
    );
}

pub fn transfer_ownership(e: &Env, new_admin: Address) {
    require_admin(e);
    e.storage().instance().set(&DataKey::Admin, &new_admin);
}
