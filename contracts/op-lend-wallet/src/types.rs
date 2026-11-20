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

/// The factory functions the wallet calls.
#[allow(dead_code)]
#[contractclient(name = "FactoryClient")]
pub trait FactoryInterface {
    fn backend_signer(env: Env) -> BytesN<32>;
    fn get_operation(env: Env, id: u32) -> Operation;
}

/// The factory's view of an operation, as returned by `get_operation`. Mirrors
/// the factory's `Operation` field for field (decoding needs them all); the
/// wallet only reads `op_token`.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct Operation {
    pub op_token: Address,
    pub total_shares: i128,
    pub eur_per_share: i128,
    pub op_name: String,
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
