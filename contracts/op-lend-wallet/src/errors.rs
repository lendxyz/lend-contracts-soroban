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
    // 7 (`BatchTooLarge`) retired with `MAX_BATCH`: an oversized batch already
    // fails on the network's resource limits. Do not reuse the code.
    SelfTargetNotAllowed = 8,
}
