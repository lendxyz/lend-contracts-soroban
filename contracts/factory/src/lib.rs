#![no_std]
use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, token, Address, BytesN,
    Env, IntoVal, String, Val, Vec,
};

#[contractclient(name = "OpLendToken")]
pub trait TokenInterface {
    fn initialize(env: Env, admin: Address, decimals: u32, name: String, symbol: String);
    fn mint(env: Env, to: Address, amount: i128);
}

#[contracttype]
pub enum DataKey {
    Admin,
    USDC,
    BackendSigner,
    OpLendWasmHash,
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

#[contractevent]
struct InvestedEvent {
    #[topic]
    op_id: u32,
    #[topic]
    user: Address,
    cost: u128,
    shares_amount: u128,
}

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

    pub fn get_oplend_wasm_hash(env: Env) -> BytesN<32> {
        env.storage()
            .persistent()
            .get(&DataKey::OpLendWasmHash)
            .expect("OpLend WASM hash not set")
    }

    fn deploy_oplend_from_hash(env: &Env, constructor_args: Vec<Val>) -> Address {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let salt: BytesN<32> = env.prng().gen();
        let wasm_hash = Self::get_oplend_wasm_hash(env.clone());

        let oplend_address = env
            .deployer()
            .with_current_contract(salt)
            .deploy_v2(wasm_hash, constructor_args);

        let client = OpLendToken::new(&env, &oplend_address);

        // TODO: dynamic constructor args
        let name = soroban_sdk::String::from_str(env, "Lend Operation");
        let symbol = soroban_sdk::String::from_str(env, "opLEND-X");

        client.initialize(&admin, &6, &name, &symbol);

        oplend_address
    }

    pub fn create_operation(
        env: Env,
        op_name: String,
        total_shares: u128,
        eur_per_shares: u128,
    ) -> Address {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let mut op_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::OperationCount)
            .unwrap();

        op_count += 1;

        // TODO: pass real constructor args
        let constructor_args: Vec<Val> = ().into_val(&env);
        let op_token_address = Self::deploy_oplend_from_hash(&env, constructor_args);

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
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::OperationStarted(id), &true);
    }

    // TODO: check backend signatures
    pub fn invest(
        env: Env,
        user: Address,
        id: u32,
        shares_amount: u128,
        nonce: String,
        // backend_signature: BytesN<64>,
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

        // TODO: use an oracle to convert from EUR to USD
        let cost = (operation.eur_per_shares * shares_amount) / 1_000_000;

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

mod test;
