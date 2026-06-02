use soroban_sdk::{contractevent, Address};

#[contractevent]
pub struct OperationCreated {
    #[topic]
    pub op_token: Address,
    #[topic]
    pub operation_id: u32,
    pub total_shares: i128,
}

#[contractevent]
pub struct OperationStarted {
    #[topic]
    pub operation_id: u32,
}

#[contractevent]
pub struct OperationCanceled {
    #[topic]
    pub operation_id: u32,
}

#[contractevent]
pub struct OperationPaused {
    #[topic]
    pub operation_id: u32,
}

#[contractevent]
pub struct OperationResumed {
    #[topic]
    pub operation_id: u32,
}

#[contractevent]
pub struct OperationFinished {
    #[topic]
    pub operation_id: u32,
    pub amount_raised_euro: i128,
}

#[contractevent]
pub struct PredepositsOpen {
    #[topic]
    pub operation_id: u32,
}

#[contractevent]
pub struct PredepositsClosed {
    #[topic]
    pub operation_id: u32,
}

#[contractevent]
pub struct Invested {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    pub usdc_amount: i128,
    pub shares_bought: i128,
}

#[contractevent]
pub struct InvestedFiat {
    #[topic]
    pub investor: Address,
    #[topic]
    pub oplend_destination: Address,
    #[topic]
    pub operation_id: u32,
    pub shares_bought: i128,
}

#[contractevent]
pub struct Gifted {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    pub usdc_amount: i128,
    pub shares_bought: i128,
}

#[contractevent]
pub struct Predeposit {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    pub usdc_amount: i128,
    pub shares_bought: i128,
}

#[contractevent]
pub struct ClaimedOpToken {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    pub amount: i128,
}

#[contractevent]
pub struct Refunded {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    pub usdc_amount: i128,
    pub shares_refunded: i128,
}
