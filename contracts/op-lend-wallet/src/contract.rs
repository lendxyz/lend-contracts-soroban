use soroban_sdk::{
    contract, contractimpl, panic_with_error, Address, BytesN, Env, String, Vec,
};

use crate::crypto::{
    build_redeem_message, consume_nonce, nonce_used, verify_backend_sig,
};
use crate::errors::Error;
use crate::events;
use crate::storage as st;
use crate::types::{OpLendClient, OpLendEntry};

const MAX_BATCH: u32 = 200;
const STRKEY_LEN: usize = 56;

fn require_contract(e: &Env, addr: &Address) {
    let mut buf = [0u8; STRKEY_LEN];
    addr.to_string().copy_into_slice(&mut buf);

    if buf[0] != b'C' {
        panic_with_error!(e, Error::NotAContract);
    }
}

fn register(e: &Env, op_id: u32, op_lend: &Address) {
    require_contract(e, op_lend);
    st::write_op_lend(e, op_id, op_lend);

    events::OpLendRegistered {
        operation_id: op_id,
        op_lend: op_lend.clone(),
    }
    .publish(e);
}

fn guard_payout(e: &Env, destination: &Address, amount: i128) {
    if amount <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }

    if *destination == e.current_contract_address() {
        panic_with_error!(e, Error::SelfTargetNotAllowed);
    }
}

fn authorize(
    e: &Env,
    op_id: u32,
    destination: &Address,
    amount: i128,
    nonce: &String,
    signature: &BytesN<64>,
) -> Address {
    guard_payout(e, destination, amount);

    let op_lend = st::load_op_lend(e, op_id);
    consume_nonce(e, nonce);

    let msg = build_redeem_message(e, op_id, destination, amount, nonce);
    verify_backend_sig(e, &st::read_backend_signer(e), &msg, signature);

    op_lend
}

fn pay_out(e: &Env, op_lend: &Address, destination: &Address, amount: i128) {
    st::op_lend_client(e, op_lend).transfer(
        &e.current_contract_address(),
        destination,
        &amount,
    );
}

#[contract]
pub struct OpLendWallet;

#[contractimpl]
impl OpLendWallet {
    pub fn __constructor(e: Env, admin: Address, backend_signer: BytesN<32>) {
        st::write_admin(&e, &admin);
        st::write_backend_signer(&e, &backend_signer);
        st::bump_instance(&e);
    }

    // --- Admin: registry ---

    pub fn register_op_lend(e: Env, op_id: u32, op_lend: Address) {
        st::require_admin(&e);
        st::bump_instance(&e);
        register(&e, op_id, &op_lend);
    }

    pub fn register_op_lends(e: Env, entries: Vec<OpLendEntry>) {
        st::require_admin(&e);
        st::bump_instance(&e);

        let len = entries.len();
        if len == 0 {
            panic_with_error!(&e, Error::EmptyBatch);
        }
        if len > MAX_BATCH {
            panic_with_error!(&e, Error::BatchTooLarge);
        }

        for entry in entries.iter() {
            register(&e, entry.op_id, &entry.op_lend);
        }
    }

    // --- Admin: config ---

    pub fn set_admin(e: Env, new_admin: Address) {
        let admin = st::require_admin(&e);
        st::bump_instance(&e);
        st::write_admin(&e, &new_admin);

        events::AdminUpdated { admin, new_admin }.publish(&e);
    }

    pub fn update_backend_signer(e: Env, new_signer: BytesN<32>) {
        st::require_admin(&e);
        st::bump_instance(&e);
        st::write_backend_signer(&e, &new_signer);

        events::BackendSignerUpdated { new_signer }.publish(&e);
    }

    pub fn upgrade(e: Env, new_wasm_hash: BytesN<32>) {
        st::require_admin(&e);
        st::bump_instance(&e);
        e.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    // --- Admin: redeem ---

    pub fn admin_redeem(
        e: Env,
        op_id: u32,
        destination: Address,
        amount: i128,
    ) {
        st::require_admin(&e);
        st::bump_instance(&e);
        guard_payout(&e, &destination, amount);

        let op_lend = st::load_op_lend(&e, op_id);
        pay_out(&e, &op_lend, &destination, amount);

        events::AdminRedeemed {
            destination,
            operation_id: op_id,
            amount,
        }
        .publish(&e);
    }

    // --- User redeem functions ---

    pub fn redeem(
        e: Env,
        op_id: u32,
        destination: Address,
        amount: i128,
        nonce: String,
        signature: BytesN<64>,
    ) {
        st::bump_instance(&e);

        let op_lend =
            authorize(&e, op_id, &destination, amount, &nonce, &signature);
        pay_out(&e, &op_lend, &destination, amount);

        events::Redeemed {
            destination,
            operation_id: op_id,
            amount,
        }
        .publish(&e);
    }

    pub fn whitelist_and_redeem(
        e: Env,
        op_id: u32,
        destination: Address,
        amount: i128,
        t_nonce: String,
        t_signature: BytesN<64>,
        r_nonce: String,
        r_signature: BytesN<64>,
    ) {
        st::bump_instance(&e);

        let op_lend =
            authorize(&e, op_id, &destination, amount, &r_nonce, &r_signature);

        OpLendClient::new(&e, &op_lend).whitelist_user(
            &destination,
            &t_nonce,
            &t_signature,
        );
        pay_out(&e, &op_lend, &destination, amount);

        events::Redeemed {
            destination,
            operation_id: op_id,
            amount,
        }
        .publish(&e);
    }

    // --- Getters ---

    pub fn admin(e: Env) -> Address {
        st::read_admin(&e)
    }

    pub fn backend_signer(e: Env) -> BytesN<32> {
        st::read_backend_signer(&e)
    }

    pub fn op_lend(e: Env, op_id: u32) -> Address {
        st::read_op_lend(&e, op_id)
    }

    pub fn op_lend_balance(e: Env, op_id: u32) -> i128 {
        st::op_lend_client(&e, &st::read_op_lend(&e, op_id))
            .balance(&e.current_contract_address())
    }

    pub fn nonce_used(e: Env, nonce: String) -> bool {
        nonce_used(&e, &nonce)
    }
}
