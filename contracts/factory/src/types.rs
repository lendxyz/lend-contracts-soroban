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
pub const F_PREDEPOSITS: u32 = 1 << 3;
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
    /// Bit set of `F_*`; five booleans as five map fields would be rewritten
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

    pub fn raised_euro(&self) -> i128 {
        self.total_shares * self.eur_per_shares
    }
}

/// Per-(operation, user) accounting, in a single ledger entry.
#[contracttype(export = false)]
#[derive(Clone)]
pub struct Position {
    /// USDC paid in by this user.
    pub invested: i128,
    /// Shares predeposited before the operation started, awaiting claim.
    pub predeposited: i128,
    /// Shares gifted by the admin, awaiting claim.
    pub gifted: i128,
}

impl Position {
    pub const ZERO: Position = Position {
        invested: 0,
        predeposited: 0,
        gifted: 0,
    };

    pub fn claimable(&self) -> i128 {
        self.predeposited + self.gifted
    }
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
}
