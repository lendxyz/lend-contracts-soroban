use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    OperationNotRegistered = 2,
    NotAContract = 3,
    InvalidAmount = 4,
    NonceAlreadyUsed = 5,
    EmptyBatch = 6,
    BatchTooLarge = 7,
    SelfTargetNotAllowed = 8,
}
