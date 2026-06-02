use soroban_sdk::{vec, Address, BytesN, Env, IntoVal, String, Val, Vec};

use crate::create_oplend::deploy_oplend_from_hash;
use crate::events;
use crate::storage::{operation_count, require_admin, require_op_exists};
use crate::types::{DataKey, Operation};
use crate::utils::{concat_str, u32_to_string};

pub fn create_operation(
    e: &Env,
    op_name: String,
    total_shares: i128,
    eur_per_shares: i128,
) -> Address {
    require_admin(e);

    if total_shares <= 0 {
        panic!("total_shares must be positive");
    }

    let op_count = operation_count(e) + 1;

    let name =
        concat_str(String::from_str(e, "Lend Operation - "), op_name.clone());
    let symbol =
        concat_str(String::from_str(e, "opLEND-"), u32_to_string(e, op_count));

    let backend_signer: BytesN<32> = e
        .storage()
        .instance()
        .get(&DataKey::BackendSigner)
        .expect("backend signer not set");

    // op-lend __constructor(admin, decimal, name, symbol, max_supply, backend_signer)
    let constructor_args: Vec<Val> = vec![
        e,
        e.current_contract_address().into_val(e),
        6u32.into_val(e),
        name.into_val(e),
        symbol.into_val(e),
        total_shares.into_val(e),
        backend_signer.into_val(e),
    ];

    let mut salt_arr = [0u8; 32];
    salt_arr[28..32].copy_from_slice(&op_count.to_be_bytes());
    let salt = BytesN::from_array(e, &salt_arr);

    let op_token = deploy_oplend_from_hash(e, constructor_args, salt);

    let operation = Operation {
        op_token: op_token.clone(),
        total_shares,
        eur_per_shares,
        op_name,
    };

    e.storage()
        .instance()
        .set(&DataKey::OperationCount, &op_count);
    e.storage()
        .persistent()
        .set(&DataKey::Operation(op_count), &operation);
    e.storage()
        .persistent()
        .set(&DataKey::FundingProgress(op_count), &0i128);
    e.storage()
        .persistent()
        .set(&DataKey::OperationStarted(op_count), &false);

    events::OperationCreated {
        op_token: op_token.clone(),
        operation_id: op_count,
        total_shares,
    }
    .publish(e);

    op_token
}

pub fn cancel_operation(e: &Env, id: u32) {
    require_admin(e);
    require_op_exists(e, id);
    e.storage()
        .persistent()
        .set(&DataKey::OperationCanceled(id), &true);
    events::OperationCanceled { operation_id: id }.publish(e);
}

pub fn start_operation(e: &Env, id: u32) {
    require_admin(e);
    require_op_exists(e, id);
    e.storage()
        .persistent()
        .set(&DataKey::OperationStarted(id), &true);
    events::OperationStarted { operation_id: id }.publish(e);
}

pub fn pause_funding(e: &Env, id: u32, state: bool) {
    require_admin(e);
    require_op_exists(e, id);
    e.storage()
        .persistent()
        .set(&DataKey::FundingPaused(id), &state);
    if state {
        events::OperationPaused { operation_id: id }.publish(e);
    } else {
        events::OperationResumed { operation_id: id }.publish(e);
    }
}

pub fn set_predeposits(e: &Env, id: u32, state: bool) {
    require_admin(e);
    require_op_exists(e, id);

    let current = crate::storage::predeposits_open(e, id);
    if current != state {
        e.storage()
            .persistent()
            .set(&DataKey::PredepositsOpen(id), &state);
        if state {
            events::PredepositsOpen { operation_id: id }.publish(e);
        } else {
            events::PredepositsClosed { operation_id: id }.publish(e);
        }
    }
}
