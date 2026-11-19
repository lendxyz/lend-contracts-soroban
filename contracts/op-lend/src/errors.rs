use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    NegativeAmount = 2,
    SupplyCapExceeded = 3,
    InsufficientBalance = 4,
    InsufficientAllowance = 5,
    NotWhitelisted = 6,
    NonceAlreadyUsed = 7,
    BadAllowanceExpiration = 8,
    NotAdmin = 9,
}
