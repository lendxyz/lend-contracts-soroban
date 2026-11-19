use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    OperationNotFound = 2,
    OperationFinished = 3,
    OperationNotStarted = 4,
    OperationCanceled = 5,
    OperationPaused = 6,
    TooManyShares = 7,
    InvalidShares = 8,
    InvalidAmount = 9,
    Blacklisted = 10,
    NonceAlreadyUsed = 11,
    NothingToWithdraw = 12,
    SelfTargetNotAllowed = 13,
    OracleNoPrice = 14,
    OracleInvalidPrice = 15,
    OracleStalePrice = 16,
    InsufficientEscrow = 17,
    InvalidOperationId = 18,
    InvalidPrice = 19,
    RaiseOverflow = 20,
    ZeroAddressNotAllowed = 21,
    StateUnchanged = 22,
    AmountTooSmall = 23,
    UsdcOffPeg = 24,
    InsufficientFiatShares = 25,
    InsufficientShareBalance = 26,
}
