use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    // 2 (`DecimalTooLarge` / `InvalidDecimal`) retired: decimals are fixed at
    // 6, no longer a constructor argument. Do not reuse the code.
    NegativeAmount = 3,
    SupplyCapExceeded = 4,
    InsufficientBalance = 5,
    InsufficientAllowance = 6,
    NotWhitelisted = 7,
    NonceAlreadyUsed = 8,
    BadAllowanceExpiration = 9,
    NotAdmin = 10,
}
