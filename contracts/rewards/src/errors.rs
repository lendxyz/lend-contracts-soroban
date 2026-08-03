use soroban_sdk::contracterror;

/// Typed contract errors, surfaced to callers as `Error(Contract, #n)`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    RootAlreadySet = 2,
    InvalidClaimBalance = 3,
    AlreadyClaimed = 4,
    InvalidProof = 5,
    RewardTokenNotWithdrawable = 6,
}
