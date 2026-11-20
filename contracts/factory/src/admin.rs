use soroban_sdk::{panic_with_error, token, Address, BytesN, Env, String, Vec};

use crate::errors::Error;
use crate::events;
use crate::oracle::{set_oracle, set_usdc_oracle};
use crate::storage as st;
use crate::types::{
    DataKey, EurcEscrow, OpData, OpLendClient, RefundEntry, F_STARTED,
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
            usdc_withdrawn: st::read_usdc_withdrawn(e, id),
            eurc_raised_before: eurc.raised,
            eurc,
            oplend: OpLendClient::new(e, &op.op_token),
            usdc: st::usdc_client(e),
            op,
        }
    }

    /// Burns `shares_to_burn` of the user's shares and repays `usdc_to_refund`
    /// USDC and `eurc_to_refund` EURC, all decided by the admin. They are not
    /// tied to each other (shares move between holders, euro buyers have no
    /// token recorded), so none is derived from another or from the
    /// `invested` records, which are left as the record of what was paid in.
    fn refund(&mut self, e: &Env, entry: RefundEntry) {
        let RefundEntry {
            user,
            shares_to_burn,
            usdc_to_refund,
            eurc_to_refund,
        } = entry;

        if shares_to_burn < 0 {
            panic_with_error!(e, Error::InvalidShares);
        }
        if usdc_to_refund < 0
            || eurc_to_refund < 0
            || (shares_to_burn == 0
                && usdc_to_refund == 0
                && eurc_to_refund == 0)
        {
            panic_with_error!(e, Error::InvalidAmount);
        }
        if shares_to_burn > self.oplend.balance(&user) {
            panic_with_error!(e, Error::InsufficientShareBalance);
        }
        // Each token of every operation sits in one balance: refunding more
        // than this operation still holds would pay out another operation's
        // escrow.
        if usdc_to_refund > self.op.usdc_raised - self.usdc_withdrawn
            || eurc_to_refund > self.eurc.raised - self.eurc.withdrawn
        {
            panic_with_error!(e, Error::InsufficientEscrow);
        }

        self.op.funding_progress -= shares_to_burn;
        self.op.usdc_raised -= usdc_to_refund;
        self.eurc.raised -= eurc_to_refund;

        if shares_to_burn > 0 {
            self.oplend.burn(&user, &shares_to_burn);
        }
        let this = e.current_contract_address();
        if usdc_to_refund > 0 {
            self.usdc.transfer(&this, &user, &usdc_to_refund);
        }
        if eurc_to_refund > 0 {
            st::eurc_client(e).transfer(&this, &user, &eurc_to_refund);
        }

        events::Refunded {
            investor: user,
            operation_id: self.id,
            usdc_amount: usdc_to_refund,
            eurc_amount: eurc_to_refund,
            shares_refunded: shares_to_burn,
        }
        .publish(e);
    }

    fn store(&self, e: &Env) {
        st::write_op(e, self.id, &self.op);
        // Operations that never took EURC keep no `EurcEscrow` entry.
        if self.eurc.raised != self.eurc_raised_before {
            st::write_eurc_escrow(e, self.id, &self.eurc);
        }
    }
}

pub fn refund_user(
    e: &Env,
    id: u32,
    user: Address,
    shares_to_burn: i128,
    usdc_to_refund: i128,
    eurc_to_refund: i128,
) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut refunds = Refunds::load(e, id);
    refunds.refund(
        e,
        RefundEntry {
            user,
            shares_to_burn,
            usdc_to_refund,
            eurc_to_refund,
        },
    );
    refunds.store(e);
}

pub fn batch_refund_users(e: &Env, id: u32, entries: Vec<RefundEntry>) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut refunds = Refunds::load(e, id);
    for entry in entries.iter() {
        refunds.refund(e, entry);
    }
    refunds.store(e);
}

/// Mirrors an off-chain euro refund: burns `shares_amount` shares from
/// `oplend_holder` (where euro buyers' shares are custodied, normally the
/// op-lend wallet) and puts them back on sale. Like `refund_user`, it is
/// bounded only by the holder's balance: the amount and the holder are the
/// admin's call, and `user` only labels the events.
pub fn fiat_refund(
    e: &Env,
    id: u32,
    user: Address,
    oplend_holder: Address,
    shares_amount: i128,
) {
    st::require_admin(e);
    st::bump_instance(e);
    if shares_amount <= 0 {
        panic_with_error!(e, Error::InvalidShares);
    }

    let mut op = st::read_op(e, id);
    let oplend = OpLendClient::new(e, &op.op_token);
    if shares_amount > oplend.balance(&oplend_holder) {
        panic_with_error!(e, Error::InsufficientShareBalance);
    }

    op.funding_progress -= shares_amount;
    st::write_op(e, id, &op);

    oplend.burn(&oplend_holder, &shares_amount);

    events::Refunded {
        investor: user.clone(),
        operation_id: id,
        usdc_amount: 0,
        eurc_amount: 0,
        shares_refunded: shares_amount,
    }
    .publish(e);
    events::RefundedFiat {
        investor: user,
        oplend_holder,
        operation_id: id,
        shares_refunded: shares_amount,
    }
    .publish(e);
}

pub fn update_oracle_address(e: &Env, new_oracle: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    set_oracle(e, &new_oracle);
}

pub fn update_usdc_oracle_address(e: &Env, new_oracle: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    set_usdc_oracle(e, &new_oracle);
}

/// Rotates the backend key for the factory and, since they read it from here,
/// for every op-lend token and the op-lend wallet pointed at this factory.
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

pub fn oplend_void_nonce(e: &Env, op_id: u32, nonce: String) {
    st::require_admin(e);
    st::bump_instance(e);
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).void_nonce(&nonce, &None);
}

/// Upgrades one operation's op-lend token (the factory is its admin), then
/// stores the hash for future operations. If the upgrade fails the whole call
/// reverts, so the stored hash never points at code that did not install.
pub fn oplend_upgrade(e: &Env, op_id: u32, new_wasm_hash: BytesN<32>) {
    st::require_admin(e);
    st::bump_instance(e);
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).upgrade(&new_wasm_hash);
    st::write_oplend_wasm_hash(e, &new_wasm_hash);
}

pub fn oplend_admin_burn(e: &Env, op_id: u32, user: Address, value: i128) {
    st::require_admin(e);
    st::bump_instance(e);
    if value <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }
    let op = st::read_op(e, op_id);
    OpLendClient::new(e, &op.op_token).burn(&user, &value);
}

/// Strkeys of the all-zero account key and contract id: valid addresses nobody
/// controls, Stellar's nearest thing to EVM's `address(0)`. USDC sent to the
/// zero contract id lands in the token's ledger and can never move again.
const ZERO_ACCOUNT: &[u8; 56] =
    b"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";
const ZERO_CONTRACT: &[u8; 56] =
    b"CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";

/// Rejects payout destinations that would strand the funds: the factory itself
/// (the USDC would stay in the shared balance but count as withdrawn, so no
/// operation could ever reach it again) and the zero addresses.
fn require_payout_destination(e: &Env, destination: &Address) {
    if *destination == e.current_contract_address() {
        panic_with_error!(e, Error::SelfTargetNotAllowed);
    }
    let mut strkey = [0u8; 56];
    destination.to_string().copy_into_slice(&mut strkey);
    if &strkey == ZERO_ACCOUNT || &strkey == ZERO_CONTRACT {
        panic_with_error!(e, Error::ZeroAddressNotAllowed);
    }
}

/// Sends everything operation `id` raised since the previous withdrawal, in
/// both tokens: USDC and EURC each go to `destination` as themselves, and
/// `FundingWithdrawn` reports each amount in its own token's base units.
pub fn withdraw_usdc(e: &Env, id: u32, destination: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    require_payout_destination(e, &destination);

    let mut op = st::read_op(e, id);
    if !op.flag(F_STARTED) {
        panic_with_error!(e, Error::OperationNotStarted);
    }
    let withdrawn = st::read_usdc_withdrawn(e, id);
    let amount = op.usdc_raised - withdrawn;
    let mut eurc = st::read_eurc_escrow(e, id);
    let eurc_amount = eurc.raised - eurc.withdrawn;
    if amount <= 0 && eurc_amount <= 0 {
        panic_with_error!(e, Error::NothingToWithdraw);
    }

    let this = e.current_contract_address();

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

pub fn transfer_ownership(e: &Env, new_admin: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage().instance().set(&DataKey::Admin, &new_admin);
}
