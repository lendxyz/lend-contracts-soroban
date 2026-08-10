#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger as _},
    token::StellarAssetClient,
    Address, Bytes, BytesN, Env, String,
};

use crate::contract::{LendFactory, LendFactoryClient};
use crate::errors::Error;
use crate::oracle::{Asset, PriceData};

// Op-lend wasm, built by `cargo build -p lend-operation-token --target wasm32v1-none --release`.
mod oplend {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/lend_operation_token.wasm"
    );
}

// --- Mock Reflector oracle: EUR/USD = 1.1, 14 decimals ---
#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn lastprice(e: Env, _asset: Asset) -> Option<PriceData> {
        Some(PriceData {
            price: 110_000_000_000_000, // 1.1 * 1e14
            timestamp: e.ledger().timestamp(),
        })
    }
    pub fn decimals(_e: Env) -> u32 {
        14
    }
}

/// A USDC whose scale differs from the 7-decimal SAC the rest of the tests use.
/// The value here MUST stay 6: the point is the contrast, so that a hardcoded
/// scale anywhere in the pricing path shows up as a failure. Only `decimals()`
/// is needed — the pricing path reads it, nothing transfers.
#[contract]
pub struct MockToken6;

#[contractimpl]
impl MockToken6 {
    pub fn decimals(_e: Env) -> u32 {
        6
    }
}

const EUR_PER_SHARE: i128 = 1_000_000; // 1 EUR (6 decimals)

fn signer_key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

struct Setup<'a> {
    e: Env,
    admin: Address,
    factory: LendFactoryClient<'a>,
    factory_id: Address,
    usdc: StellarAssetClient<'a>,
    usdc_addr: Address,
}

fn setup<'a>() -> Setup<'a> {
    let e = Env::default();
    e.mock_all_auths();
    e.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&e);

    let usdc_issuer = Address::generate(&e);
    let usdc_sac = e.register_stellar_asset_contract_v2(usdc_issuer);
    let usdc_addr = usdc_sac.address();
    let usdc = StellarAssetClient::new(&e, &usdc_addr);

    let oracle_addr = e.register(MockOracle, ());

    let signer =
        BytesN::from_array(&e, &signer_key().verifying_key().to_bytes());
    let wasm_hash = e.deployer().upload_contract_wasm(oplend::WASM);

    let factory_id = e.register(LendFactory, ());
    let factory = LendFactoryClient::new(&e, &factory_id);
    factory.initialize(&admin, &usdc_addr, &oracle_addr, &signer, &wasm_hash);

    Setup {
        e,
        admin,
        factory,
        factory_id,
        usdc,
        usdc_addr,
    }
}

/// Independent reconstruction of the `ONCHAIN_INVEST` payload. Keep it naive
/// (plain appends, not the contract's buffer layout) so a byte-layout change in
/// `build_invest_message` breaks the tests instead of every backend signature.
fn sign_invest(
    e: &Env,
    factory_id: &Address,
    id: u32,
    user: &Address,
    amount: i128,
    nonce: &String,
) -> BytesN<64> {
    let mut msg = Bytes::from_slice(e, b"ONCHAIN_INVEST");
    msg.append(&Bytes::from(factory_id.to_string()));
    msg.append(&Bytes::from_slice(e, &id.to_be_bytes()));
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));
    let v: std::vec::Vec<u8> = msg.iter().collect();
    BytesN::from_array(e, &signer_key().sign(&v).to_bytes())
}

fn create_op(s: &Setup, shares: i128) -> (u32, Address) {
    let name = String::from_str(&s.e, "Alpha");
    let op_addr = s.factory.create_operation(&name, &shares, &EUR_PER_SHARE);
    (s.factory.operation_count(), op_addr)
}

#[test]
fn test_create_operation() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    assert_eq!(id, 1);

    let op = s.factory.get_operation(&id);
    assert_eq!(op.total_shares, 1000);
    assert_eq!(op.eur_per_shares, EUR_PER_SHARE);
    assert_eq!(op.op_token, op_addr);
    assert_eq!(s.factory.operation_count(), 1);
    assert!(!s.factory.operation_started(&id));
}

#[test]
fn test_get_amount_in_out() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    // 100 shares * 1 EUR * 1.1 USD = 110 six-decimal USDC, which is 1100 base
    // units on the 7-decimal SAC the tests run against.
    assert_eq!(s.factory.get_amount_in(&id, &100), 1100);
    // Exact round trip back to the shares that produced it.
    assert_eq!(s.factory.get_amount_out(&id, &1100), 100);
}

/// A factory wired to an arbitrary USDC token, for the decimal-scale tests.
fn factory_with_usdc<'a>(e: &Env, usdc: &Address) -> LendFactoryClient<'a> {
    let admin = Address::generate(e);
    let oracle_addr = e.register(MockOracle, ());
    let signer =
        BytesN::from_array(e, &signer_key().verifying_key().to_bytes());
    let wasm_hash = e.deployer().upload_contract_wasm(oplend::WASM);
    let factory = LendFactoryClient::new(e, &e.register(LendFactory, ()));
    factory.initialize(&admin, usdc, &oracle_addr, &signer, &wasm_hash);
    factory
}

/// `get_amount_in` answers "how much USDC must move to buy N shares", so its
/// result is in the token's base units — not the 6-decimal pricing space.
/// Spelled out in whole units: one whole share priced at 1 EUR, with EUR/USD =
/// 1.1, costs 1.1 USDC. On the 7-decimal SAC that is 11_000_000 base units; on
/// a 6-decimal token, 1_100_000. Both are the same 1.1 USDC.
#[test]
fn test_amount_in_is_denominated_in_usdc_base_units() {
    /// Shares are 6-decimal, so one whole share is 1e6 base units.
    const ONE_SHARE: i128 = 1_000_000;

    let s = setup();
    let (id, _) = create_op(&s, 1_000 * ONE_SHARE);
    assert_eq!(
        soroban_sdk::token::Client::new(&s.e, &s.usdc_addr).decimals(),
        7
    );
    assert_eq!(s.factory.get_amount_in(&id, &ONE_SHARE), 11_000_000);
    assert_eq!(s.factory.get_amount_out(&id, &11_000_000), ONE_SHARE);

    let e = Env::default();
    e.mock_all_auths();
    e.ledger().set_timestamp(1_000_000);
    let factory = factory_with_usdc(&e, &e.register(MockToken6, ()));
    factory.create_operation(
        &String::from_str(&e, "Alpha"),
        &(1_000 * ONE_SHARE),
        &EUR_PER_SHARE,
    );
    assert_eq!(factory.get_amount_in(&1, &ONE_SHARE), 1_100_000);
    assert_eq!(factory.get_amount_out(&1, &1_100_000), ONE_SHARE);
}

/// The USDC leg is denominated in the token's own decimals. Circle's USDC is a
/// classic asset behind a SAC and reports 7, which both mainnet and (since the
/// DummyUSDC redeploy) testnet run on — but the factory must price off whatever
/// its token declares, not a constant. Same price, same shares, two token
/// scales, amounts an order of magnitude apart: hardcoding 1e6 would have
/// underpaid every 7-decimal invest by 10x.
#[test]
fn test_amount_in_follows_usdc_decimals() {
    let s = setup();
    let usdc = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc.decimals(), 7);
    let (id, _) = create_op(&s, 1000);
    assert_eq!(s.factory.get_amount_in(&id, &100), 1100);

    // Same price and shares against a 6-decimal token: 10x smaller number, same
    // real amount.
    let e = Env::default();
    e.mock_all_auths();
    e.ledger().set_timestamp(1_000_000);
    let factory = factory_with_usdc(&e, &e.register(MockToken6, ()));
    factory.create_operation(
        &String::from_str(&e, "Alpha"),
        &1000,
        &EUR_PER_SHARE,
    );
    assert_eq!(factory.get_amount_in(&1, &100), 110);
}

#[test]
fn test_invest_happy_path() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);

    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);

    assert_eq!(s.factory.funding_progress(&id), 100);
    assert_eq!(s.factory.usdc_raised(&id), 1100);
    assert_eq!(s.factory.usdc_raised_per_client(&id, &user), 1100);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&user), 100);
    // user paid 1100 base units = 110.0 USDC at six decimals
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&user), 8_900);
    assert_eq!(usdc_tok.balance(&s.factory_id), 1100);
}

#[test]
fn test_invest_before_start_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::OperationNotStarted.into()))
    );
}

#[test]
fn test_invest_nonce_replay_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}

#[test]
fn test_blacklisted_user_cannot_invest() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    s.factory.blacklist(&user, &true);

    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::Blacklisted.into()))
    );
}

#[test]
fn test_predeposit_and_claim() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.set_predeposits(&id, &true);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);

    let nonce = String::from_str(&s.e, "p1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 50, &nonce);
    s.factory.predeposit(&user, &id, &50, &nonce, &sig);

    assert_eq!(s.factory.predeposits(&id, &user), 50);
    assert_eq!(s.factory.claimable_total(&id, &user), 50);

    s.factory.start_operation(&id);
    s.factory.claim_op_tokens(&id, &user);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&user), 50);
    assert_eq!(s.factory.predeposits(&id, &user), 0);
}

#[test]
fn test_gift_op_tokens() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.usdc.mint(&s.admin, &10_000);

    let user = Address::generate(&s.e);
    s.factory.gift_op_tokens(&id, &30, &user);

    assert_eq!(s.factory.gifted(&id, &user), 30);
    assert_eq!(s.factory.funding_progress(&id), 30);
}

#[test]
fn test_refund_user() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);

    s.factory.refund_user(&id, &user);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&user), 0);
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&user), 10_000); // fully refunded
}

#[test]
fn test_withdraw_usdc_after_finish() {
    let s = setup();
    let (id, _) = create_op(&s, 100);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    // buy all 100 shares -> finished
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert!(s.factory.is_operation_finished(&id));

    let dest = Address::generate(&s.e);
    s.factory.withdraw_usdc(&id, &dest);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&dest), 1100);
    assert!(s.factory.usdc_withdrawn(&id));
}

#[test]
fn test_canceled_blocks_invest() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    s.factory.cancel_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::OperationCanceled.into()))
    );
}

#[test]
fn test_get_operation_nonexistent() {
    let s = setup();
    assert_eq!(
        s.factory.try_get_operation(&99).err(),
        Some(Ok(Error::OperationNotFound.into()))
    );
}

/// The batch paths accumulate into one in-memory `OpData` and write it once, so
/// a per-user update that got dropped or double-counted would only show up with
/// more than one user in the batch.
#[test]
fn test_batch_refund_accumulates_across_users() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let users = [Address::generate(&s.e), Address::generate(&s.e)];
    for (i, user) in users.iter().enumerate() {
        s.usdc.mint(user, &10_000);
        let nonce = crate::utils::op_symbol(&s.e, i as u32);
        let shares = 100 + i as i128;
        let sig = sign_invest(&s.e, &s.factory_id, id, user, shares, &nonce);
        s.factory.invest(user, &id, &shares, &nonce, &sig);
    }
    assert_eq!(s.factory.funding_progress(&id), 201);
    // 1100 + 1111: scaling before the divide keeps the 7th decimal that the
    // old 1e6-only math truncated away.
    assert_eq!(s.factory.usdc_raised(&id), 2211);

    let batch = soroban_sdk::vec![&s.e, users[0].clone(), users[1].clone()];
    s.factory.batch_refund_users(&id, &batch, &2);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    for user in users.iter() {
        assert_eq!(op_token.balance(user), 0);
        assert_eq!(usdc_tok.balance(user), 10_000);
        assert_eq!(s.factory.usdc_raised_per_client(&id, user), 0);
    }
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
}

#[test]
fn test_claim_batch_mints_each_user() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.usdc.mint(&s.admin, &10_000);

    let users = [Address::generate(&s.e), Address::generate(&s.e)];
    s.factory.gift_op_tokens(&id, &30, &users[0]);
    s.factory.gift_op_tokens(&id, &40, &users[1]);
    s.factory.start_operation(&id);

    let batch = soroban_sdk::vec![&s.e, users[0].clone(), users[1].clone()];
    s.factory.claim_op_tokens_batch(&id, &batch);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&users[0]), 30);
    assert_eq!(op_token.balance(&users[1]), 40);
    assert_eq!(s.factory.claimable_total(&id, &users[0]), 0);
    assert_eq!(s.factory.claimable_total(&id, &users[1]), 0);

    // Re-claiming is a no-op rather than a double mint.
    s.factory.claim_op_tokens_batch(&id, &batch);
    assert_eq!(op_token.balance(&users[0]), 30);
}

/// The five per-operation booleans are now bits in one `u32`; a mask mistake
/// would silently leak between flags.
#[test]
fn test_flags_are_independent() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);

    s.factory.set_predeposits(&id, &true);
    s.factory.start_operation(&id);
    s.factory.pause_funding(&id, &true);
    assert!(s.factory.predeposits_open(&id));
    assert!(s.factory.operation_started(&id));
    assert!(s.factory.funding_paused(&id));
    assert!(!s.factory.operation_canceled(&id));
    assert!(!s.factory.usdc_withdrawn(&id));

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "paused");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::OperationPaused.into()))
    );

    s.factory.pause_funding(&id, &false);
    assert!(!s.factory.funding_paused(&id));
    assert!(s.factory.operation_started(&id));
    assert!(s.factory.predeposits_open(&id));
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(s.factory.funding_progress(&id), 100);
}

/// Independent reconstruction of the `FIAT_INVEST` payload — deliberately built
/// with naive appends rather than the contract's buffer layout, so an off-by-one
/// in `build_fiat_invest_message` fails here instead of rejecting every backend
/// signature in production.
fn sign_fiat_invest(
    e: &Env,
    factory_id: &Address,
    id: u32,
    user: &Address,
    holder: &Address,
    amount: i128,
    nonce: &String,
) -> BytesN<64> {
    let mut msg = Bytes::from_slice(e, b"FIAT_INVEST");
    msg.append(&Bytes::from(factory_id.to_string()));
    msg.append(&Bytes::from_slice(e, &id.to_be_bytes()));
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(holder.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));
    let v: std::vec::Vec<u8> = msg.iter().collect();
    BytesN::from_array(e, &signer_key().sign(&v).to_bytes())
}

#[test]
fn test_fiat_invest() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    let holder = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "fiat-1");
    let sig =
        sign_fiat_invest(&s.e, &s.factory_id, id, &user, &holder, 40, &nonce);

    s.factory
        .fiat_invest(&id, &40, &user, &holder, &nonce, &sig);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&holder), 40);
    assert!(op_token.is_whitelisted(&user));
    // Fiat settles off-chain: shares count, USDC does not.
    assert_eq!(s.factory.funding_progress(&id), 40);
    assert_eq!(s.factory.usdc_raised(&id), 0);
    assert_eq!(s.factory.usdc_raised_per_client(&id, &user), 0);

    // The nonce is consumed, so the same signature cannot be replayed.
    assert_eq!(
        s.factory
            .try_fiat_invest(&id, &40, &user, &holder, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}
