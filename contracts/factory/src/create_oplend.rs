use soroban_sdk::{Address, BytesN, Env, Val, Vec};

use crate::types::DataKey;

fn get_oplend_wasm_hash(env: Env) -> BytesN<32> {
    env.storage()
        .persistent()
        .get(&DataKey::OpLendWasmHash)
        .expect("OpLend WASM hash not set")
}

pub fn deploy_oplend_from_hash(
    env: &Env,
    constructor_args: Vec<Val>,
    salt: BytesN<32>,
) -> Address {
    let wasm_hash = get_oplend_wasm_hash(env.clone());

    env.deployer()
        .with_current_contract(salt)
        .deploy_v2(wasm_hash, constructor_args)
}
