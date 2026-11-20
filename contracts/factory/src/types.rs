use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, String};

use crate::oracle::PRICE_PRECISION;

/// Subset of the op-lend token interface the factory calls cross-contract.
#[contractclient(name = "OpLendClient")]
#[allow(dead_code)]
pub trait OpLendInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn whitelist_user_admin(env: Env, user: Address, state: bool);
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
    pub eur_per_share: i128,
    pub op_name: String,
}

/// One refund in `batch_refund_users`: shares to burn from `user` and the USDC
/// and EURC to repay them, all decided by the admin.
#[contracttype]
#[derive(Clone)]
pub struct RefundEntry {
    pub user: Address,
    pub shares_to_burn: i128,
    pub usdc_to_refund: i128,
    pub eurc_to_refund: i128,
}

// `OpData::flags` bits.
pub const F_STARTED: u32 = 1 << 0;
pub const F_CANCELED: u32 = 1 << 1;
pub const F_PAUSED: u32 = 1 << 2;
/// Set on the first `withdraw_usdc` that sends USDC; the amount lives under
/// `DataKey::UsdcWithdrawn`. EURC withdrawals do not touch it.
pub const F_WITHDRAWN: u32 = 1 << 3;

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
    pub eur_per_share: i128,
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
    /// Shares and `eur_per_share` are both 6-decimal, so their product is
    /// 12-decimal; rescale it the same way `amount_in` prices shares.
    pub fn raised_euro(&self) -> i128 {
        self.total_shares * self.eur_per_share / PRICE_PRECISION
    }
}

/// Per-(operation, user) accounting, in a single ledger entry.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct Position {
    /// USDC paid in by this user. A record, not reduced by refunds.
    pub invested: i128,
}

impl Position {
    pub const ZERO: Position = Position { invested: 0 };
}

/// EURC side of an operation's escrow, raised by `invest_eur`. Kept out of
/// [`OpData`] (adding a field there would make every stored `Op` undecodable
/// after an in-place upgrade) and absent for operations that never took EURC.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct EurcEscrow {
    /// EURC paid in, net of refunds, in EURC base units.
    pub raised: i128,
    /// Cumulative EURC sent out by `withdraw_usdc`.
    pub withdrawn: i128,
}

impl EurcEscrow {
    pub const ZERO: EurcEscrow = EurcEscrow {
        raised: 0,
        withdrawn: 0,
    };
}

/// EURC side of a [`Position`], kept apart for the same layout reason.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct EurcPosition {
    /// EURC paid in by this user, in EURC base units. A record, not reduced
    /// by refunds.
    pub invested: i128,
}

impl EurcPosition {
    pub const ZERO: EurcPosition = EurcPosition { invested: 0 };
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
    /// EURC token and its cached decimals, written by the constructor.
    Eurc,
    EurcDecimals,
    EurcEscrow(u32),
    EurcPosition(u32, Address),
}
