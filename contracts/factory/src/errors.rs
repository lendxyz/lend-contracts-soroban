use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    OperationNotFound = 3,
    OperationFinished = 4,
    OperationNotStarted = 5,
    OperationAlreadyStarted = 6,
    OperationCanceled = 7,
    OperationPaused = 8,
    PredepositsClosed = 9,
    TooManyShares = 10,
    InvalidShares = 11,
    InvalidAmount = 12,
    Blacklisted = 13,
    NonceAlreadyUsed = 14,
    BatchTooLarge = 15,
    InvalidBatchLen = 16,
    OperationNotFinished = 17,
    AlreadyWithdrawn = 18,
    NoParticipation = 19,
    NoOpLendBalance = 20,
    SelfTargetNotAllowed = 21,
    OracleNoPrice = 22,
    OracleInvalidPrice = 23,
    OracleStalePrice = 24,
}
