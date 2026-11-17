use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, String};

use crate::oracle::PRICE_PRECISION;

/// Subset of the op-lend token interface the factory calls cross-contract.
#[contractclient(name = "OpLendClient")]
#[allow(dead_code)]
pub trait OpLendInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn whitelist_user_admin(env: Env, user: Address, state: bool);
    fn update_backend_signer(env: Env, new_signer: BytesN<32>);
    fn void_nonce(env: Env, nonce: String, signature: Option<BytesN<64>>);
    fn balance(env: Env, id: Address) -> i128;
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
}

/// Public view of an operation, returned by `operations` / `get_operation`.
/// Assembled on read from [`OpData`] + the operation name.
#[contracttype]
#[derive(Clone)]
pub struct Operation {
    pub op_token: Address,
    pub total_shares: i128,
    pub eur_per_shares: i128,
    pub op_name: String,
}

// `OpData::flags` bits.
pub const F_STARTED: u32 = 1 << 0;
pub const F_CANCELED: u32 = 1 << 1;
pub const F_PAUSED: u32 = 1 << 2;
/// Set on the first `withdraw_usdc`; the amount lives under
/// `DataKey::UsdcWithdrawn`.
pub const F_WITHDRAWN: u32 = 1 << 4;

/// Everything a funding path needs, in a single ledger entry.
///
/// The EVM original kept eight parallel `mapping(uint256 => ...)`s; on Soroban
/// each of those is an independent ledger entry with its own key, footprint
/// slot, TTL entry and rent. One struct means one read and one write per call.
/// The name lives under a separate key because it is metadata that no
/// state-changing path reads, and would otherwise be rewritten on every invest.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct OpData {
    pub op_token: Address,
    pub total_shares: i128,
    pub eur_per_shares: i128,
    pub funding_progress: i128,
    pub usdc_raised: i128,
    /// Bit set of `F_*`; four booleans as four map fields would be rewritten
    /// on every invest.
    pub flags: u32,
}

impl OpData {
    pub fn flag(&self, f: u32) -> bool {
        self.flags & f != 0
    }

    pub fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }

    pub fn fully_funded(&self) -> bool {
        self.funding_progress >= self.total_shares
    }

    pub fn finished(&self) -> bool {
        self.flag(F_STARTED) && self.fully_funded()
    }

    /// EUR value of the whole operation, 6-decimal like every internal amount.
    /// Shares and `eur_per_shares` are both 6-decimal, so their product is
    /// 12-decimal; rescale it the same way `amount_in` prices shares.
    pub fn raised_euro(&self) -> i128 {
        self.total_shares * self.eur_per_shares / PRICE_PRECISION
    }
}

/// Per-(operation, user) accounting, in a single ledger entry.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct Position {
    /// USDC paid in by this user.
    pub invested: i128,
    /// Shares bought with euros through `fiat_invest` and not yet refunded
    /// with `fiat_refund`: the cap on what `fiat_refund` may take back.
    pub fiat_shares: i128,
}

impl Position {
    pub const ZERO: Position = Position {
        invested: 0,
        fiat_shares: 0,
    };
}

#[contracttype(export = false)]
pub enum DataKey {
    Admin,
    Usdc,
    UsdcDecimals,
    Oracle,
    OracleDecimals,
    BackendSigner,
    OpLendWasmHash,
    OperationCount,
    Op(u32),
    OpName(u32),
    Position(u32, Address),
    Blacklisted(Address),
    UsedNonce(String),
    /// Cumulative USDC withdrawn from an operation. Kept out of [`OpData`]
    /// because adding a field there would make every already-stored `Op`
    /// entry undecodable. New variants MUST be appended, never reordered.
    UsdcWithdrawn(u32),
    /// USDC/USD oracle for the depeg check, and its cached decimals.
    UsdcOracle,
    UsdcOracleDecimals,
}
