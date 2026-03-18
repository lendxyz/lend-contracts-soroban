#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, BytesN, Env, String, Symbol,
};

#[contracttype]
pub enum DataKey {
    Admin,
    USDC,
    Oracle,
    BackendSigner,
    OperationCount,
    Operation(u32),
    FundingProgress(u32),
    UsdcRaised(u32),
    OperationStarted(u32),
    UserInvested(u32, Address),
    UsedNonce(String),
}

#[contracttype]
pub struct Operation {
    pub op_token: Address,
    pub total_shares: u128,
    pub eur_per_shares: u128,
    pub op_name: String,
}

mod oracle {
    soroban_sdk::contractimport!(file = "./path/to/stellar_oracle.wasm");
}

mod lend_operation {
    soroban_sdk::contractimport!(file = "./target/wasm32v1-none/lend_operation_token.wasm");
}

#[contract]
pub struct LendFactory;

#[contractimpl]
impl LendFactory {
    pub fn initialize(
        env: Env,
        admin: Address,
        usdc: Address,
        oracle: Address,
        backend_signer: Address,
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
            .instance()
            .set(&DataKey::OperationCount, &0u32);
    }

    pub fn create_operation(
        env: Env,
        op_name: String,
        total_shares: u128,
        eur_per_shares: u128,
        op_token_wasm_hash: BytesN<32>,
    ) -> Address {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let mut op_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::OperationCount)
            .unwrap();
        op_count += 1;
        env.storage()
            .instance()
            .set(&DataKey::OperationCount, &op_count);

        let deployer = env.deployer().with_current_contract(op_count.into());
        let op_token_address = deployer.deploy(op_token_wasm_hash);

        let operation = Operation {
            op_token: op_token_address.clone(),
            total_shares,
            eur_per_shares,
            op_name,
        };
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
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::OperationStarted(id), &true);
    }

    pub fn invest(
        env: Env,
        user: Address,
        id: u32,
        shares_amount: u128,
        nonce: String,
        backend_signature: BytesN<64>,
    ) {
        user.require_auth();

        let operation: Operation = env
            .storage()
            .persistent()
            .get(&DataKey::Operation(id))
            .unwrap();
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
        assert!(
            current_progress + shares_amount <= operation.total_shares,
            "Cannot buy that many shares"
        );
        assert!(shares_amount > 0, "Not enough shares");

        let nonce_key = DataKey::UsedNonce(nonce.clone());
        assert!(!env.storage().persistent().has(&nonce_key), "Nonce used");
        env.storage().persistent().set(&nonce_key, &true);

        // --- CRYPTO VERIFICATION MOCK ---
        // Replace ecrecover[cite: 1] with Ed25519 verification.
        // let backend_signer: Address = env.storage().instance().get(&DataKey::BackendSigner).unwrap();
        // env.crypto().ed25519_verify(...);

        let oracle_addr: Address = env.storage().instance().get(&DataKey::Oracle).unwrap();
        let oracle_client = oracle::Client::new(&env, &oracle_addr);
        let oracle_price = oracle_client.get_latest_price();

        // TODO: Adjust the math based on Soroban's native types
        let shares_price_eur = (operation.eur_per_shares * shares_amount) / 1_000_000;
        let cost = shares_price_eur * oracle_price / 1_000_000;

        let usdc_addr: Address = env.storage().instance().get(&DataKey::USDC).unwrap();
        let usdc_client = token::Client::new(&env, &usdc_addr);
        usdc_client.transfer(&user, &env.current_contract_address(), &(cost as i128));

        env.storage().persistent().set(
            &DataKey::FundingProgress(id),
            &(current_progress + shares_amount),
        );

        let mut user_invested: u128 = env
            .storage()
            .persistent()
            .get(&DataKey::UserInvested(id, user.clone()))
            .unwrap_or(0);
        user_invested += cost;
        env.storage()
            .persistent()
            .set(&DataKey::UserInvested(id, user.clone()), &user_invested);

        // Mint opLend tokens to the user[cite: 1]
        let op_token_client = lend_operation::Client::new(&env, &operation.op_token);
        op_token_client.mint(&user, &(shares_amount as i128));

        env.events().publish(
            (Symbol::new(&env, "Invested"), id, user),
            (cost, shares_amount),
        );
    }
}
