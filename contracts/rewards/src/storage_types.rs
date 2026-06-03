use soroban_sdk::{contracttype, Address};

pub(crate) const DAY_IN_LEDGERS: u32 = 17280;
pub(crate) const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub(crate) const INSTANCE_LIFETIME_THRESHOLD: u32 =
    INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// A single epoch claim with its merkle proof (mirrors EVM `ClaimData`).
#[contracttype]
#[derive(Clone)]
pub struct ClaimData {
    pub epoch: u32,
    pub balance: i128,
    pub merkle_proof: soroban_sdk::Vec<soroban_sdk::BytesN<32>>,
}

#[contracttype]
pub enum DataKey {
    Admin,
    RewardToken,
    // opId => epoch => merkle root
    OpMerkleRoot(u32, u32),
    // opId => epoch => user => claimed
    OpClaimed(u32, u32, Address),
    // epoch => merkle root
    RefMerkleRoot(u32),
    // epoch => user => claimed
    RefClaimed(u32, Address),
}
