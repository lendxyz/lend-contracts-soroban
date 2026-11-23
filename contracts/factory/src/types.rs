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

/// The stablecoins an operation is funded with on-chain: USDC through
/// `invest`, EURC through `invest_eur`. Each has its own escrow and records,
/// always in its own token's base units.
#[contracttype(export = false)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Coin {
    Usdc,
    Eurc,
}

impl Coin {
    pub const ALL: [Coin; 2] = [Coin::Usdc, Coin::Eurc];
}

// `OpData::flags` bits.
pub const F_STARTED: u32 = 1 << 0;
pub const F_CANCELED: u32 = 1 << 1;
pub const F_PAUSED: u32 = 1 << 2;

/// One coin's escrow for an operation, in that coin's base units.
///
/// Invariant: `raised >= withdrawn`. Refunds take at most
/// [`Escrow::available`] off `raised`, and a withdrawal sets `withdrawn` to
/// `raised`.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct Escrow {
    /// Paid in, net of refunds.
    pub raised: i128,
    /// Cumulative amount sent out by `withdraw_funds`.
    pub withdrawn: i128,
}

impl Escrow {
    pub const ZERO: Escrow = Escrow {
        raised: 0,
        withdrawn: 0,
    };

    /// Still held by the factory for this operation: what a refund or the
    /// next withdrawal can take. Never negative (see the invariant).
    pub fn available(&self) -> i128 {
        self.raised - self.withdrawn
    }
}

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
    pub usdc: Escrow,
    pub eurc: Escrow,
    /// Bit set of `F_*`; three booleans as three map fields would be
    /// rewritten on every invest.
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

    pub fn escrow(&self, coin: Coin) -> &Escrow {
        match coin {
            Coin::Usdc => &self.usdc,
            Coin::Eurc => &self.eurc,
        }
    }

    pub fn escrow_mut(&mut self, coin: Coin) -> &mut Escrow {
        match coin {
            Coin::Usdc => &mut self.usdc,
            Coin::Eurc => &mut self.eurc,
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

/// Per-(operation, user) accounting, in a single ledger entry: what the user
/// paid in with each coin, in that coin's base units. A record, not reduced by
/// refunds.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct Position {
    pub usdc_invested: i128,
    pub eurc_invested: i128,
}

impl Position {
    pub const ZERO: Position = Position {
        usdc_invested: 0,
        eurc_invested: 0,
    };

    pub fn invested(&self, coin: Coin) -> i128 {
        match coin {
            Coin::Usdc => self.usdc_invested,
            Coin::Eurc => self.eurc_invested,
        }
    }

    pub fn invested_mut(&mut self, coin: Coin) -> &mut i128 {
        match coin {
            Coin::Usdc => &mut self.usdc_invested,
            Coin::Eurc => &mut self.eurc_invested,
        }
    }
}

#[contracttype(export = false)]
pub enum DataKey {
    Admin,
    BackendSigner,
    OpLendWasmHash,
    OperationCount,
    /// A coin's token and its cached decimals, written by the constructor.
    Token(Coin),
    TokenDecimals(Coin),
    /// EUR/USD forex feed, and its cached decimals.
    Oracle,
    OracleDecimals,
    /// Feed pricing USDC and EURC in USD for the depeg checks, and its cached
    /// decimals.
    StablecoinOracle,
    StablecoinOracleDecimals,
    Op(u32),
    OpName(u32),
    Position(u32, Address),
    Blacklisted(Address),
    UsedNonce(String),
}
