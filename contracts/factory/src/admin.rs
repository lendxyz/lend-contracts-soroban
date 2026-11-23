use soroban_sdk::{panic_with_error, Address, BytesN, Env, String, Vec};

use crate::errors::Error;
use crate::events;
use crate::oracle::{set_oracle, set_stablecoin_oracle};
use crate::storage as st;
use crate::types::{
    Coin, DataKey, OpData, OpLendClient, RefundEntry, F_STARTED,
};

/// One operation, read once and written once per refund call however many
/// users it covers.
struct Refunds<'a> {
    id: u32,
    op: OpData,
    oplend: OpLendClient<'a>,
    /// Pays a refund the operation's escrow cannot cover, from its own
    /// wallet; it authorized the refund call, which covers the transfer.
    admin: Address,
}

impl<'a> Refunds<'a> {
    fn load(e: &'a Env, id: u32, admin: Address) -> Self {
        let op = st::read_op(e, id);
        Refunds {
            id,
            oplend: OpLendClient::new(e, &op.op_token),
            op,
            admin,
        }
    }

    /// Burns `shares_to_burn` of the user's shares and repays `usdc_to_refund`
    /// USDC and `eurc_to_refund` EURC, all decided by the admin. They are not
    /// tied to each other (shares move between holders, euro buyers have no
    /// coin recorded), so none is derived from another or from the `invested`
    /// records, which are left as the record of what was paid in.
    fn refund(&mut self, e: &Env, entry: RefundEntry) {
        let RefundEntry {
            user,
            shares_to_burn,
            usdc_to_refund,
            eurc_to_refund,
        } = entry;
        let refunds =
            [(Coin::Usdc, usdc_to_refund), (Coin::Eurc, eurc_to_refund)];

        if shares_to_burn < 0 {
            panic_with_error!(e, Error::InvalidShares);
        }

        if refunds.iter().any(|&(_, amount)| amount < 0)
            || (shares_to_burn == 0
                && refunds.iter().all(|&(_, amount)| amount == 0))
        {
            panic_with_error!(e, Error::InvalidAmount);
        }
        if shares_to_burn > self.oplend.balance(&user) {
            panic_with_error!(e, Error::InsufficientShareBalance);
        }

        // Each coin of every operation sits in one balance: refunding more
        // than this operation still holds would pay out another operation's
        // escrow. Such a refund is paid in full from the admin's wallet
        // instead, if it holds enough; the escrow pays only when it covers the
        // whole amount, so one coin's refund never comes from both.
        let from_admin = refunds.map(|(coin, amount)| {
            if amount <= self.op.escrow(coin).available() {
                return false;
            }
            if amount > st::token_client(e, coin).balance(&self.admin) {
                panic_with_error!(e, Error::InsufficientEscrow);
            }
            true
        });

        self.op.funding_progress -= shares_to_burn;
        if shares_to_burn > 0 {
            self.oplend.burn(&user, &shares_to_burn);
        }

        let this = e.current_contract_address();
        for ((coin, amount), from_admin) in refunds.into_iter().zip(from_admin)
        {
            if amount == 0 {
                continue;
            }
            let token = st::token_client(e, coin);
            if from_admin {
                token.transfer(&self.admin, &user, &amount);
            } else {
                self.op.escrow_mut(coin).raised -= amount;
                token.transfer(&this, &user, &amount);
            }
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
    let admin = st::require_admin(e);
    st::bump_instance(e);

    let mut refunds = Refunds::load(e, id, admin);
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
    let admin = st::require_admin(e);
    st::bump_instance(e);

    let mut refunds = Refunds::load(e, id, admin);
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

pub fn update_stablecoin_oracle_address(e: &Env, new_oracle: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    set_stablecoin_oracle(e, &new_oracle);
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
/// controls, Stellar's nearest thing to EVM's `address(0)`. Tokens sent to the
/// zero contract id land in the token's ledger and can never move again.
const ZERO_ACCOUNT: &[u8; 56] =
    b"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";
const ZERO_CONTRACT: &[u8; 56] =
    b"CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";

/// Rejects payout destinations that would strand the funds: the factory itself
/// (the coins would stay in the shared balance but count as withdrawn, so no
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
/// both coins: USDC and EURC each go to `destination` as themselves, and
/// `FundingWithdrawn` reports each amount in its own coin's base units.
pub fn withdraw_funds(e: &Env, id: u32, destination: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    require_payout_destination(e, &destination);

    let mut op = st::read_op(e, id);
    if !op.flag(F_STARTED) {
        panic_with_error!(e, Error::OperationNotStarted);
    }
    let amounts = Coin::ALL.map(|coin| op.escrow(coin).available());
    if amounts.iter().all(|&amount| amount == 0) {
        panic_with_error!(e, Error::NothingToWithdraw);
    }

    for (coin, amount) in Coin::ALL.into_iter().zip(amounts) {
        if amount > 0 {
            let escrow = op.escrow_mut(coin);
            escrow.withdrawn = escrow.raised;
        }
    }
    st::write_op(e, id, &op);

    let this = e.current_contract_address();
    for (coin, amount) in Coin::ALL.into_iter().zip(amounts) {
        if amount > 0 {
            st::token_client(e, coin).transfer(&this, &destination, &amount);
        }
    }

    let [usdc_amount, eurc_amount] = amounts;
    events::FundingWithdrawn {
        operation_id: id,
        usdc_amount,
        eurc_amount,
    }
    .publish(e);
}

pub fn transfer_ownership(e: &Env, new_admin: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage().instance().set(&DataKey::Admin, &new_admin);
}
