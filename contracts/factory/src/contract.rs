use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, String, Vec,
};

use crate::types::{DataKey, Operation};
use crate::{admin, getters, invest, operations};

#[contract]
pub struct LendFactory;

#[contractimpl]
impl LendFactory {
    pub fn initialize(
        env: Env,
        admin: Address,
        usdc: Address,
        oracle: Address,
        backend_signer: BytesN<32>,
        oplend_wasm_hash: BytesN<32>,
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::USDC, &usdc);
        env.storage().instance().set(&DataKey::Oracle, &oracle);
        env.storage()
            .instance()
            .set(&DataKey::BackendSigner, &backend_signer);
        env.storage()
            .persistent()
            .set(&DataKey::OpLendWasmHash, &oplend_wasm_hash);
        env.storage().instance().set(&DataKey::OperationCount, &0u32);
    }

    pub fn set_oplend_wasm_hash(env: Env, oplend_wasm_hash: BytesN<32>) {
        crate::storage::require_admin(&env);
        env.storage()
            .persistent()
            .set(&DataKey::OpLendWasmHash, &oplend_wasm_hash);
    }

    // --- Operations ---

    pub fn create_operation(
        env: Env,
        op_name: String,
        total_shares: i128,
        eur_per_shares: i128,
    ) -> Address {
        operations::create_operation(&env, op_name, total_shares, eur_per_shares)
    }

    pub fn cancel_operation(env: Env, id: u32) {
        operations::cancel_operation(&env, id);
    }

    pub fn start_operation(env: Env, id: u32) {
        operations::start_operation(&env, id);
    }

    pub fn pause_funding(env: Env, id: u32, state: bool) {
        operations::pause_funding(&env, id, state);
    }

    pub fn set_predeposits(env: Env, id: u32, state: bool) {
        operations::set_predeposits(&env, id, state);
    }

    // --- Invest ---

    pub fn invest(
        env: Env,
        user: Address,
        id: u32,
        shares_amount: i128,
        nonce: String,
        signature: BytesN<64>,
    ) {
        invest::invest(&env, user, id, shares_amount, nonce, signature);
    }

    pub fn fiat_invest(
        env: Env,
        id: u32,
        shares_amount: i128,
        user: Address,
        oplend_holder: Address,
        nonce: String,
        signature: BytesN<64>,
    ) {
        invest::fiat_invest(
            &env, id, shares_amount, user, oplend_holder, nonce, signature,
        );
    }

    pub fn gift_op_tokens(env: Env, id: u32, shares_amount: i128, user: Address) {
        invest::gift_op_tokens(&env, id, shares_amount, user);
    }

    pub fn predeposit(
        env: Env,
        user: Address,
        id: u32,
        shares_amount: i128,
        nonce: String,
        signature: BytesN<64>,
    ) {
        invest::predeposit(&env, user, id, shares_amount, nonce, signature);
    }

    pub fn claim_op_tokens(env: Env, id: u32, user: Address) {
        invest::claim_op_tokens(&env, id, user);
    }

    pub fn claim_op_tokens_batch(env: Env, id: u32, users: Vec<Address>) {
        invest::claim_op_tokens_batch(&env, id, users);
    }

    pub fn get_amount_in(env: Env, id: u32, shares_amount: i128) -> i128 {
        invest::get_amount_in(&env, id, shares_amount)
    }

    pub fn get_amount_out(env: Env, id: u32, usdc_amount: i128) -> i128 {
        invest::get_amount_out(&env, id, usdc_amount)
    }

    // --- Admin ---

    pub fn refund_user(env: Env, id: u32, user: Address) {
        admin::refund_user(&env, id, user);
    }

    pub fn batch_refund_users(env: Env, id: u32, users: Vec<Address>, len: u32) {
        admin::batch_refund_users(&env, id, users, len);
    }

    pub fn update_oracle_address(env: Env, new_oracle: Address) {
        admin::update_oracle_address(&env, new_oracle);
    }

    pub fn update_backend_signer(env: Env, new_signer: BytesN<32>) {
        admin::update_backend_signer(&env, new_signer);
    }

    pub fn blacklist(env: Env, user: Address, state: bool) {
        admin::blacklist(&env, user, state);
    }

    pub fn oplend_whitelist_user(
        env: Env,
        op_id: u32,
        user: Address,
        state: bool,
    ) {
        admin::oplend_whitelist_user(&env, op_id, user, state);
    }

    pub fn oplend_update_backend_signer(
        env: Env,
        op_id: u32,
        new_signer: BytesN<32>,
    ) {
        admin::oplend_update_backend_signer(&env, op_id, new_signer);
    }

    pub fn oplend_admin_burn(env: Env, op_id: u32, user: Address, value: i128) {
        admin::oplend_admin_burn(&env, op_id, user, value);
    }

    pub fn withdraw_usdc(env: Env, id: u32, destination: Address) {
        admin::withdraw_usdc(&env, id, destination);
    }

    pub fn transfer_ownership(env: Env, new_admin: Address) {
        admin::transfer_ownership(&env, new_admin);
    }

    // --- Getters ---

    pub fn usdc(env: Env) -> Address {
        getters::usdc(&env)
    }

    pub fn operation_count(env: Env) -> u32 {
        getters::operation_count(&env)
    }

    pub fn operations(env: Env, id: u32) -> Operation {
        getters::operations(&env, id)
    }

    pub fn get_operation(env: Env, id: u32) -> Operation {
        getters::operations(&env, id)
    }

    pub fn is_operation_finished(env: Env, id: u32) -> bool {
        getters::is_operation_finished(&env, id)
    }

    pub fn funding_progress(env: Env, id: u32) -> i128 {
        getters::funding_progress(&env, id)
    }

    pub fn usdc_raised(env: Env, id: u32) -> i128 {
        getters::usdc_raised(&env, id)
    }

    pub fn funding_paused(env: Env, id: u32) -> bool {
        getters::funding_paused(&env, id)
    }

    pub fn operation_started(env: Env, id: u32) -> bool {
        getters::operation_started(&env, id)
    }

    pub fn usdc_withdrawn(env: Env, id: u32) -> bool {
        getters::usdc_withdrawn(&env, id)
    }

    pub fn operation_canceled(env: Env, id: u32) -> bool {
        getters::operation_canceled(&env, id)
    }

    pub fn usdc_raised_per_client(env: Env, id: u32, user: Address) -> i128 {
        getters::usdc_raised_per_client(&env, id, user)
    }

    pub fn predeposits(env: Env, id: u32, user: Address) -> i128 {
        getters::predeposits(&env, id, user)
    }

    pub fn gifted(env: Env, id: u32, user: Address) -> i128 {
        getters::gifted(&env, id, user)
    }

    pub fn claimable_total(env: Env, id: u32, user: Address) -> i128 {
        getters::claimable_total(&env, id, user)
    }

    pub fn predeposits_open(env: Env, id: u32) -> bool {
        getters::predeposits_open(&env, id)
    }

    pub fn blacklisted(env: Env, user: Address) -> bool {
        getters::blacklisted(&env, user)
    }
}
