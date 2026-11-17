use soroban_sdk::{
    contract, contractimpl, panic_with_error, Address, BytesN, Env, String, Vec,
};

use crate::errors::Error;
use crate::types::{DataKey, Operation, RefundEntry};
use crate::{admin, getters, invest, operations, oracle, storage as st};

#[contract]
pub struct LendFactory;

#[contractimpl]
impl LendFactory {
    /// Runs once, inside the deploy transaction, so no one can front-run the
    /// setup (the old `initialize` left a window between deploy and init).
    ///
    /// `first_op_id` is the id the first `create_operation` gets (ids then go
    /// up by one), so a redeployed factory can continue a previous one's
    /// numbering. `operation_count` reports the last id issued:
    /// `first_op_id - 1` until an operation exists.
    pub fn __constructor(
        env: Env,
        admin: Address,
        usdc: Address,
        oracle: Address,
        usdc_oracle: Address,
        backend_signer: BytesN<32>,
        oplend_wasm_hash: BytesN<32>,
        first_op_id: u32,
    ) {
        if first_op_id == 0 {
            panic_with_error!(&env, Error::InvalidOperationId);
        }

        let instance = env.storage().instance();
        instance.set(&DataKey::Admin, &admin);
        instance.set(&DataKey::BackendSigner, &backend_signer);
        st::write_oplend_wasm_hash(&env, &oplend_wasm_hash);

        st::set_operation_count(&env, first_op_id - 1);
        st::set_usdc(&env, &usdc);
        oracle::set_oracle(&env, &oracle);
        oracle::set_usdc_oracle(&env, &usdc_oracle);
        st::bump_instance(&env);
    }

    pub fn set_oplend_wasm_hash(env: Env, oplend_wasm_hash: BytesN<32>) {
        st::require_admin(&env);
        st::bump_instance(&env);
        st::write_oplend_wasm_hash(&env, &oplend_wasm_hash);
    }

    // --- Operations ---

    pub fn create_operation(
        env: Env,
        op_name: String,
        total_shares: i128,
        eur_per_shares: i128,
    ) -> Address {
        operations::create_operation(
            &env,
            op_name,
            total_shares,
            eur_per_shares,
        )
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
            &env,
            id,
            shares_amount,
            user,
            oplend_holder,
            nonce,
            signature,
        );
    }

    pub fn void_nonce(env: Env, nonce: String, signature: Option<BytesN<64>>) {
        invest::void_nonce(&env, nonce, signature);
    }

    pub fn get_amount_in(env: Env, id: u32, shares_amount: i128) -> i128 {
        invest::get_amount_in(&env, id, shares_amount)
    }

    pub fn get_amount_out(env: Env, id: u32, usdc_amount: i128) -> i128 {
        invest::get_amount_out(&env, id, usdc_amount)
    }

    // --- Admin ---

    /// Burns `shares_to_burn` of `user`'s shares and repays `usdc_to_refund`
    /// USDC from the operation's escrow; the admin decides both amounts.
    pub fn refund_user(
        env: Env,
        id: u32,
        user: Address,
        shares_to_burn: i128,
        usdc_to_refund: i128,
    ) {
        admin::refund_user(&env, id, user, shares_to_burn, usdc_to_refund);
    }

    pub fn batch_refund_users(env: Env, id: u32, entries: Vec<RefundEntry>) {
        admin::batch_refund_users(&env, id, entries);
    }

    pub fn fiat_refund(
        env: Env,
        id: u32,
        user: Address,
        oplend_holder: Address,
        shares_amount: i128,
    ) {
        admin::fiat_refund(&env, id, user, oplend_holder, shares_amount);
    }

    pub fn update_oracle_address(env: Env, new_oracle: Address) {
        admin::update_oracle_address(&env, new_oracle);
    }

    pub fn update_usdc_oracle_address(env: Env, new_oracle: Address) {
        admin::update_usdc_oracle_address(&env, new_oracle);
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

    pub fn oplend_void_nonce(env: Env, op_id: u32, nonce: String) {
        admin::oplend_void_nonce(&env, op_id, nonce);
    }

    /// Upgrades operation `op_id`'s op-lend token and makes `new_wasm_hash`
    /// the code later `create_operation` calls deploy from.
    pub fn oplend_upgrade(env: Env, op_id: u32, new_wasm_hash: BytesN<32>) {
        admin::oplend_upgrade(&env, op_id, new_wasm_hash);
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

    /// Swap the factory's own code, keeping the contract id and all state.
    /// Admin-only. Note that instance storage survives, so a new wasm must be
    /// able to read what the old one wrote (or migrate it itself), and the
    /// constructor does not run again to fill in new keys.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        st::require_admin(&env);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        // After the swap: instance TTL extension also covers the code entry,
        // so this keeps the *new* wasm alive, not the one just replaced.
        st::bump_instance(&env);
    }

    // --- Getters ---

    /// The backend ed25519 key. op-lend tokens and the op-lend wallet read it
    /// from here on every signature check, so `update_backend_signer` rotates
    /// it for all of them at once.
    pub fn backend_signer(env: Env) -> BytesN<32> {
        getters::backend_signer(&env)
    }

    pub fn usdc(env: Env) -> Address {
        getters::usdc(&env)
    }

    pub fn oplend_wasm_hash(env: Env) -> BytesN<32> {
        getters::oplend_wasm_hash(&env)
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

    pub fn usdc_withdrawn_amount(env: Env, id: u32) -> i128 {
        getters::usdc_withdrawn_amount(&env, id)
    }

    pub fn operation_canceled(env: Env, id: u32) -> bool {
        getters::operation_canceled(&env, id)
    }

    pub fn usdc_raised_per_client(env: Env, id: u32, user: Address) -> i128 {
        getters::usdc_raised_per_client(&env, id, user)
    }

    pub fn fiat_shares(env: Env, id: u32, user: Address) -> i128 {
        getters::fiat_shares(&env, id, user)
    }

    pub fn blacklisted(env: Env, user: Address) -> bool {
        getters::blacklisted(&env, user)
    }
}
