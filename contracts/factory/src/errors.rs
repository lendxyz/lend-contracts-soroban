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
    // 6 (`OperationAlreadyStarted`) retired with predeposits. Do not reuse.
    OperationCanceled = 7,
    OperationPaused = 8,
    // 9 (`PredepositsClosed`) retired with predeposits. Do not reuse.
    TooManyShares = 10,
    InvalidShares = 11,
    InvalidAmount = 12,
    Blacklisted = 13,
    NonceAlreadyUsed = 14,
    // 15 (`BatchTooLarge`) retired with `claim_op_tokens_batch`. Do not reuse.
    InvalidBatchLen = 16,
    // 17 (`OperationNotFinished`) retired: withdrawals no longer wait for the
    // end of the sale. Do not reuse the code.
    NothingToWithdraw = 18,
    NoParticipation = 19,
    NoOpLendBalance = 20,
    SelfTargetNotAllowed = 21,
    OracleNoPrice = 22,
    OracleInvalidPrice = 23,
    OracleStalePrice = 24,
    InsufficientEscrow = 25,
}
