use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    OperationNotRegistered = 2,
    // 3 (`NotAContract`) retired: registry tokens come from the factory and a
    // factory is checked by calling it. Do not reuse the code.
    InvalidAmount = 4,
    NonceAlreadyUsed = 5,
    EmptyBatch = 6,
    BatchTooLarge = 7,
    SelfTargetNotAllowed = 8,
}
