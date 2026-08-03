use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    DecimalTooLarge = 2,
    NegativeAmount = 3,
    SupplyCapExceeded = 4,
    InsufficientBalance = 5,
    InsufficientAllowance = 6,
    NotWhitelisted = 7,
    NonceAlreadyUsed = 8,
    BadAllowanceExpiration = 9,
}
