#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    contract, contractimpl, symbol_short,
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

// --- Mock Reflector oracle: EUR/USD = 1.1 unless `set_price`, 14 decimals ---
#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn lastprice(e: Env, _asset: Asset) -> Option<PriceData> {
        Some(PriceData {
            price: e
                .storage()
                .instance()
                .get(&symbol_short!("price"))
                .unwrap_or(110_000_000_000_000), // 1.1 * 1e14
            timestamp: e.ledger().timestamp(),
        })
    }
    pub fn decimals(_e: Env) -> u32 {
        14
    }
    pub fn set_price(e: Env, price: i128) {
        e.storage().instance().set(&symbol_short!("price"), &price);
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
    oracle: MockOracleClient<'a>,
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

    let oracle = MockOracleClient::new(&e, &oracle_addr);
    Setup {
        e,
        admin,
        factory,
        factory_id,
        usdc,
        usdc_addr,
        oracle,
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

/// `upgrade` really swaps the executable: after pointing the factory at the
/// op-lend wasm, the instance no longer answers factory calls. Nothing else in
/// the repo could catch a no-op upgrade, since the contract id never changes.
#[test]
fn test_upgrade_replaces_the_executable() {
    let s = setup();
    create_op(&s, 1000);
    assert_eq!(s.factory.operation_count(), 1);

    let other_wasm = s.e.deployer().upload_contract_wasm(oplend::WASM);
    s.factory.upgrade(&other_wasm);

    // State survived, but the code behind it is op-lend now, which has no
    // `operation_count`.
    assert!(s.factory.try_operation_count().is_err());
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

fn invest_as(s: &Setup, id: u32, user: &Address, shares: i128, nonce: &str) {
    let nonce = String::from_str(&s.e, nonce);
    let sig = sign_invest(&s.e, &s.factory_id, id, user, shares, &nonce);
    s.factory.invest(user, &id, &shares, &nonce, &sig);
}

/// Withdrawal no longer waits for the end of the sale; each call takes only
/// what was raised since the previous one.
#[test]
fn test_withdraw_usdc_during_sale_is_incremental() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &100_000);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let dest = Address::generate(&s.e);

    invest_as(&s, id, &user, 100, "n1");
    assert!(!s.factory.is_operation_finished(&id));
    s.factory.withdraw_usdc(&id, &dest);
    assert_eq!(usdc_tok.balance(&dest), 1100);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1100);
    assert!(s.factory.usdc_withdrawn(&id));
    assert_eq!(
        s.factory.try_withdraw_usdc(&id, &dest),
        Err(Ok(Error::NothingToWithdraw.into()))
    );

    // The sale is still open after a withdrawal.
    invest_as(&s, id, &user, 50, "n2");
    s.factory.withdraw_usdc(&id, &dest);
    assert_eq!(usdc_tok.balance(&dest), 1650);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1650);
    assert_eq!(s.factory.usdc_raised(&id), 1650);
    assert_eq!(usdc_tok.balance(&s.factory_id), 0);
}

#[test]
fn test_withdraw_usdc_before_start_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.set_predeposits(&id, &true);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "p1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 50, &nonce);
    s.factory.predeposit(&user, &id, &50, &nonce, &sig);

    assert_eq!(
        s.factory.try_withdraw_usdc(&id, &Address::generate(&s.e)),
        Err(Ok(Error::OperationNotStarted.into()))
    );
}

/// The factory pools every operation's USDC in one balance, so a refund on an
/// operation whose funds were already withdrawn must not be paid out of
/// another operation's escrow.
#[test]
fn test_refund_after_early_withdraw_cannot_drain_other_operation() {
    let s = setup();
    let (a, _) = create_op(&s, 1000);
    let (b, _) = create_op(&s, 1000);
    s.factory.start_operation(&a);
    s.factory.start_operation(&b);
    let early = Address::generate(&s.e);
    let late = Address::generate(&s.e);
    let other = Address::generate(&s.e);
    for u in [&early, &late, &other] {
        s.usdc.mint(u, &10_000);
    }

    invest_as(&s, a, &early, 100, "a1");
    s.factory.withdraw_usdc(&a, &Address::generate(&s.e));
    invest_as(&s, a, &late, 10, "a2");
    invest_as(&s, b, &other, 100, "b1");

    assert_eq!(
        s.factory.try_refund_user(&a, &early),
        Err(Ok(Error::InsufficientEscrow.into()))
    );
    // Money invested after the withdrawal is still escrowed and refundable.
    s.factory.refund_user(&a, &late);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&late), 10_000);
    assert_eq!(usdc_tok.balance(&s.factory_id), 1100); // operation b, intact
}

/// Operations withdrawn by the previously deployed code carry `F_WITHDRAWN`
/// but no `UsdcWithdrawn` entry; they must read as fully withdrawn rather than
/// letting the admin take the same USDC a second time.
#[test]
fn test_legacy_withdrawn_operation_counts_as_fully_withdrawn() {
    use crate::types::{DataKey, OpData, F_WITHDRAWN};

    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    invest_as(&s, id, &user, 100, "n1");

    s.e.as_contract(&s.factory_id, || {
        let key = DataKey::Op(id);
        let mut op: OpData = s.e.storage().persistent().get(&key).unwrap();
        op.set_flag(F_WITHDRAWN, true);
        s.e.storage().persistent().set(&key, &op);
    });

    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1100);
    assert_eq!(
        s.factory.try_withdraw_usdc(&id, &Address::generate(&s.e)),
        Err(Ok(Error::NothingToWithdraw.into()))
    );
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

// --- EURC ---

/// Registers a 7-decimal EURC SAC (Circle's EURC is a classic asset too) and
/// points the factory at it.
fn with_eurc<'a>(
    s: &Setup,
) -> (StellarAssetClient<'a>, soroban_sdk::token::Client<'a>) {
    let sac =
        s.e.register_stellar_asset_contract_v2(Address::generate(&s.e));
    s.factory.set_eurc(&sac.address());
    (
        StellarAssetClient::new(&s.e, &sac.address()),
        soroban_sdk::token::Client::new(&s.e, &sac.address()),
    )
}

fn invest_eur_as(
    s: &Setup,
    id: u32,
    user: &Address,
    shares: i128,
    nonce: &str,
) {
    let nonce = String::from_str(&s.e, nonce);
    let sig = sign_invest(&s.e, &s.factory_id, id, user, shares, &nonce);
    s.factory.invest_eur(user, &id, &shares, &nonce, &sig);
}

/// `fields` of the latest `name` event the factory published in the last
/// invocation — so call it before any other client call.
fn last_event<const N: usize>(
    s: &Setup,
    name: &str,
    fields: [&str; N],
) -> [i128; N] {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::xdr::{ContractEventBody, ScVal};
    use soroban_sdk::{Map, Symbol, TryFromVal, Val};

    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    let body = events
        .events()
        .iter()
        .rev()
        .map(|ev| {
            let ContractEventBody::V0(body) = &ev.body;
            body
        })
        .find(|body| {
            matches!(&body.topics[0], ScVal::Symbol(sym)
                if sym.0.as_slice() == name.as_bytes())
        })
        .unwrap_or_else(|| panic!("no `{name}` event"));
    let data = Val::try_from_val(&s.e, &body.data).unwrap();
    let data = Map::<Symbol, i128>::try_from_val(&s.e, &data).unwrap();
    fields.map(|f| data.get(Symbol::new(&s.e, f)).unwrap())
}

/// Shares are priced in EUR, so EURC pays the price as is: 100 shares at
/// 1 EUR are 1000 base units of a 7-decimal EURC, with no oracle leg. The
/// `Invested` event still reports USD — the same 1100 a USDC invest charges.
#[test]
fn test_invest_eur_happy_path() {
    let s = setup();
    let (eurc, eurc_tok) = with_eurc(&s);
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    eurc.mint(&user, &10_000);

    assert_eq!(s.factory.get_amount_in_eur(&id, &100), 1000);
    invest_eur_as(&s, id, &user, 100, "e1");
    assert_eq!(last_event(&s, "invested", ["usdc_amount"]), [1100]);

    assert_eq!(s.factory.funding_progress(&id), 100);
    assert_eq!(s.factory.eurc_raised(&id), 1000);
    assert_eq!(s.factory.eurc_raised_per_client(&id, &user), 1000);
    // EURC never leaks into the USDC books.
    assert_eq!(s.factory.usdc_raised(&id), 0);
    assert_eq!(s.factory.usdc_raised_per_client(&id, &user), 0);
    assert_eq!(eurc_tok.balance(&user), 9_000);
    assert_eq!(eurc_tok.balance(&s.factory_id), 1000);
    assert_eq!(oplend::Client::new(&s.e, &op_addr).balance(&user), 100);
}

/// `invest_eur` takes the existing `ONCHAIN_INVEST` signature, so one backend
/// authorization pays in either token, once.
#[test]
fn test_invest_eur_shares_nonces_with_invest() {
    let s = setup();
    let (eurc, _) = with_eurc(&s);
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    eurc.mint(&user, &10_000);

    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(
        s.factory.try_invest_eur(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}

#[test]
fn test_eurc_is_required_and_set_once() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "e1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    assert_eq!(
        s.factory.try_invest_eur(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::NotInitialized.into()))
    );

    let (_, eurc_tok) = with_eurc(&s);
    assert_eq!(s.factory.eurc(), eurc_tok.address);
    let other =
        s.e.register_stellar_asset_contract_v2(Address::generate(&s.e));
    assert_eq!(
        s.factory.try_set_eurc(&other.address()),
        Err(Ok(Error::AlreadyInitialized.into()))
    );
}

/// `withdraw_usdc` sends each token of a mixed raise as itself. An EURC-only
/// withdrawal must not set `F_WITHDRAWN`: without a `UsdcWithdrawn` entry that
/// flag reads as "all USDC taken" and would lock later USDC in the factory.
#[test]
fn test_withdraw_mixed_raise_sends_both_tokens() {
    let s = setup();
    let (eurc, eurc_tok) = with_eurc(&s);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &100_000);
    eurc.mint(&user, &100_000);
    let dest = Address::generate(&s.e);

    invest_eur_as(&s, id, &user, 100, "e1");
    s.factory.withdraw_usdc(&id, &dest);
    assert_eq!(
        last_event(&s, "funding_withdrawn", ["usdc_amount", "eurc_amount"]),
        [0, 1000]
    );
    assert_eq!(eurc_tok.balance(&dest), 1000);
    assert!(!s.factory.usdc_withdrawn(&id));

    invest_as(&s, id, &user, 100, "u1");
    invest_eur_as(&s, id, &user, 50, "e2");
    s.factory.withdraw_usdc(&id, &dest);
    assert_eq!(
        last_event(&s, "funding_withdrawn", ["usdc_amount", "eurc_amount"]),
        [1100, 500]
    );
    assert_eq!(usdc_tok.balance(&dest), 1100);
    assert_eq!(eurc_tok.balance(&dest), 1500);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1100);
    assert_eq!(s.factory.eurc_withdrawn_amount(&id), 1500);
    assert!(s.factory.usdc_withdrawn(&id));
    assert_eq!(usdc_tok.balance(&s.factory_id), 0);
    assert_eq!(eurc_tok.balance(&s.factory_id), 0);
    assert_eq!(
        s.factory.try_withdraw_usdc(&id, &dest),
        Err(Ok(Error::NothingToWithdraw.into()))
    );
}

/// Refunds return each token as it was paid, for USDC-only, EURC-only and
/// mixed investors alike. `Refunded` counts EURC at its invest-time USD value,
/// so it nets out the user's `Invested` events even after the rate moved.
#[test]
fn test_refund_mixed_raise_returns_each_token() {
    let s = setup();
    let (eurc, eurc_tok) = with_eurc(&s);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let usdc_only = Address::generate(&s.e);
    let eurc_only = Address::generate(&s.e);
    let both = Address::generate(&s.e);
    let users = [&usdc_only, &eurc_only, &both];
    for u in users {
        s.usdc.mint(u, &10_000);
        eurc.mint(u, &10_000);
    }

    invest_as(&s, id, &usdc_only, 100, "u1");
    invest_eur_as(&s, id, &eurc_only, 100, "e1");
    invest_as(&s, id, &both, 10, "u2"); // 110 USDC
    invest_eur_as(&s, id, &both, 20, "e2"); // 200 EURC, worth 220 USD at 1.1

    s.oracle.set_price(&120_000_000_000_000); // EUR/USD 1.2
    s.factory.refund_user(&id, &both);
    assert_eq!(last_event(&s, "refunded", ["usdc_amount"]), [330]);

    let batch = soroban_sdk::vec![&s.e, eurc_only.clone(), usdc_only.clone()];
    s.factory.batch_refund_users(&id, &batch, &2);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    for u in users {
        assert_eq!(op_token.balance(u), 0);
        assert_eq!(usdc_tok.balance(u), 10_000);
        assert_eq!(eurc_tok.balance(u), 10_000);
        assert_eq!(s.factory.usdc_raised_per_client(&id, u), 0);
        assert_eq!(s.factory.eurc_raised_per_client(&id, u), 0);
    }
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
    assert_eq!(s.factory.eurc_raised(&id), 0);
}

/// EURC of every operation sits in one balance, exactly like USDC.
#[test]
fn test_eurc_refund_after_withdraw_cannot_drain_other_operation() {
    let s = setup();
    let (eurc, eurc_tok) = with_eurc(&s);
    let (a, _) = create_op(&s, 1000);
    let (b, _) = create_op(&s, 1000);
    s.factory.start_operation(&a);
    s.factory.start_operation(&b);
    let early = Address::generate(&s.e);
    let late = Address::generate(&s.e);
    let other = Address::generate(&s.e);
    for u in [&early, &late, &other] {
        eurc.mint(u, &10_000);
    }

    invest_eur_as(&s, a, &early, 100, "a1");
    s.factory.withdraw_usdc(&a, &Address::generate(&s.e));
    invest_eur_as(&s, a, &late, 10, "a2");
    invest_eur_as(&s, b, &other, 100, "b1");

    assert_eq!(
        s.factory.try_refund_user(&a, &early),
        Err(Ok(Error::InsufficientEscrow.into()))
    );
    s.factory.refund_user(&a, &late);
    assert_eq!(eurc_tok.balance(&late), 10_000);
    assert_eq!(eurc_tok.balance(&s.factory_id), 1000); // operation b, intact
}
