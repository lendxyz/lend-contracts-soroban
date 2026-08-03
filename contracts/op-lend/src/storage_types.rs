use soroban_sdk::{contracttype, Address, String};

pub(crate) const DAY_IN_LEDGERS: u32 = 17280;
pub(crate) const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub(crate) const INSTANCE_LIFETIME_THRESHOLD: u32 =
    INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub(crate) const ACCOUNT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub(crate) const ACCOUNT_LIFETIME_THRESHOLD: u32 =
    ACCOUNT_BUMP_AMOUNT - DAY_IN_LEDGERS;

#[derive(Clone)]
#[contracttype(export = false)]
pub struct AllowanceDataKey {
    pub from: Address,
    pub spender: Address,
}

#[contracttype]
pub struct AllowanceValue {
    pub amount: i128,
    pub expiration_ledger: u32,
}

#[contracttype(export = false)]
#[derive(Clone)]
pub struct Account {
    pub balance: i128,
    pub whitelisted: bool,
}

impl Account {
    pub const ZERO: Account = Account {
        balance: 0,
        whitelisted: false,
    };
}

#[derive(Clone)]
#[contracttype(export = false)]
pub enum DataKey {
    Allowance(AllowanceDataKey),
    Account(Address),
    Admin,
    TotalSupply,
    MaxSupply,
    BackendSigner,
    UsedNonce(String),
}
