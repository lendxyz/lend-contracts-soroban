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

/// The one factory function the wallet calls.
#[allow(dead_code)]
#[contractclient(name = "FactoryClient")]
pub trait FactoryInterface {
    fn backend_signer(env: Env) -> BytesN<32>;
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
    /// Factory the backend signer is read from. Replaces the stored
    /// `BackendSigner` of earlier versions (left behind, unread, on upgrade).
    Factory,
    OpLend(u32),
    UsedNonce(String),
}
