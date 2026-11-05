use soroban_sdk::{panic_with_error, token, Address, BytesN, Env, Vec};

use crate::errors::Error;
use crate::events;
use crate::oracle::set_oracle;
use crate::storage as st;
use crate::types::{
    DataKey, EurcEscrow, EurcPosition, OpData, OpLendClient, F_STARTED,
    F_WITHDRAWN,
};

/// One operation's escrow (USDC and EURC), read once and written once per
/// refund call however many users it covers.
struct Refunds<'a> {
    id: u32,
    op: OpData,
    usdc_withdrawn: i128,
    eurc: EurcEscrow,
    eurc_raised_before: i128,
    oplend: OpLendClient<'a>,
    usdc: token::Client<'a>,
}

impl<'a> Refunds<'a> {
    fn load(e: &'a Env, id: u32) -> Self {
        let op = st::read_op(e, id);
        let eurc = st::read_eurc_escrow(e, id);
        Refunds {
            id,
            usdc_withdrawn: st::read_usdc_withdrawn(e, id, &op),
            eurc_raised_before: eurc.raised,
            eurc,
            oplend: OpLendClient::new(e, &op.op_token),
            usdc: st::usdc_client(e),
            op,
        }
    }

    /// Burns the user's op-lend and returns everything they paid in, in the
    /// token they paid it in: USDC from `invest`/`predeposit`/`gift_op_tokens`,
    /// EURC from `invest_eur`, or both.
    fn refund(&mut self, e: &Env, user: Address) {
        let id = self.id;
        let mut pos = st::read_position(e, id, &user);
        let eurc_pos = st::read_eurc_position(e, id, &user);
        let invested = pos.invested;
        let eurc_invested = eurc_pos.invested;
        let oplend_balance = self.oplend.balance(&user);

        if invested == 0 && eurc_invested == 0 {
            panic_with_error!(e, Error::NoParticipation);
        }
        if oplend_balance == 0 {
            panic_with_error!(e, Error::NoOpLendBalance);
        }
        // Each token of every operation sits in one balance: refunding more
        // than this operation still holds would pay out another operation's
        // escrow.
        if invested > self.op.usdc_raised - self.usdc_withdrawn
            || eurc_invested > self.eurc.raised - self.eurc.withdrawn
        {
            panic_with_error!(e, Error::InsufficientEscrow);
        }

        self.op.funding_progress -= oplend_balance;
        self.op.usdc_raised -= invested;
        self.eurc.raised -= eurc_invested;
        if invested > 0 {
            pos.invested = 0;
            st::write_position(e, id, &user, &pos);
        }
        if eurc_invested > 0 {
            st::write_eurc_position(e, id, &user, &EurcPosition::ZERO);
        }

        self.oplend.admin_burn(&user, &oplend_balance);
        let this = e.current_contract_address();
        if invested > 0 {
            self.usdc.transfer(&this, &user, &invested);
        }
        if eurc_invested > 0 {
            st::eurc_client(e).transfer(&this, &user, &eurc_invested);
        }

        events::Refunded {
            investor: user,
            operation_id: id,
            // EURC counts at the USD value its `Invested` events reported.
            usdc_amount: invested + eurc_pos.invested_usd,
            shares_refunded: oplend_balance,
        }
        .publish(e);
    }

    fn store(&self, e: &Env) {
        st::write_op(e, self.id, &self.op);
        if self.eurc.raised != self.eurc_raised_before {
            st::write_eurc_escrow(e, self.id, &self.eurc);
        }
    }
}

pub fn refund_user(e: &Env, id: u32, user: Address) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut refunds = Refunds::load(e, id);
    refunds.refund(e, user);
    refunds.store(e);
}

pub fn batch_refund_users(e: &Env, id: u32, users: Vec<Address>, len: u32) {
    st::require_admin(e);
    st::bump_instance(e);
    if len == 0 || len > users.len() {
        panic_with_error!(e, Error::InvalidBatchLen);
    }

    let mut refunds = Refunds::load(e, id);
    for user in users.iter().take(len as usize) {
        refunds.refund(e, user);
    }
    refunds.store(e);
}

pub fn update_oracle_address(e: &Env, new_oracle: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    set_oracle(e, &new_oracle);
}

pub fn update_backend_signer(e: &Env, new_signer: BytesN<32>) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage()
        .instance()
        .set(&DataKey::BackendSigner, &new_signer);
}

pub fn blacklist(e: &Env, user: Address, state: bool) {
    st::require_admin(e);
    st::bump_instance(e);
    if user == e.current_contract_address() {
        panic_with_error!(e, Error::SelfTargetNotAllowed);
    }
    st::set_blacklisted(e, &user, state);
}

pub fn oplend_whitelist_user(e: &Env, op_id: u32, user: Address, state: bool) {
    st::require_admin(e);
    st::bump_instance(e);
    if user == e.current_contract_address() {
        panic_with_error!(e, Error::SelfTargetNotAllowed);
    }
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).whitelist_user_admin(&user, &state);
}

pub fn oplend_update_backend_signer(
    e: &Env,
    op_id: u32,
    new_signer: BytesN<32>,
) {
    st::require_admin(e);
    st::bump_instance(e);
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).update_backend_signer(&new_signer);
}

pub fn oplend_admin_burn(e: &Env, op_id: u32, user: Address, value: i128) {
    st::require_admin(e);
    st::bump_instance(e);
    if value <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).admin_burn(&user, &value);
}

/// Sends everything operation `id` raised since the previous withdrawal, in
/// both tokens: USDC and EURC each go to `destination` as themselves, and
/// `FundingWithdrawn` reports each amount in its own token's base units.
pub fn withdraw_usdc(e: &Env, id: u32, destination: Address) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut op = st::read_op(e, id);
    if !op.flag(F_STARTED) {
        panic_with_error!(e, Error::OperationNotStarted);
    }
    let withdrawn = st::read_usdc_withdrawn(e, id, &op);
    let amount = op.usdc_raised - withdrawn;
    let mut eurc = st::read_eurc_escrow(e, id);
    let eurc_amount = eurc.raised - eurc.withdrawn;
    if amount <= 0 && eurc_amount <= 0 {
        panic_with_error!(e, Error::NothingToWithdraw);
    }

    let this = e.current_contract_address();

    // `F_WITHDRAWN` stays tied to USDC: set without a `UsdcWithdrawn` entry it
    // reads as "all USDC taken", so an EURC-only withdrawal must not touch it.
    if amount > 0 {
        st::write_usdc_withdrawn(e, id, op.usdc_raised);
        if !op.flag(F_WITHDRAWN) {
            op.set_flag(F_WITHDRAWN, true);
            st::write_op(e, id, &op);
        }
        st::usdc_client(e).transfer(&this, &destination, &amount);
    }

    if eurc_amount > 0 {
        eurc.withdrawn = eurc.raised;
        st::write_eurc_escrow(e, id, &eurc);
        st::eurc_client(e).transfer(&this, &destination, &eurc_amount);
    }

    events::FundingWithdrawn {
        operation_id: id,
        usdc_amount: amount.max(0),
        eurc_amount: eurc_amount.max(0),
    }
    .publish(e);
}

pub fn set_eurc(e: &Env, eurc: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    st::set_eurc(e, &eurc);
}

pub fn transfer_ownership(e: &Env, new_admin: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage().instance().set(&DataKey::Admin, &new_admin);
}
