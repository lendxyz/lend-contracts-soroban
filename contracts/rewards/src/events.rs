use soroban_sdk::{contractevent, Address};

#[contractevent]
pub struct RewardTokenUpdated {
    #[topic]
    pub new_token: Address,
}

#[contractevent]
pub struct EmergencyWithdrawn {
    #[topic]
    pub token: Address,
    pub amount: i128,
}

#[contractevent]
pub struct Claimed {
    #[topic]
    pub op_id: u32,
    #[topic]
    pub user: Address,
    pub balance: i128,
}

#[contractevent]
pub struct RewardsDistributed {
    #[topic]
    pub op_id: u32,
    #[topic]
    pub epoch: u32,
    pub amount: i128,
}
