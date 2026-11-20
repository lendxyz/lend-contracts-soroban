use common::{build_void_nonce_message, verify_backend_sig};
use soroban_sdk::{
    contract, contractimpl, panic_with_error, Address, BytesN, Env, String, Vec,
};

use crate::crypto::{build_redeem_message, consume_nonce, nonce_used};
use crate::errors::Error;
use crate::events;
use crate::storage as st;
use crate::types::{FactoryClient, OpLendClient};

/// Fails unless `factory` serves a backend signer, i.e. is a factory the
/// wallet can read from (an account or an unrelated contract traps here).
fn require_factory(e: &Env, factory: &Address) {
    FactoryClient::new(e, factory).backend_signer();
}

/// Records operation `op_id`'s share token as the factory has it, rather than
/// an address passed in: a release can only ever pay out the token the factory
/// created for that operation. Fails if the factory has no such operation.
fn register(e: &Env, factory: &FactoryClient, op_id: u32) {
    let op_lend = factory.get_operation(&op_id).op_token;
    st::write_op_lend(e, op_id, &op_lend);

    events::OpLendRegistered {
        operation_id: op_id,
        op_lend,
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
    pub fn __constructor(e: Env, admin: Address, factory: Address) {
        require_factory(&e, &factory);
        st::write_admin(&e, &admin);
        st::write_factory(&e, &factory);
        st::bump_instance(&e);
    }

    // --- Admin: registry ---

    /// Maps `op_id` to the share token the factory recorded for it.
    pub fn register_op_lend(e: Env, op_id: u32) {
        st::require_admin(&e);
        st::bump_instance(&e);
        let factory = FactoryClient::new(&e, &st::read_factory(&e));
        register(&e, &factory, op_id);
    }

    pub fn register_op_lends(e: Env, op_ids: Vec<u32>) {
        st::require_admin(&e);
        st::bump_instance(&e);

        if op_ids.is_empty() {
            panic_with_error!(&e, Error::EmptyBatch);
        }

        let factory = FactoryClient::new(&e, &st::read_factory(&e));
        for op_id in op_ids.iter() {
            register(&e, &factory, op_id);
        }
    }

    // --- Admin: config ---

    pub fn set_admin(e: Env, new_admin: Address) {
        let admin = st::require_admin(&e);
        st::bump_instance(&e);
        st::write_admin(&e, &new_admin);

        events::AdminUpdated { admin, new_admin }.publish(&e);
    }

    /// Points a wallet upgraded from the stored-signer version at the factory
    /// whose `backend_signer` authorizes redeems, and deletes the signer that
    /// version stored. One-shot: once a factory is recorded it is fixed, so
    /// this fails with `FactoryAlreadySet`. The constructor records one, so a
    /// freshly deployed wallet never needs it.
    pub fn set_factory(e: Env, factory: Address) {
        st::require_admin(&e);
        if st::has_factory(&e) {
            panic_with_error!(&e, Error::FactoryAlreadySet);
        }
        st::bump_instance(&e);
        require_factory(&e, &factory);
        st::write_factory(&e, &factory);
        st::remove_legacy_backend_signer(&e);

        events::FactoryUpdated { factory }.publish(&e);
    }

    pub fn upgrade(e: Env, new_wasm_hash: BytesN<32>) {
        st::require_admin(&e);
        e.deployer().update_current_contract_wasm(new_wasm_hash);
        // After the swap: instance TTL extension also covers the code entry,
        // so this keeps the *new* wasm alive, not the one just replaced.
        st::bump_instance(&e);
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

    /// Burns an unspent redeem `nonce` so the signature carrying it can never
    /// be used. With a `signature`, authorized by the backend signing the void
    /// message (not the original signature, which binds fields this call does
    /// not take); without one, by the admin's auth.
    pub fn void_nonce(e: Env, nonce: String, signature: Option<BytesN<64>>) {
        match signature {
            Some(signature) => {
                let msg = build_void_nonce_message(&e, &nonce);
                let signer = st::read_backend_signer(&e);
                verify_backend_sig(&e, &signer, &msg, &signature);
            }
            None => {
                st::require_admin(&e);
            }
        }
        st::bump_instance(&e);
        consume_nonce(&e, &nonce);

        events::NonceVoided { nonce }.publish(&e);
    }

    // --- Getters ---

    pub fn admin(e: Env) -> Address {
        st::read_admin(&e)
    }

    pub fn backend_signer(e: Env) -> BytesN<32> {
        st::read_backend_signer(&e)
    }

    pub fn factory(e: Env) -> Address {
        st::read_factory(&e)
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
