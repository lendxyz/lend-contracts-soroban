use soroban_sdk::{contractevent, Address, BytesN};

#[contractevent]
pub struct Redeemed {
    #[topic]
    pub destination: Address,
    #[topic]
    pub operation_id: u32,
    pub amount: i128,
}

#[contractevent]
pub struct AdminRedeemed {
    #[topic]
    pub destination: Address,
    #[topic]
    pub operation_id: u32,
    pub amount: i128,
}

#[contractevent]
pub struct OpLendRegistered {
    #[topic]
    pub operation_id: u32,
    pub op_lend: Address,
}

#[contractevent]
pub struct BackendSignerUpdated {
    pub new_signer: BytesN<32>,
}

#[contractevent]
pub struct AdminUpdated {
    #[topic]
    pub admin: Address,
    pub new_admin: Address,
}
