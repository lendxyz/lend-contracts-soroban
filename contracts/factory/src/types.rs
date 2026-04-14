use soroban_sdk::{
    contractclient, contractevent, contracttype, Address, Env, String,
};

#[contractclient(name = "OpLendToken")]
#[allow(dead_code)]
pub trait TokenInterface {
    fn mint(env: Env, to: Address, amount: i128);
}

#[contracttype]
pub struct Operation {
    pub op_token: Address,
    pub total_shares: u128,
    pub eur_per_shares: u128,
    pub op_name: String,
}

#[contractevent]
pub struct InvestedEvent {
    #[topic]
    pub op_id: u32,
    #[topic]
    pub user: Address,
    pub cost: u128,
    pub shares_amount: u128,
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
    UsdcWithdrew(u32),
    OperationStarted(u32),
    UserInvested(u32, Address),
    UsedNonce(String),
}
