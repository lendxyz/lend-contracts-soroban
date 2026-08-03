use soroban_sdk::{contracttype, Address, BytesN, Vec};

pub(crate) const DAY_IN_LEDGERS: u32 = 17280;
pub(crate) const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub(crate) const INSTANCE_LIFETIME_THRESHOLD: u32 =
    INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub(crate) const CLAIM_BUMP_AMOUNT: u32 = 180 * DAY_IN_LEDGERS;
pub(crate) const CLAIM_LIFETIME_THRESHOLD: u32 =
    CLAIM_BUMP_AMOUNT - 7 * DAY_IN_LEDGERS;

#[contracttype]
#[derive(Clone)]
pub struct ClaimData {
    pub epoch: u32,
    pub balance: i128,
    pub merkle_proof: Vec<BytesN<32>>,
}

#[contracttype(export = false)]
pub enum DataKey {
    Admin,
    RewardToken,
    // opId => epoch => merkle root
    OpMerkleRoot(u32, u32),
    // opId => epoch => user => claimed
    OpClaimed(u32, u32, Address),
}
