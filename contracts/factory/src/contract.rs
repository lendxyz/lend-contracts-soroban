use soroban_sdk::{
    contract, contractimpl, token, vec, Address, BytesN, Env, IntoVal, String,
    Val, Vec,
};

use crate::{
    create_oplend,
    types::{DataKey, InvestedEvent, OpLendToken, Operation},
    utils::concat_str,
};

#[contract]
pub struct LendFactory;

#[contractimpl]
impl LendFactory {
    pub fn initialize(
        env: Env,
        admin: Address,
        usdc: Address,
        backend_signer: Address,
        oplend_wasm_hash: BytesN<32>,
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::USDC, &usdc);
        env.storage()
            .persistent()
            .set(&DataKey::OpLendWasmHash, &oplend_wasm_hash);
        env.storage()
            .instance()
            .set(&DataKey::BackendSigner, &backend_signer);
        env.storage()
            .instance()
            .set(&DataKey::OperationCount, &0u32);
    }

    pub fn set_oplend_wasm_hash(env: Env, oplend_wasm_hash: BytesN<32>) {
        let admin: Address =
            env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        env.storage()
            .persistent()
            .set(&DataKey::OpLendWasmHash, &oplend_wasm_hash);
    }

    pub fn create_operation(
        env: Env,
        op_name: String,
        total_shares: u128,
        eur_per_shares: u128,
    ) -> Address {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not initialized");

        admin.require_auth();

        let mut op_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::OperationCount)
            .unwrap_or(0);

        op_count += 1;

        let name = concat_str(
            String::from_str(&env, "Lend Operation - "),
            op_name.clone(),
        );

        // TODO: handle concatenation with op_count
        let symbol = String::from_str(&env, "opLEND");

        let constructor_args: Vec<Val> = vec![
            &env,
            env.current_contract_address().into_val(&env),
            6u32.into_val(&env),
            name.into_val(&env),
            symbol.into_val(&env),
        ];

        let mut salt_arr = [0u8; 32];
        salt_arr[28..32].copy_from_slice(&op_count.to_be_bytes());
        let salt = BytesN::from_array(&env, &salt_arr);

        let op_token_address = create_oplend::deploy_oplend_from_hash(
            &env,
            constructor_args,
            salt,
        );

        let operation = Operation {
            op_token: op_token_address.clone(),
            total_shares,
            eur_per_shares,
            op_name,
        };

        env.storage()
            .instance()
            .set(&DataKey::OperationCount, &op_count);
        env.storage()
            .persistent()
            .set(&DataKey::Operation(op_count), &operation);
        env.storage()
            .persistent()
            .set(&DataKey::FundingProgress(op_count), &0u128);
        env.storage()
            .persistent()
            .set(&DataKey::OperationStarted(op_count), &false);

        op_token_address
    }

    pub fn start_operation(env: Env, id: u32) {
        let admin: Address =
            env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::OperationStarted(id), &true);
    }

    pub fn withdraw_usdc(env: Env, id: u32) {
        let admin: Address =
            env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let o = env.storage().persistent().get(&DataKey::Operation(id));
        assert!(o.is_some(), "Operation does no exists");

        let operation: Operation = o.unwrap();
        let current_progress: u128 = env
            .storage()
            .persistent()
            .get(&DataKey::FundingProgress(id))
            .unwrap();

        assert!(
            current_progress >= operation.total_shares,
            "Operation is not finished yet"
        );

        let has_withdrew = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcWithdrew(id))
            .unwrap_or(false);

        assert!(!has_withdrew, "Already withdrew USDC");

        let usdc_raised: u128 = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcRaised(id))
            .unwrap_or(0);

        assert!(usdc_raised > 0, "Nothing to withdraw");

        let usdc_client = token::Client::new(
            &env,
            &env.storage().instance().get(&DataKey::USDC).unwrap(),
        );

        usdc_client.transfer(
            &env.current_contract_address(),
            &admin,
            &(usdc_raised as i128),
        );
    }

    // TODO: check backend signatures
    pub fn invest(
        env: Env,
        user: Address,
        id: u32,
        shares_amount: u128,
        // nonce: String,
        // backend_signature: BytesN<64>,
    ) {
        user.require_auth();

        let o: Option<Operation> =
            env.storage().persistent().get(&DataKey::Operation(id));

        assert!(o.is_some(), "Operation does no exists");

        let operation = o.unwrap();

        let is_started: bool = env
            .storage()
            .persistent()
            .get(&DataKey::OperationStarted(id))
            .unwrap_or(false);
        let current_progress: u128 = env
            .storage()
            .persistent()
            .get(&DataKey::FundingProgress(id))
            .unwrap();

        assert!(is_started, "Operation is not started");
        assert!(shares_amount > 0, "Not enough shares");
        assert!(
            current_progress + shares_amount <= operation.total_shares,
            "Cannot buy that many shares"
        );

        // let nonce_key = DataKey::UsedNonce(nonce.clone());
        // assert!(!env.storage().persistent().has(&nonce_key), "Nonce used");
        // env.storage().persistent().set(&nonce_key, &true);

        // --- CRYPTO VERIFICATION MOCK ---
        // Replace ecrecover[cite: 1] with Ed25519 verification.
        // let backend_signer: Address = env.storage().instance().get(&DataKey::BackendSigner).unwrap();
        // env.crypto().ed25519_verify(...);

        // TODO: use an oracle to convert from EUR to USD
        let cost = (operation.eur_per_shares * shares_amount) / 1_000_000;
        let usdc_client = token::Client::new(
            &env,
            &env.storage().instance().get(&DataKey::USDC).unwrap(),
        );

        usdc_client.transfer(
            &user,
            &env.current_contract_address(),
            &(cost as i128),
        );

        env.storage().persistent().set(
            &DataKey::FundingProgress(id),
            &(current_progress + shares_amount),
        );

        let mut usdc_raised: u128 = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcRaised(id))
            .unwrap_or(0);
        usdc_raised += cost;
        env.storage()
            .persistent()
            .set(&DataKey::UsdcRaised(id), &usdc_raised);

        let mut user_invested: u128 = env
            .storage()
            .persistent()
            .get(&DataKey::UserInvested(id, user.clone()))
            .unwrap_or(0);
        user_invested += cost;
        env.storage()
            .persistent()
            .set(&DataKey::UserInvested(id, user.clone()), &user_invested);

        // Mint opLend tokens to the user
        let oplend_client = OpLendToken::new(&env, &operation.op_token);
        oplend_client.mint(&user, &(shares_amount as i128));

        InvestedEvent {
            op_id: id,
            user: user.clone(),
            cost,
            shares_amount,
        }
        .publish(&env);
    }
}
