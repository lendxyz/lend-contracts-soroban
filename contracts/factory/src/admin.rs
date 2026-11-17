use soroban_sdk::{panic_with_error, token, Address, BytesN, Env, String, Vec};

use crate::errors::Error;
use crate::events;
use crate::oracle::{set_oracle, set_usdc_oracle};
use crate::storage as st;
use crate::types::{DataKey, OpData, OpLendClient, F_STARTED, F_WITHDRAWN};

fn refund_one(
    e: &Env,
    id: u32,
    op: &mut OpData,
    oplend: &OpLendClient,
    usdc: &token::Client,
    user: Address,
    withdrawn: i128,
) {
    let mut pos = st::read_position(e, id, &user);
    let invested = pos.invested;
    let oplend_balance = oplend.balance(&user);

    if invested == 0 {
        panic_with_error!(e, Error::NoParticipation);
    }
    if oplend_balance == 0 {
        panic_with_error!(e, Error::NoOpLendBalance);
    }
    // USDC of every operation sits in one balance: refunding more than this
    // operation still holds would pay out another operation's escrow.
    if invested > op.usdc_raised - withdrawn {
        panic_with_error!(e, Error::InsufficientEscrow);
    }

    op.funding_progress -= oplend_balance;
    op.usdc_raised -= invested;
    pos.invested = 0;
    st::write_position(e, id, &user, &pos);

    oplend.burn(&user, &oplend_balance);
    usdc.transfer(&e.current_contract_address(), &user, &invested);

    events::Refunded {
        investor: user,
        operation_id: id,
        usdc_amount: invested,
        shares_refunded: oplend_balance,
    }
    .publish(e);
}

pub fn refund_user(e: &Env, id: u32, user: Address) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut op = st::read_op(e, id);
    let oplend = OpLendClient::new(e, &op.op_token);
    let withdrawn = st::read_usdc_withdrawn(e, id);
    refund_one(
        e,
        id,
        &mut op,
        &oplend,
        &st::usdc_client(e),
        user,
        withdrawn,
    );
    st::write_op(e, id, &op);
}

pub fn batch_refund_users(e: &Env, id: u32, users: Vec<Address>) {
    st::require_admin(e);
    st::bump_instance(e);

    let mut op = st::read_op(e, id);
    let oplend = OpLendClient::new(e, &op.op_token);
    let usdc = st::usdc_client(e);
    let withdrawn = st::read_usdc_withdrawn(e, id);
    for user in users.iter() {
        refund_one(e, id, &mut op, &oplend, &usdc, user, withdrawn);
    }
    st::write_op(e, id, &op);
}

/// Mirrors an off-chain euro refund: burns `shares_amount` of `user`'s
/// fiat-bought shares from `oplend_holder` (where they are custodied, normally
/// the op-lend wallet) and puts them back on sale. Capped at what `user`
/// bought through `fiat_invest`, so it cannot unwind USDC purchases.
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
    let mut pos = st::read_position(e, id, &user);
    if shares_amount > pos.fiat_shares {
        panic_with_error!(e, Error::InsufficientFiatShares);
    }

    pos.fiat_shares -= shares_amount;
    st::write_position(e, id, &user, &pos);
    op.funding_progress -= shares_amount;
    st::write_op(e, id, &op);

    OpLendClient::new(e, &op.op_token).burn(&oplend_holder, &shares_amount);

    events::Refunded {
        investor: user.clone(),
        operation_id: id,
        usdc_amount: 0,
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
    if amount <= 0 {
        panic_with_error!(e, Error::NothingToWithdraw);
    }

    st::write_usdc_withdrawn(e, id, op.usdc_raised);
    if !op.flag(F_WITHDRAWN) {
        op.set_flag(F_WITHDRAWN, true);
        st::write_op(e, id, &op);
    }

    st::usdc_client(e).transfer(
        &e.current_contract_address(),
        &destination,
        &amount,
    );

    events::FundingWithdrawn {
        operation_id: id,
        usdc_amount: amount,
    }
    .publish(e);
}

pub fn transfer_ownership(e: &Env, new_admin: Address) {
    st::require_admin(e);
    st::bump_instance(e);
    e.storage().instance().set(&DataKey::Admin, &new_admin);
}
