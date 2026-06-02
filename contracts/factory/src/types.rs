use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, String};

/// Subset of the op-lend token interface the factory calls cross-contract.
#[contractclient(name = "OpLendClient")]
#[allow(dead_code)]
pub trait OpLendInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn admin_burn(env: Env, user: Address, amount: i128);
    fn whitelist_user_admin(env: Env, user: Address, state: bool);
    fn update_backend_signer(env: Env, new_signer: BytesN<32>);
    fn balance(env: Env, id: Address) -> i128;
}

#[contracttype]
#[derive(Clone)]
pub struct Operation {
    pub op_token: Address,
    pub total_shares: i128,
    pub eur_per_shares: i128,
    pub op_name: String,
}

#[contracttype]
pub enum DataKey {
    Admin,
    USDC,
    Oracle,
    BackendSigner,
    OpLendWasmHash,
    OperationCount,
    Operation(u32),
    FundingProgress(u32),
    UsdcRaised(u32),
    UsdcWithdrew(u32),
    OperationStarted(u32),
    OperationCanceled(u32),
    FundingPaused(u32),
    PredepositsOpen(u32),
    Predeposits(u32, Address),
    Gifted(u32, Address),
    UserInvested(u32, Address),
    Blacklisted(Address),
    UsedNonce(String),
}
