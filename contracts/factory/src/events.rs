use soroban_sdk::{contractevent, Address, String};

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

/// `asset` is the token paid with: USDC for `invest`, EURC for `invest_eur`,
/// and the zero contract id for `fiat_invest`, settled off-chain.
/// `usdc_amount` is in USDC base units: the USDC paid for `invest`, and for
/// `invest_eur` / `fiat_invest` the USD value of the purchase at the current
/// EUR/USD rate.
#[contractevent]
pub struct Invested {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    #[topic]
    pub asset: Address,
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

/// `usdc_amount` and `eurc_amount` are the USDC and EURC sent back, each in
/// its own token's base units: refunds are admin-decided amounts, not a
/// reversal of `Invested`, so no oracle conversion applies.
#[contractevent]
pub struct Refunded {
    #[topic]
    pub investor: Address,
    #[topic]
    pub operation_id: u32,
    pub usdc_amount: i128,
    pub eurc_amount: i128,
    pub shares_refunded: i128,
}

/// Emitted with `Refunded` (which carries zero USDC and EURC) when a euro
/// purchase is refunded off-chain and mirrored with `fiat_refund`.
#[contractevent]
pub struct RefundedFiat {
    #[topic]
    pub investor: Address,
    #[topic]
    pub oplend_holder: Address,
    #[topic]
    pub operation_id: u32,
    pub shares_refunded: i128,
}

#[contractevent]
pub struct FundingWithdrawn {
    #[topic]
    pub operation_id: u32,
    pub usdc_amount: i128,
    pub eurc_amount: i128,
}

#[contractevent]
pub struct NonceVoided {
    #[topic]
    pub nonce: String,
}
