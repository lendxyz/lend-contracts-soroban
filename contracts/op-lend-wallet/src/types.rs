use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, String};

#[allow(dead_code)]
#[contractclient(name = "OpLendClient")]
pub trait OpLendInterface {
    fn whitelist_user(
        env: Env,
        user: Address,
        nonce: String,
        signature: BytesN<64>,
    );
}

#[contracttype]
#[derive(Clone)]
pub struct OpLendEntry {
    pub op_id: u32,
    pub op_lend: Address,
}

#[contracttype(export = false)]
pub enum DataKey {
    Admin,
    BackendSigner,
    OpLend(u32),
    UsedNonce(String),
}
