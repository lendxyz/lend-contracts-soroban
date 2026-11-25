#![cfg(test)]
extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Deployer as _, Events as _, Ledger as _},
    token::StellarAssetClient,
    Address, Bytes, BytesN, Env, Event as _, String, Symbol,
};

use crate::contract::{LendFactory, LendFactoryClient};
use crate::errors::Error;
use crate::events::OperationFinished;
use crate::oracle::{Asset, PriceData};
use crate::types::RefundEntry;

// Op-lend wasm, built by `cargo build -p lend-operation-token --target wasm32v1-none --release`.
mod oplend {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/lend_operation_token.wasm"
    );
}

// Op-lend-wallet wasm: reads the factory's `backend_signer` and
// `get_operation`, so it is exercised against the real factory here.
mod wallet {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/lend_wallet.wasm"
    );
}

/// Mock Reflector feed, 14 decimals, answering only for the symbols it was
/// given a price for (so a factory asking for the wrong asset gets no price).
/// Tests move a price, remove them all, or backdate them to exercise the
/// factory's oracle checks.
#[contract]
pub struct MockFeed;

#[contractimpl]
impl MockFeed {
    pub fn __constructor(e: Env, symbol: Symbol, price: i128) {
        e.storage().instance().set(&symbol_short!("sym"), &symbol);
        e.storage().instance().set(&symbol, &price);
    }
    /// Moves the price of the symbol the feed was deployed with.
    pub fn set_price(e: Env, price: i128) {
        let symbol: Symbol =
            e.storage().instance().get(&symbol_short!("sym")).unwrap();
        Self::set_price_of(e, symbol, price);
    }
    pub fn set_price_of(e: Env, symbol: Symbol, price: i128) {
        e.storage().instance().set(&symbol, &price);
        e.storage().instance().remove(&symbol_short!("none"));
    }
    /// The feed has no price for any symbol any more.
    pub fn remove_price(e: Env) {
        e.storage().instance().set(&symbol_short!("none"), &true);
    }
    /// Reports its prices as published `age` seconds ago.
    pub fn set_age(e: Env, age: u64) {
        e.storage().instance().set(&symbol_short!("age"), &age);
    }
    pub fn lastprice(e: Env, asset: Asset) -> Option<PriceData> {
        let st = e.storage().instance();
        if st.has(&symbol_short!("none")) {
            return None;
        }
        match asset {
            Asset::Other(symbol) => st.get(&symbol).map(|price| PriceData {
                price,
                timestamp: e
                    .ledger()
                    .timestamp()
                    .saturating_sub(st.get(&symbol_short!("age")).unwrap_or(0)),
            }),
            Asset::Stellar(_) => None,
        }
    }
    pub fn decimals(_e: Env) -> u32 {
        14
    }
}

/// EUR/USD forex feed at 1.1.
fn eur_feed(e: &Env) -> Address {
    e.register(MockFeed, (symbol_short!("EUR"), 110_000_000_000_000_i128))
}

/// Stablecoin feed with both coins at par: USDC at 1 USD, EURC at 1.1 USD
/// (1 EUR at the `eur_feed` rate).
fn stablecoin_feed(e: &Env) -> Address {
    let feed =
        e.register(MockFeed, (symbol_short!("USDC"), 100_000_000_000_000_i128));
    MockFeedClient::new(e, &feed)
        .set_price_of(&symbol_short!("EURC"), &110_000_000_000_000_i128);
    feed
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
    eurc: StellarAssetClient<'a>,
    eurc_addr: Address,
    oracle: MockFeedClient<'a>,
    stablecoin_oracle: MockFeedClient<'a>,
}

fn setup<'a>() -> Setup<'a> {
    setup_from(1)
}

fn setup_from<'a>(first_op_id: u32) -> Setup<'a> {
    let e = Env::default();
    e.mock_all_auths();
    e.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&e);

    let usdc_issuer = Address::generate(&e);
    let usdc_sac = e.register_stellar_asset_contract_v2(usdc_issuer);
    let usdc_addr = usdc_sac.address();
    let usdc = StellarAssetClient::new(&e, &usdc_addr);

    let oracle_addr = eur_feed(&e);

    let (factory_id, stablecoin_oracle, eurc_addr) =
        register_factory(&e, &admin, &usdc_addr, &oracle_addr, first_op_id);
    let factory = LendFactoryClient::new(&e, &factory_id);

    Setup {
        oracle: MockFeedClient::new(&e, &oracle_addr),
        stablecoin_oracle: MockFeedClient::new(&e, &stablecoin_oracle),
        eurc: StellarAssetClient::new(&e, &eurc_addr),
        e,
        admin,
        factory,
        factory_id,
        usdc,
        usdc_addr,
        eurc_addr,
    }
}

/// Deploys a factory, running its `__constructor`, with a fresh at-par
/// stablecoin feed and a fresh 7-decimal EURC SAC (Circle's EURC is a classic
/// asset too). Returns (factory, stablecoin oracle, EURC).
fn register_factory(
    e: &Env,
    admin: &Address,
    usdc: &Address,
    oracle: &Address,
    first_op_id: u32,
) -> (Address, Address, Address) {
    let signer =
        BytesN::from_array(e, &signer_key().verifying_key().to_bytes());
    let wasm_hash = e.deployer().upload_contract_wasm(oplend::WASM);
    let stablecoin_oracle = stablecoin_feed(e);
    let eurc = e
        .register_stellar_asset_contract_v2(Address::generate(e))
        .address();
    let factory = e.register(
        LendFactory,
        (
            admin.clone(),
            usdc.clone(),
            eurc.clone(),
            oracle.clone(),
            stablecoin_oracle.clone(),
            signer,
            wasm_hash,
            first_op_id,
        ),
    );
    (factory, stablecoin_oracle, eurc)
}

/// Independent reconstruction of the `ONCHAIN_INVEST` payload. Keep it naive
/// (plain appends, not the contract's buffer layout) so a byte-layout change in
/// `build_invest_message` breaks the tests instead of every backend signature.
fn invest_msg(
    e: &Env,
    factory_id: &Address,
    id: u32,
    user: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    let mut msg = Bytes::from_slice(e, b"ONCHAIN_INVEST");
    msg.append(&Bytes::from(factory_id.to_string()));
    msg.append(&Bytes::from_slice(e, &id.to_be_bytes()));
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

fn sign_with(e: &Env, key: &SigningKey, msg: &Bytes) -> BytesN<64> {
    let v: std::vec::Vec<u8> = msg.iter().collect();
    BytesN::from_array(e, &key.sign(&v).to_bytes())
}

fn sign_invest(
    e: &Env,
    factory_id: &Address,
    id: u32,
    user: &Address,
    amount: i128,
    nonce: &String,
) -> BytesN<64> {
    let msg = invest_msg(e, factory_id, id, user, amount, nonce);
    sign_with(e, &signer_key(), &msg)
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
    assert_eq!(op.eur_per_share, EUR_PER_SHARE);
    assert_eq!(op.op_token, op_addr);
    assert_eq!(s.factory.operation_count(), 1);
    assert!(!s.factory.operation_started(&id));
}

/// A redeployed factory continues a previous one's numbering, so ids keyed in
/// other contracts (wallet registry, rewards roots) never collide.
#[test]
fn test_first_op_id_offsets_operation_ids() {
    let s = setup_from(5);
    assert_eq!(s.factory.operation_count(), 4);

    let (id, op_addr) = create_op(&s, 1000);
    assert_eq!(id, 5);
    assert_eq!(s.factory.get_operation(&5).op_token, op_addr);
    assert_eq!(
        s.factory.try_get_operation(&1).err(),
        Some(Ok(Error::OperationNotFound.into()))
    );

    let (next, _) = create_op(&s, 1000);
    assert_eq!(next, 6);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_first_op_id_zero_is_rejected() {
    let e = Env::default();
    e.mock_all_auths();
    let usdc = e
        .register_stellar_asset_contract_v2(Address::generate(&e))
        .address();
    let oracle = eur_feed(&e);

    register_factory(&e, &Address::generate(&e), &usdc, &oracle, 0);
}

/// AUDIT Factory C16: a zero or negative price would make shares free (or pay
/// the buyer), so `create_operation` rejects it.
#[test]
fn test_create_operation_rejects_non_positive_price() {
    let s = setup();
    let name = String::from_str(&s.e, "Alpha");
    for price in [0, -1] {
        assert_eq!(
            s.factory.try_create_operation(&name, &1000, &price).err(),
            Some(Ok(Error::InvalidPrice.into()))
        );
    }
    assert_eq!(s.factory.operation_count(), 0);
}

/// AUDIT Factory C16: if `total_shares * eur_per_share` overflows, the last
/// share sold would panic computing the operation's EUR value, so the
/// operation could never finish. Rejected up front instead.
#[test]
fn test_create_operation_rejects_overflowing_raise() {
    let s = setup();
    let name = String::from_str(&s.e, "Alpha");
    let shares = i128::MAX / 2 + 1;

    assert_eq!(
        s.factory.try_create_operation(&name, &shares, &2).err(),
        Some(Ok(Error::RaiseOverflow.into()))
    );
    // The largest product that still fits is accepted.
    s.factory.create_operation(&name, &(i128::MAX / 2), &2);
    assert_eq!(s.factory.operation_count(), 1);
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

    // EURC is taken at par with EUR: 100 shares * 1 EUR = 1000 base units of
    // the 7-decimal EURC, and back.
    assert_eq!(s.factory.get_amount_in_eur(&id, &100), 1000);
    assert_eq!(s.factory.get_amount_out_eur(&id, &1000), 100);

    // A moved EUR/USD rate moves the USDC quotes only.
    s.oracle.set_price(&120_000_000_000_000);
    assert_eq!(s.factory.get_amount_out(&id, &1200), 100);
    assert_eq!(s.factory.get_amount_out_eur(&id, &1000), 100);
}

/// AUDIT Factory C13: an order that rounds to zero fails instead of being
/// quoted (and sold) at 1 base unit.
#[test]
fn test_order_rounding_to_zero_fails() {
    let s = setup();
    // 1e-6 EUR per share: one share base unit costs less than 1e-7 USDC.
    let name = String::from_str(&s.e, "Tiny");
    let op_addr = s.factory.create_operation(&name, &1000, &1);
    let id = s.factory.operation_count();
    s.factory.start_operation(&id);
    let too_small = Some(Ok(Error::AmountTooSmall.into()));

    assert_eq!(s.factory.try_get_amount_in(&id, &1).err(), too_small);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "tiny");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 1, &nonce);
    assert_eq!(
        s.factory.try_invest(&user, &id, &1, &nonce, &sig).err(),
        too_small
    );
    assert_eq!(oplend::Client::new(&s.e, &op_addr).balance(&user), 0);

    // The other direction: 1 base unit of 7-decimal USDC buys no share unit.
    let (normal, _) = create_op(&s, 1000);
    assert_eq!(s.factory.try_get_amount_out(&normal, &1).err(), too_small);
    // Same for 1 base unit of 7-decimal EURC.
    assert_eq!(
        s.factory.try_get_amount_out_eur(&normal, &1).err(),
        too_small
    );

    // EURC follows the same rule: 1e-12 EUR is less than 1e-7 EURC.
    assert_eq!(s.factory.try_get_amount_in_eur(&id, &1).err(), too_small);
    s.eurc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "tiny-eur");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 1, &nonce);
    assert_eq!(
        s.factory.try_invest_eur(&user, &id, &1, &nonce, &sig).err(),
        too_small
    );
    assert_eq!(oplend::Client::new(&s.e, &op_addr).balance(&user), 0);
}

/// AUDIT Factory C12: a missing cached scale (USDC, EURC or oracle decimals)
/// fails the quote instead of silently pricing with a guessed 6 decimals.
#[test]
fn test_missing_decimals_fail_instead_of_defaulting() {
    use crate::types::{Coin, DataKey};

    for key in [DataKey::TokenDecimals(Coin::Usdc), DataKey::OracleDecimals] {
        let s = setup();
        let (id, _) = create_op(&s, 1000);
        s.e.as_contract(&s.factory_id, || {
            s.e.storage().instance().remove(&key);
        });

        assert_eq!(
            s.factory.try_get_amount_in(&id, &100),
            Err(Ok(Error::NotInitialized.into()))
        );
    }

    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.e.as_contract(&s.factory_id, || {
        s.e.storage()
            .instance()
            .remove(&DataKey::TokenDecimals(Coin::Eurc));
    });
    assert_eq!(
        s.factory.try_get_amount_in_eur(&id, &100),
        Err(Ok(Error::NotInitialized.into()))
    );
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
    let oracle = eur_feed(e);
    let (factory_id, _, _) =
        register_factory(e, &Address::generate(e), usdc, &oracle, 1);
    LendFactoryClient::new(e, &factory_id)
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
    let expected = crate::events::Invested {
        investor: user.clone(),
        operation_id: id,
        asset: s.usdc_addr.clone(),
        usdc_amount: 1100,
        shares_bought: 100,
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));

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

/// Independent reconstruction of the `VOID_NONCE` payload.
fn sign_void(
    e: &Env,
    contract: &Address,
    nonce: &String,
) -> Option<BytesN<64>> {
    let mut msg = Bytes::from_slice(e, b"VOID_NONCE");
    msg.append(&Bytes::from(contract.to_string()));
    msg.append(&Bytes::from(nonce.clone()));
    let v: std::vec::Vec<u8> = msg.iter().collect();
    Some(BytesN::from_array(e, &signer_key().sign(&v).to_bytes()))
}

#[test]
fn test_void_nonce_blocks_the_invest_signature() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    let void_sig = sign_void(&s.e, &s.factory_id, &nonce);

    s.factory.void_nonce(&nonce, &void_sig);

    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
    assert_eq!(
        s.factory.try_void_nonce(&nonce, &void_sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}

/// Neither the invest signature nor a void signed for another contract may
/// burn the nonce, and a rejected void leaves it spendable.
#[test]
fn test_void_nonce_rejects_foreign_signatures() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    let other = Address::generate(&s.e);

    assert!(s
        .factory
        .try_void_nonce(&nonce, &Some(sig.clone()))
        .is_err());
    assert!(s
        .factory
        .try_void_nonce(&nonce, &sign_void(&s.e, &other, &nonce))
        .is_err());

    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(s.factory.funding_progress(&id), 100);
}

#[test]
fn test_admin_void_nonce_needs_no_signature() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);

    s.factory.void_nonce(&nonce, &None);

    assert_eq!(
        s.factory.try_invest(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );

    // Without a signature, only the admin's auth can void.
    s.e.set_auths(&[]);
    let other = String::from_str(&s.e, "n2");
    assert!(s.factory.try_void_nonce(&other, &None).is_err());
}

/// op-lend's admin is the factory, so its admin void runs through here.
#[test]
fn test_oplend_void_nonce_blocks_the_whitelist_signature() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    let op_token = oplend::Client::new(&s.e, &op_addr);

    let user = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "wl-1");
    let mut msg = Bytes::from(op_addr.to_string());
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(nonce.clone()));
    let v: std::vec::Vec<u8> = msg.iter().collect();
    let sig = BytesN::from_array(&s.e, &signer_key().sign(&v).to_bytes());

    s.factory.oplend_void_nonce(&id, &nonce);

    assert!(op_token.try_whitelist_user(&user, &nonce, &sig).is_err());
    assert!(!op_token.is_whitelisted(&user));
}

/// Stand-in "new op-lend code" for the upgrade test: any other wasm will do.
const OTHER_WASM: &[u8] =
    include_bytes!("../../../target/wasm32v1-none/release/lend_wallet.wasm");

/// AUDIT OpLend C3: op-lend tokens are upgradable through the factory, which
/// also deploys later operations from the new code.
#[test]
fn test_oplend_upgrade_swaps_code_and_future_deploys() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.total_supply(), 0);

    let new_hash = s.e.deployer().upload_contract_wasm(OTHER_WASM);
    s.factory.oplend_upgrade(&id, &new_hash);

    assert_eq!(s.factory.oplend_wasm_hash(), new_hash);
    // Same id, other code: the wallet wasm has no `total_supply`.
    assert!(op_token.try_total_supply().is_err());
    // The TTL bump runs after the swap, so the new code is as long-lived as
    // the token instance instead of keeping its short upload TTL.
    let d = s.e.deployer();
    assert_eq!(
        d.get_contract_code_ttl(&op_addr),
        d.get_contract_instance_ttl(&op_addr)
    );
}

/// AUDIT Factory C19: the TTL extension in `upgrade` runs after the swap.
/// Instance TTL extension also covers the code entry, so the *new* wasm must
/// come out as long-lived as the instance, not keep its short upload TTL.
#[test]
fn test_upgrade_extends_the_new_code_ttl() {
    let s = setup();
    let new_hash = s.e.deployer().upload_contract_wasm(OTHER_WASM);
    s.factory.upgrade(&new_hash);

    let d = s.e.deployer();
    assert_eq!(
        d.get_contract_code_ttl(&s.factory_id),
        d.get_contract_instance_ttl(&s.factory_id)
    );
}

#[test]
fn test_oplend_upgrade_requires_factory_admin() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    let old_hash = s.factory.oplend_wasm_hash();
    let new_hash = s.e.deployer().upload_contract_wasm(OTHER_WASM);

    s.e.set_auths(&[]);
    assert!(s.factory.try_oplend_upgrade(&id, &new_hash).is_err());
    assert_eq!(s.factory.oplend_wasm_hash(), old_hash);
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
fn test_refund_user() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);

    s.factory.refund_user(&id, &user, &100, &1100, &0);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&user), 0);
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&user), 10_000); // fully refunded
}

/// AUDIT Factory C1: shares and USDC are refunded in admin-decided amounts.
/// Shares moved to another wallet before the refund cannot be counted as
/// burned, and the USDC repaid is whatever the admin settles on, not the
/// whole recorded investment.
#[test]
fn test_refund_takes_admin_decided_shares_and_usdc() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let op_token = oplend::Client::new(&s.e, &op_addr);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    invest_as(&s, id, &user, 100, "n1"); // 100 shares for 1100

    // Before the refund, 60 shares move to another wallet.
    let friend = Address::generate(&s.e);
    s.factory.oplend_whitelist_user(&id, &friend, &true);
    op_token.transfer(&user, &friend, &60);

    // The full position can no longer be burned from `user`...
    assert_eq!(
        s.factory.try_refund_user(&id, &user, &100, &1100, &0).err(),
        Some(Ok(Error::InsufficientShareBalance.into()))
    );
    // ...so the admin refunds only the 40 shares still held, for their USDC.
    s.factory.refund_user(&id, &user, &40, &440, &0);

    assert_eq!(op_token.balance(&user), 0);
    assert_eq!(op_token.balance(&friend), 60);
    assert_eq!(usdc_tok.balance(&user), 10_000 - 1100 + 440);
    assert_eq!(s.factory.funding_progress(&id), 60);
    assert_eq!(s.factory.usdc_raised(&id), 1100 - 440);
    assert_eq!(s.factory.usdc_raised_per_client(&id, &user), 1100);

    // Nothing to burn and nothing to pay, or negative amounts, are rejected.
    assert_eq!(
        s.factory.try_refund_user(&id, &friend, &0, &0, &0).err(),
        Some(Ok(Error::InvalidAmount.into()))
    );
    assert_eq!(
        s.factory.try_refund_user(&id, &friend, &-1, &0, &0).err(),
        Some(Ok(Error::InvalidShares.into()))
    );
    assert_eq!(
        s.factory.try_refund_user(&id, &friend, &1, &-1, &0).err(),
        Some(Ok(Error::InvalidAmount.into()))
    );
    assert_eq!(
        s.factory.try_refund_user(&id, &friend, &1, &0, &-1).err(),
        Some(Ok(Error::InvalidAmount.into()))
    );
}

#[test]
fn test_withdraw_funds_after_finish() {
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
    s.factory.withdraw_funds(&id, &dest);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&dest), 1100);
    assert!(s.factory.funds_withdrawn(&id));
}

fn invest_as(s: &Setup, id: u32, user: &Address, shares: i128, nonce: &str) {
    let nonce = String::from_str(&s.e, nonce);
    let sig = sign_invest(&s.e, &s.factory_id, id, user, shares, &nonce);
    s.factory.invest(user, &id, &shares, &nonce, &sig);
}

/// AUDIT Factory C8: `amount_raised_euro` is 6-decimal EUR like every other
/// internal amount, not the 12-decimal shares × price product.
#[test]
fn test_operation_finished_reports_six_decimal_euros() {
    let s = setup();
    // 2 whole shares (6-decimal) at 1 EUR each.
    let (id, _) = create_op(&s, 2_000_000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &100_000_000);
    invest_as(&s, id, &user, 2_000_000, "fill");

    let expected = OperationFinished {
        operation_id: id,
        amount_raised_euro: 2_000_000, // 2 EUR
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));
}

/// Withdrawal no longer waits for the end of the sale; each call takes only
/// what was raised since the previous one.
#[test]
fn test_withdraw_funds_during_sale_is_incremental() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &100_000);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let dest = Address::generate(&s.e);

    invest_as(&s, id, &user, 100, "n1");
    assert!(!s.factory.is_operation_finished(&id));
    s.factory.withdraw_funds(&id, &dest);
    assert_eq!(usdc_tok.balance(&dest), 1100);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1100);
    assert!(s.factory.funds_withdrawn(&id));
    assert_eq!(
        s.factory.try_withdraw_funds(&id, &dest),
        Err(Ok(Error::NothingToWithdraw.into()))
    );

    // The sale is still open after a withdrawal.
    invest_as(&s, id, &user, 50, "n2");
    s.factory.withdraw_funds(&id, &dest);
    assert_eq!(usdc_tok.balance(&dest), 1650);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1650);
    assert_eq!(s.factory.usdc_raised(&id), 1650);
    assert_eq!(usdc_tok.balance(&s.factory_id), 0);
}

#[test]
fn test_withdraw_funds_before_start_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    assert_eq!(
        s.factory.try_withdraw_funds(&id, &Address::generate(&s.e)),
        Err(Ok(Error::OperationNotStarted.into()))
    );
}

/// AUDIT Factory C18: raised funds cannot be withdrawn to the factory itself
/// (it would count as withdrawn yet stay stranded in the shared balance) or to
/// the all-zero account / contract ids, Stellar's `address(0)`.
#[test]
fn test_withdraw_funds_rejects_stranding_destinations() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    invest_as(&s, id, &user, 100, "n1");

    assert_eq!(
        s.factory.try_withdraw_funds(&id, &s.factory_id),
        Err(Ok(Error::SelfTargetNotAllowed.into()))
    );
    for zero in [
        "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
        "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
    ] {
        assert_eq!(
            s.factory
                .try_withdraw_funds(&id, &Address::from_str(&s.e, zero)),
            Err(Ok(Error::ZeroAddressNotAllowed.into()))
        );
    }
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 0);

    let dest = Address::generate(&s.e);
    s.factory.withdraw_funds(&id, &dest);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1100);
}

/// The factory pools every operation's USDC in one balance, so a refund on an
/// operation whose funds were already withdrawn must not be paid out of
/// another operation's escrow. The admin's own wallet pays it instead, in
/// full, when it holds enough; otherwise it fails with `InsufficientEscrow`.
#[test]
fn test_refund_after_early_withdraw_cannot_drain_other_operation() {
    let s = setup();
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
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
    s.factory.withdraw_funds(&a, &Address::generate(&s.e));
    invest_as(&s, a, &late, 10, "a2");
    invest_as(&s, b, &other, 100, "b1");

    // Escrow of `a` holds 110, and the admin's wallet one unit short of 1100.
    let insufficient = Err(Ok(Error::InsufficientEscrow.into()));
    assert_eq!(
        s.factory.try_refund_user(&a, &early, &100, &1100, &0),
        insufficient
    );
    s.usdc.mint(&s.admin, &1099);
    assert_eq!(
        s.factory.try_refund_user(&a, &early, &100, &1100, &0),
        insufficient
    );

    // With enough, the admin pays all of it; the escrow is left as it was.
    s.usdc.mint(&s.admin, &1);
    s.factory.refund_user(&a, &early, &100, &1100, &0);
    // The admin's signature on `refund_user` must cover the nested transfer
    // out of its wallet.
    let (who, tree) = s.e.auths().into_iter().next().unwrap();
    assert_eq!(who, s.admin);
    assert!(tree.sub_invocations.iter().any(|sub| matches!(
        &sub.function,
        soroban_sdk::testutils::AuthorizedFunction::Contract((token, f, _))
            if *token == s.usdc_addr && *f == Symbol::new(&s.e, "transfer")
    )));
    let expected = crate::events::Refunded {
        investor: early.clone(),
        operation_id: a,
        usdc_amount: 1100,
        eurc_amount: 0,
        shares_refunded: 100,
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));
    assert_eq!(usdc_tok.balance(&early), 10_000);
    assert_eq!(usdc_tok.balance(&s.admin), 0);
    assert_eq!(s.factory.usdc_raised(&a), 1210);
    assert_eq!(s.factory.funding_progress(&a), 10);

    // Money invested after the withdrawal is still escrowed and refundable,
    // from the escrow, whatever the admin holds.
    s.usdc.mint(&s.admin, &500);
    s.factory.refund_user(&a, &late, &10, &110, &0);
    assert_eq!(usdc_tok.balance(&late), 10_000);
    assert_eq!(usdc_tok.balance(&s.admin), 500);
    assert_eq!(s.factory.usdc_raised(&a), 1100);
    assert_eq!(usdc_tok.balance(&s.factory_id), 1100); // operation b, intact
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

/// Every per-operation read fails the same way for an unknown id, whichever
/// coin it is about (EURC reads used to answer 0).
#[test]
fn test_get_operation_nonexistent() {
    let s = setup();
    let not_found = Some(Ok(Error::OperationNotFound.into()));
    assert_eq!(s.factory.try_get_operation(&99).err(), not_found);
    assert_eq!(s.factory.try_usdc_raised(&99).err(), not_found);
    assert_eq!(s.factory.try_eurc_raised(&99).err(), not_found);
    assert_eq!(s.factory.try_usdc_withdrawn_amount(&99).err(), not_found);
    assert_eq!(s.factory.try_eurc_withdrawn_amount(&99).err(), not_found);
    assert_eq!(s.factory.try_funds_withdrawn(&99).err(), not_found);
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

    let entry = |user: &Address, shares: i128, usdc: i128| RefundEntry {
        user: user.clone(),
        shares_to_burn: shares,
        usdc_to_refund: usdc,
        eurc_to_refund: 0,
    };
    let batch = soroban_sdk::vec![
        &s.e,
        entry(&users[0], 100, 1100),
        entry(&users[1], 101, 1111)
    ];
    s.factory.batch_refund_users(&id, &batch);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    for (user, paid) in users.iter().zip([1100, 1111]) {
        assert_eq!(op_token.balance(user), 0);
        assert_eq!(usdc_tok.balance(user), 10_000);
        // The record of USDC paid in is not rewritten by a refund.
        assert_eq!(s.factory.usdc_raised_per_client(&id, user), paid);
    }
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
}

/// The per-operation booleans are bits in one `u32`; a mask mistake
/// would silently leak between flags.
#[test]
fn test_flags_are_independent() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);

    s.factory.start_operation(&id);
    s.factory.pause_funding(&id, &true);
    assert!(s.factory.operation_started(&id));
    assert!(s.factory.funding_paused(&id));
    assert!(!s.factory.operation_canceled(&id));

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
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(s.factory.funding_progress(&id), 100);
}

/// AUDIT Factory C15: start / cancel / pause / resume fail when the operation
/// is already in the requested state, instead of emitting an event for a
/// change that never happened.
#[test]
fn test_repeated_state_change_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    let unchanged = Err(Ok(Error::StateUnchanged.into()));

    // Resuming an operation that was never paused.
    assert_eq!(s.factory.try_pause_funding(&id, &false), unchanged);

    s.factory.start_operation(&id);
    assert_eq!(s.factory.try_start_operation(&id), unchanged);

    s.factory.pause_funding(&id, &true);
    assert_eq!(s.factory.try_pause_funding(&id, &true), unchanged);
    s.factory.pause_funding(&id, &false);
    assert_eq!(s.factory.try_pause_funding(&id, &false), unchanged);

    s.factory.cancel_operation(&id);
    assert_eq!(s.factory.try_cancel_operation(&id), unchanged);

    assert!(s.factory.operation_started(&id));
    assert!(!s.factory.funding_paused(&id));
    assert!(s.factory.operation_canceled(&id));
}

/// Independent reconstruction of the `FIAT_INVEST` payload — deliberately built
/// with naive appends rather than the contract's buffer layout, so an off-by-one
/// in `build_fiat_invest_message` fails here instead of rejecting every backend
/// signature in production.
fn fiat_msg(
    e: &Env,
    factory_id: &Address,
    id: u32,
    user: &Address,
    holder: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    let mut msg = Bytes::from_slice(e, b"FIAT_INVEST");
    msg.append(&Bytes::from(factory_id.to_string()));
    msg.append(&Bytes::from_slice(e, &id.to_be_bytes()));
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(holder.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

fn sign_fiat_invest(
    e: &Env,
    factory_id: &Address,
    id: u32,
    user: &Address,
    holder: &Address,
    amount: i128,
    nonce: &String,
) -> BytesN<64> {
    let msg = fiat_msg(e, factory_id, id, user, holder, amount, nonce);
    sign_with(e, &signer_key(), &msg)
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
    // No token moves, so the asset is the zero contract id.
    let expected = crate::events::Invested {
        investor: user.clone(),
        operation_id: id,
        asset: Address::from_str(
            &s.e,
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
        ),
        usdc_amount: 440,
        shares_bought: 40,
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));

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

/// AUDIT Factory C9: like `invest`, `fiat_invest` only mints on a started
/// operation, and a rejected call leaves the nonce spendable.
#[test]
fn test_fiat_invest_before_start_fails() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);

    let user = Address::generate(&s.e);
    let holder = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "fiat-early");
    let sig =
        sign_fiat_invest(&s.e, &s.factory_id, id, &user, &holder, 40, &nonce);

    assert_eq!(
        s.factory
            .try_fiat_invest(&id, &40, &user, &holder, &nonce, &sig),
        Err(Ok(Error::OperationNotStarted.into()))
    );
    assert_eq!(oplend::Client::new(&s.e, &op_addr).balance(&holder), 0);
    assert_eq!(s.factory.funding_progress(&id), 0);

    s.factory.start_operation(&id);
    s.factory
        .fiat_invest(&id, &40, &user, &holder, &nonce, &sig);
    assert_eq!(s.factory.funding_progress(&id), 40);
}

/// AUDIT Factory C4: USDC purchases are refused while USDC trades outside
/// [0.98, 1.02] USD (bounds included as on-peg). Fiat investments, settled in
/// euros off-chain, are not affected.
#[test]
fn test_invest_refused_while_usdc_off_peg() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let try_buy = |nonce: &str| {
        let nonce = String::from_str(&s.e, nonce);
        let sig = sign_invest(&s.e, &s.factory_id, id, &user, 10, &nonce);
        s.factory.try_invest(&user, &id, &10, &nonce, &sig).err()
    };

    // 14-decimal prices: 0.88 (March 2023), just under 0.98, just over 1.02.
    for (i, price) in [
        88_000_000_000_000_i128,
        97_999_900_000_000,
        102_000_100_000_000,
    ]
    .into_iter()
    .enumerate()
    {
        s.stablecoin_oracle.set_price(&price);
        assert_eq!(
            try_buy(&std::format!("off-{i}")),
            Some(Ok(Error::UsdcOffPeg.into()))
        );
    }
    assert_eq!(s.factory.funding_progress(&id), 0);

    // Fiat still goes through during the depeg.
    let holder = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "fiat");
    let sig =
        sign_fiat_invest(&s.e, &s.factory_id, id, &user, &holder, 40, &nonce);
    s.factory
        .fiat_invest(&id, &40, &user, &holder, &nonce, &sig);

    // The band edges are on-peg.
    for (i, price) in [98_000_000_000_000_i128, 102_000_000_000_000]
        .into_iter()
        .enumerate()
    {
        s.stablecoin_oracle.set_price(&price);
        assert_eq!(try_buy(&std::format!("edge-{i}")), None);
    }
    assert_eq!(s.factory.funding_progress(&id), 60);
}

/// AUDIT Factory C5: an off-chain euro refund is mirrored with `fiat_refund`,
/// which burns the custodied shares and puts them back on sale. Like
/// `refund_user`, it is bounded by the holder's balance and refused once the
/// operation is finished.
#[test]
fn test_fiat_refund_burns_custodied_shares_and_reopens_the_sale() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 100);
    s.factory.start_operation(&id);
    let op_token = oplend::Client::new(&s.e, &op_addr);

    // A euro buyer takes 60 shares; they sit with the holder.
    let fiat_user = Address::generate(&s.e);
    let holder = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "fiat");
    let sig = sign_fiat_invest(
        &s.e,
        &s.factory_id,
        id,
        &fiat_user,
        &holder,
        60,
        &nonce,
    );
    s.factory
        .fiat_invest(&id, &60, &fiat_user, &holder, &nonce, &sig);

    s.factory.fiat_refund(&id, &fiat_user, &holder, &40);

    let expected = crate::events::RefundedFiat {
        investor: fiat_user.clone(),
        oplend_holder: holder.clone(),
        operation_id: id,
        shares_refunded: 40,
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));

    assert_eq!(op_token.balance(&holder), 20);
    assert_eq!(op_token.total_supply(), 20);
    assert_eq!(s.factory.funding_progress(&id), 20);

    // No more than the holder has left.
    assert_eq!(
        s.factory
            .try_fiat_refund(&id, &fiat_user, &holder, &21)
            .err(),
        Some(Ok(Error::InsufficientShareBalance.into()))
    );
    assert_eq!(
        s.factory
            .try_fiat_refund(&id, &fiat_user, &holder, &0)
            .err(),
        Some(Ok(Error::InvalidShares.into()))
    );

    // Admin only.
    s.e.set_auths(&[]);
    assert!(s
        .factory
        .try_fiat_refund(&id, &fiat_user, &holder, &10)
        .is_err());
    assert_eq!(op_token.balance(&holder), 20);
    s.e.mock_all_auths();

    // The refunded shares are on sale again: a USDC buyer takes the remaining
    // 80, which finishes the operation, and a finished operation is not
    // refunded.
    let usdc_user = Address::generate(&s.e);
    s.usdc.mint(&usdc_user, &10_000);
    invest_as(&s, id, &usdc_user, 80, "usdc");
    assert!(s.factory.is_operation_finished(&id));
    assert_eq!(
        s.factory
            .try_fiat_refund(&id, &fiat_user, &holder, &10)
            .err(),
        Some(Ok(Error::OperationFinished.into()))
    );
    assert_eq!(op_token.balance(&holder), 20);
}

/// A finished operation is settled: `refund_user`, `batch_refund_users` and
/// `cancel_operation` all fail with `OperationFinished`, and nothing moves.
#[test]
fn test_finished_operation_cannot_be_refunded_or_canceled() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 100);
    s.factory.start_operation(&id);
    let op_token = oplend::Client::new(&s.e, &op_addr);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    invest_as(&s, id, &user, 100, "n1"); // the whole sale, for 1100
    assert!(s.factory.is_operation_finished(&id));

    let finished = Some(Ok(Error::OperationFinished.into()));
    assert_eq!(
        s.factory.try_refund_user(&id, &user, &100, &1100, &0).err(),
        finished
    );
    let batch = soroban_sdk::vec![
        &s.e,
        RefundEntry {
            user: user.clone(),
            shares_to_burn: 100,
            usdc_to_refund: 1100,
            eurc_to_refund: 0,
        }
    ];
    assert_eq!(
        s.factory.try_batch_refund_users(&id, &batch).err(),
        finished
    );
    assert_eq!(s.factory.try_cancel_operation(&id).err(), finished);

    assert!(!s.factory.operation_canceled(&id));
    assert_eq!(op_token.balance(&user), 100);
    assert_eq!(usdc_tok.balance(&user), 10_000 - 1100);
    assert_eq!(s.factory.funding_progress(&id), 100);
    assert_eq!(s.factory.usdc_raised(&id), 1100);
}

// --- AUDIT Factory T1: missing tests ---
// (A purchase whose USDC cost rounds to zero is covered by
// `test_order_rounding_to_zero_fails`.)

/// A rate with many decimals: 1.08345678912345 USD/EUR normalizes to 1.083456,
/// and what `invest` charges is exactly what `get_amount_in` quotes. Converting
/// a cost back to shares rounds down, never in the buyer's favour. Expected
/// values computed independently of the contract.
#[test]
fn test_non_round_rate_is_charged_as_quoted() {
    let s = setup();
    s.oracle.set_price(&108_345_678_912_345);
    let (id, _) = create_op(&s, 10_000_000);
    s.factory.start_operation(&id);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);

    // One whole share at 1 EUR: 1.083456 USDC = 10_834_560 base units.
    assert_eq!(s.factory.get_amount_in(&id, &1_000_000), 10_834_560);
    assert_eq!(s.factory.get_amount_out(&id, &10_834_560), 1_000_000);
    // 0.333333 share: the cost truncates, and so does the way back.
    assert_eq!(s.factory.get_amount_in(&id, &333_333), 3_611_516);
    assert_eq!(s.factory.get_amount_out(&id, &3_611_516), 333_332);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &100_000_000);
    invest_as(&s, id, &user, 1_000_000, "whole");
    invest_as(&s, id, &user, 333_333, "third");
    assert_eq!(
        usdc_tok.balance(&user),
        100_000_000 - 10_834_560 - 3_611_516
    );
    assert_eq!(s.factory.usdc_raised(&id), 10_834_560 + 3_611_516);
}

/// Every way the EUR/USD feed can fail stops all three purchase paths and the
/// USDC quotes; a stablecoin feed failure stops `invest` and `invest_eur` (the
/// paths with a depeg check). The EURC quote reads no feed. A price exactly
/// 600 s old is still accepted.
#[test]
fn test_oracle_failures_block_purchases_and_quotes() {
    type Break = fn(&MockFeedClient, &Symbol);
    let failures: [(Break, Error); 4] = [
        (|f, _| f.remove_price(), Error::OracleNoPrice),
        (|f, s| f.set_price_of(s, &0), Error::OracleInvalidPrice),
        (|f, s| f.set_price_of(s, &-1), Error::OracleInvalidPrice),
        (|f, _| f.set_age(&601), Error::OracleStalePrice),
    ];

    for (break_feed, expected) in failures {
        let s = setup();
        let (id, _) = create_op(&s, 1000);
        s.factory.start_operation(&id);
        let user = Address::generate(&s.e);
        let holder = Address::generate(&s.e);
        s.usdc.mint(&user, &10_000);
        s.eurc.mint(&user, &10_000);
        let expected = Some(Ok(expected.into()));
        let try_invest = |nonce: &str| {
            let nonce = String::from_str(&s.e, nonce);
            let sig = sign_invest(&s.e, &s.factory_id, id, &user, 10, &nonce);
            s.factory.try_invest(&user, &id, &10, &nonce, &sig).err()
        };
        let try_invest_eur = |nonce: &str| {
            let nonce = String::from_str(&s.e, nonce);
            let sig = sign_invest(&s.e, &s.factory_id, id, &user, 10, &nonce);
            s.factory
                .try_invest_eur(&user, &id, &10, &nonce, &sig)
                .err()
        };

        break_feed(&s.oracle, &symbol_short!("EUR"));
        assert_eq!(try_invest("eur"), expected);
        assert_eq!(try_invest_eur("eur-e"), expected);
        let nonce = String::from_str(&s.e, "fiat");
        let sig = sign_fiat_invest(
            &s.e,
            &s.factory_id,
            id,
            &user,
            &holder,
            10,
            &nonce,
        );
        assert_eq!(
            s.factory
                .try_fiat_invest(&id, &10, &user, &holder, &nonce, &sig)
                .err(),
            expected
        );
        assert_eq!(s.factory.try_get_amount_in(&id, &10).err(), expected);
        assert_eq!(s.factory.try_get_amount_out(&id, &110).err(), expected);
        assert_eq!(s.factory.get_amount_in_eur(&id, &10), 100);

        // Restore the EUR feed; break the stablecoin one instead.
        s.oracle.set_price(&110_000_000_000_000);
        s.oracle.set_age(&0);
        for coin in [symbol_short!("USDC"), symbol_short!("EURC")] {
            break_feed(&s.stablecoin_oracle, &coin);
        }
        assert_eq!(try_invest("usdc"), expected);
        assert_eq!(try_invest_eur("eurc"), expected);
        assert_eq!(s.factory.get_amount_in(&id, &10), 110);
        assert_eq!(s.factory.funding_progress(&id), 0);
    }

    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.oracle.set_age(&600);
    s.stablecoin_oracle.set_age(&600);
    assert_eq!(s.factory.get_amount_in(&id, &10), 110);
}

/// `invest` only accepts an approval signed by the backend key over exactly
/// this operation, user and amount. A rejected one leaves the nonce unspent.
#[test]
fn test_invest_rejects_tampered_signatures() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    let (other_op, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    s.factory.start_operation(&other_op);
    let user = Address::generate(&s.e);
    let other_user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    s.usdc.mint(&other_user, &10_000);
    let nonce = String::from_str(&s.e, "n");
    let msg = invest_msg(&s.e, &s.factory_id, id, &user, 100, &nonce);
    let sig = sign_with(&s.e, &signer_key(), &msg);
    let forged = sign_with(&s.e, &SigningKey::from_bytes(&[7u8; 32]), &msg);

    // Another key, then the right signature over a different amount, user or
    // operation.
    assert!(s
        .factory
        .try_invest(&user, &id, &100, &nonce, &forged)
        .is_err());
    assert!(s
        .factory
        .try_invest(&user, &id, &101, &nonce, &sig)
        .is_err());
    assert!(s
        .factory
        .try_invest(&other_user, &id, &100, &nonce, &sig)
        .is_err());
    assert!(s
        .factory
        .try_invest(&user, &other_op, &100, &nonce, &sig)
        .is_err());
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.funding_progress(&other_op), 0);

    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(s.factory.funding_progress(&id), 100);
}

/// Same for `fiat_invest`, which also binds the share holder.
#[test]
fn test_fiat_invest_rejects_tampered_signatures() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    let (other_op, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    s.factory.start_operation(&other_op);
    let user = Address::generate(&s.e);
    let holder = Address::generate(&s.e);
    let other = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "n");
    let msg = fiat_msg(&s.e, &s.factory_id, id, &user, &holder, 100, &nonce);
    let sig = sign_with(&s.e, &signer_key(), &msg);
    let forged = sign_with(&s.e, &SigningKey::from_bytes(&[7u8; 32]), &msg);
    let f = &s.factory;

    assert!(f
        .try_fiat_invest(&id, &100, &user, &holder, &nonce, &forged)
        .is_err());
    assert!(f
        .try_fiat_invest(&id, &101, &user, &holder, &nonce, &sig)
        .is_err());
    assert!(f
        .try_fiat_invest(&id, &100, &other, &holder, &nonce, &sig)
        .is_err());
    assert!(f
        .try_fiat_invest(&id, &100, &user, &other, &nonce, &sig)
        .is_err());
    assert!(f
        .try_fiat_invest(&other_op, &100, &user, &holder, &nonce, &sig)
        .is_err());
    assert_eq!(f.funding_progress(&id), 0);
    assert_eq!(f.funding_progress(&other_op), 0);

    f.fiat_invest(&id, &100, &user, &holder, &nonce, &sig);
    assert_eq!(f.funding_progress(&id), 100);
}

/// A started, partly sold operation is canceled: new purchases fail, and
/// `refund_user` / `batch_refund_users` return every buyer's USDC.
#[test]
fn test_refunds_after_cancel() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let op_token = oplend::Client::new(&s.e, &op_addr);

    let buyers = [
        (Address::generate(&s.e), 100_i128, 1100_i128),
        (Address::generate(&s.e), 200, 2200),
        (Address::generate(&s.e), 50, 550),
    ];
    for (i, (user, shares, _)) in buyers.iter().enumerate() {
        s.usdc.mint(user, &10_000);
        invest_as(&s, id, user, *shares, &std::format!("b{i}"));
    }
    assert_eq!(s.factory.funding_progress(&id), 350);

    s.factory.cancel_operation(&id);

    let late = Address::generate(&s.e);
    s.usdc.mint(&late, &10_000);
    let nonce = String::from_str(&s.e, "late");
    let sig = sign_invest(&s.e, &s.factory_id, id, &late, 10, &nonce);
    assert_eq!(
        s.factory.try_invest(&late, &id, &10, &nonce, &sig).err(),
        Some(Ok(Error::OperationCanceled.into()))
    );
    let holder = Address::generate(&s.e);
    let sig =
        sign_fiat_invest(&s.e, &s.factory_id, id, &late, &holder, 10, &nonce);
    assert_eq!(
        s.factory
            .try_fiat_invest(&id, &10, &late, &holder, &nonce, &sig)
            .err(),
        Some(Ok(Error::OperationCanceled.into()))
    );

    let (first, shares, usdc) = &buyers[0];
    s.factory.refund_user(&id, first, shares, usdc, &0);
    let batch = soroban_sdk::vec![
        &s.e,
        RefundEntry {
            user: buyers[1].0.clone(),
            shares_to_burn: buyers[1].1,
            usdc_to_refund: buyers[1].2,
            eurc_to_refund: 0,
        },
        RefundEntry {
            user: buyers[2].0.clone(),
            shares_to_burn: buyers[2].1,
            usdc_to_refund: buyers[2].2,
            eurc_to_refund: 0,
        }
    ];
    s.factory.batch_refund_users(&id, &batch);

    for (user, _, _) in buyers.iter() {
        assert_eq!(usdc_tok.balance(user), 10_000);
        assert_eq!(op_token.balance(user), 0);
    }
    assert_eq!(usdc_tok.balance(&s.factory_id), 0);
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
}

/// The share token's whitelist message: token ‖ user ‖ nonce.
fn whitelist_msg(token: &Address, who: &Address, nonce: &String) -> Bytes {
    let mut msg = Bytes::from(token.to_string());
    msg.append(&Bytes::from(who.to_string()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

/// AUDIT OpLend C4: one `update_backend_signer` on the factory rotates the
/// key everywhere, since share tokens read it from the factory: both the
/// factory and an existing token reject the old key and accept the new one.
#[test]
fn test_backend_key_rotation() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let op_token = oplend::Client::new(&s.e, &op_addr);
    let new_key = SigningKey::from_bytes(&[10u8; 32]);
    let new_pub = BytesN::from_array(&s.e, &new_key.verifying_key().to_bytes());
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);

    s.factory.update_backend_signer(&new_pub);
    assert_eq!(s.factory.backend_signer(), new_pub);
    assert_eq!(op_token.factory(), s.factory_id);

    let nonce = String::from_str(&s.e, "inv");
    let msg = invest_msg(&s.e, &s.factory_id, id, &user, 10, &nonce);
    let old_sig = sign_with(&s.e, &signer_key(), &msg);
    assert!(s
        .factory
        .try_invest(&user, &id, &10, &nonce, &old_sig)
        .is_err());
    s.factory
        .invest(&user, &id, &10, &nonce, &sign_with(&s.e, &new_key, &msg));

    let other = Address::generate(&s.e);
    let nonce = String::from_str(&s.e, "wl");
    let msg = whitelist_msg(&op_addr, &other, &nonce);
    let old_sig = sign_with(&s.e, &signer_key(), &msg);
    assert!(op_token
        .try_whitelist_user(&other, &nonce, &old_sig)
        .is_err());
    op_token.whitelist_user(&other, &nonce, &sign_with(&s.e, &new_key, &msg));
    assert!(op_token.is_whitelisted(&other));
}

/// The op-lend wallet against the real factory (OpLendWallet C3 / C4): it
/// accepts the factory, decodes its `get_operation` record into its own
/// mirror of `Operation`, and reads the backend signer through it.
#[test]
fn test_wallet_reads_operations_and_signer_from_the_factory() {
    let s = setup();
    let (id, op_addr) = create_op(&s, 1000);
    let wallet_id = s.e.register(
        wallet::WASM,
        (Address::generate(&s.e), s.factory_id.clone()),
    );
    let wallet = wallet::Client::new(&s.e, &wallet_id);

    wallet.register_op_lend(&id);
    assert_eq!(wallet.op_lend(&id), op_addr);
    assert_eq!(wallet.backend_signer(), s.factory.backend_signer());
    // An id the factory never created cannot be registered.
    assert!(wallet.try_register_op_lend(&(id + 1)).is_err());
}

// --- EURC ---

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

/// Shares are priced in EUR, so EURC pays the price as is: 100 shares at
/// 1 EUR are 1000 base units of a 7-decimal EURC, with no oracle leg. The
/// `Invested` event still reports USD: the same 1100 a USDC invest charges.
#[test]
fn test_invest_eur_happy_path() {
    let s = setup();
    let eurc_tok = soroban_sdk::token::Client::new(&s.e, &s.eurc_addr);
    let (id, op_addr) = create_op(&s, 1000);
    let user = Address::generate(&s.e);
    s.eurc.mint(&user, &10_000);
    assert_eq!(s.factory.eurc(), s.eurc_addr);

    // Like every funding path (AUDIT Factory C9), not before the start.
    let nonce = String::from_str(&s.e, "e1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    assert_eq!(
        s.factory
            .try_invest_eur(&user, &id, &100, &nonce, &sig)
            .err(),
        Some(Ok(Error::OperationNotStarted.into()))
    );
    s.factory.start_operation(&id);

    assert_eq!(s.factory.get_amount_in_eur(&id, &100), 1000);
    s.factory.invest_eur(&user, &id, &100, &nonce, &sig);
    let expected = crate::events::Invested {
        investor: user.clone(),
        operation_id: id,
        asset: s.eurc_addr.clone(),
        usdc_amount: 1100,
        shares_bought: 100,
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));

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

/// The USDC depeg check (AUDIT Factory C4) guards USDC purchases only: an
/// EURC purchase moves no USDC.
#[test]
fn test_invest_eur_ignores_the_usdc_peg() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.eurc.mint(&user, &10_000);

    s.stablecoin_oracle.set_price(&90_000_000_000_000); // USDC at 0.90
    invest_eur_as(&s, id, &user, 100, "e1");
    assert_eq!(s.factory.eurc_raised(&id), 1000);
}

/// Like USDC (AUDIT Factory C4), EURC is refused while it trades outside
/// [0.98, 1.02] EUR (bounds included as on-peg), measured as its USD price
/// over EUR/USD. USDC purchases are not affected, and EURC following a moved
/// EUR/USD rate is no depeg.
#[test]
fn test_invest_eur_refused_while_eurc_off_peg() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    s.eurc.mint(&user, &10_000);
    let eurc = symbol_short!("EURC");
    let try_buy = |nonce: &str| {
        let nonce = String::from_str(&s.e, nonce);
        let sig = sign_invest(&s.e, &s.factory_id, id, &user, 10, &nonce);
        s.factory
            .try_invest_eur(&user, &id, &10, &nonce, &sig)
            .err()
    };

    // EUR/USD is 1.1, so EURC at 1.0758 / 1.1222 USD is 0.978 / 1.0202 EUR.
    for (i, price) in [107_580_000_000_000_i128, 112_220_000_000_000]
        .into_iter()
        .enumerate()
    {
        s.stablecoin_oracle.set_price_of(&eurc, &price);
        assert_eq!(
            try_buy(&std::format!("off-{i}")),
            Some(Ok(Error::EurcOffPeg.into()))
        );
    }
    assert_eq!(s.factory.funding_progress(&id), 0);

    // USDC still sells during an EURC depeg.
    invest_as(&s, id, &user, 10, "usdc");

    // The band edges are on-peg: 1.078 and 1.122 USD are 0.98 and 1.02 EUR.
    for (i, price) in [107_800_000_000_000_i128, 112_200_000_000_000]
        .into_iter()
        .enumerate()
    {
        s.stablecoin_oracle.set_price_of(&eurc, &price);
        assert_eq!(try_buy(&std::format!("edge-{i}")), None);
    }

    // EUR/USD moves to 1.2 and EURC with it: still 1 EUR.
    s.oracle.set_price(&120_000_000_000_000);
    s.stablecoin_oracle
        .set_price_of(&eurc, &120_000_000_000_000);
    assert_eq!(try_buy("moved"), None);

    assert_eq!(s.factory.funding_progress(&id), 40);
    assert_eq!(s.factory.eurc_raised(&id), 300);
}

/// `invest_eur` takes the existing `ONCHAIN_INVEST` signature, so one backend
/// authorization pays in either token, once.
#[test]
fn test_invest_eur_shares_nonces_with_invest() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    s.eurc.mint(&user, &10_000);

    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    assert_eq!(
        s.factory.try_invest_eur(&user, &id, &100, &nonce, &sig),
        Err(Ok(Error::NonceAlreadyUsed.into()))
    );
}

/// `withdraw_funds` sends each coin of a mixed raise as itself. An EURC-only
/// withdrawal counts as a withdrawal of the operation's funds.
#[test]
fn test_withdraw_mixed_raise_sends_both_tokens() {
    let s = setup();
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let eurc_tok = soroban_sdk::token::Client::new(&s.e, &s.eurc_addr);
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &100_000);
    s.eurc.mint(&user, &100_000);
    let dest = Address::generate(&s.e);
    let withdrawn = |usdc_amount: i128, eurc_amount: i128| {
        let expected = crate::events::FundingWithdrawn {
            operation_id: id,
            usdc_amount,
            eurc_amount,
        }
        .to_xdr(&s.e, &s.factory_id);
        let events = s.e.events().all().filter_by_contract(&s.factory_id);
        events.events().contains(&expected)
    };

    invest_eur_as(&s, id, &user, 100, "e1");
    s.factory.withdraw_funds(&id, &dest);
    assert!(withdrawn(0, 1000));
    assert_eq!(eurc_tok.balance(&dest), 1000);
    assert!(s.factory.funds_withdrawn(&id));
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 0);

    invest_as(&s, id, &user, 100, "u1");
    invest_eur_as(&s, id, &user, 50, "e2");
    s.factory.withdraw_funds(&id, &dest);
    assert!(withdrawn(1100, 500));
    assert_eq!(usdc_tok.balance(&dest), 1100);
    assert_eq!(eurc_tok.balance(&dest), 1500);
    assert_eq!(s.factory.usdc_withdrawn_amount(&id), 1100);
    assert_eq!(s.factory.eurc_withdrawn_amount(&id), 1500);
    assert!(s.factory.funds_withdrawn(&id));
    assert_eq!(usdc_tok.balance(&s.factory_id), 0);
    assert_eq!(eurc_tok.balance(&s.factory_id), 0);
    assert_eq!(
        s.factory.try_withdraw_funds(&id, &dest),
        Err(Ok(Error::NothingToWithdraw.into()))
    );
}

/// Refunds pay the USDC and EURC amounts the admin decides (AUDIT Factory
/// C1), each as itself: a moved EUR/USD rate changes nothing, `Refunded`
/// reports both raw amounts, and the `invested` records are left as they were.
#[test]
fn test_refund_mixed_raise_returns_each_token() {
    let s = setup();
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let eurc_tok = soroban_sdk::token::Client::new(&s.e, &s.eurc_addr);
    let (id, op_addr) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    let usdc_only = Address::generate(&s.e);
    let eurc_only = Address::generate(&s.e);
    let both = Address::generate(&s.e);
    let users = [&usdc_only, &eurc_only, &both];
    for u in users {
        s.usdc.mint(u, &10_000);
        s.eurc.mint(u, &10_000);
    }

    invest_as(&s, id, &usdc_only, 100, "u1"); // 1100 USDC
    invest_eur_as(&s, id, &eurc_only, 100, "e1"); // 1000 EURC
    invest_as(&s, id, &both, 10, "u2"); // 110 USDC
    invest_eur_as(&s, id, &both, 20, "e2"); // 200 EURC

    s.oracle.set_price(&120_000_000_000_000); // EUR/USD 1.2
    s.factory.refund_user(&id, &both, &30, &110, &200);
    let expected = crate::events::Refunded {
        investor: both.clone(),
        operation_id: id,
        usdc_amount: 110,
        eurc_amount: 200,
        shares_refunded: 30,
    }
    .to_xdr(&s.e, &s.factory_id);
    let events = s.e.events().all().filter_by_contract(&s.factory_id);
    assert!(events.events().contains(&expected));

    let entry = |user: &Address, shares, usdc, eurc| RefundEntry {
        user: user.clone(),
        shares_to_burn: shares,
        usdc_to_refund: usdc,
        eurc_to_refund: eurc,
    };
    let batch = soroban_sdk::vec![
        &s.e,
        entry(&eurc_only, 100, 0, 1000),
        entry(&usdc_only, 100, 1100, 0)
    ];
    s.factory.batch_refund_users(&id, &batch);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    for u in users {
        assert_eq!(op_token.balance(u), 0);
        assert_eq!(usdc_tok.balance(u), 10_000);
        assert_eq!(eurc_tok.balance(u), 10_000);
    }
    assert_eq!(s.factory.eurc_raised_per_client(&id, &eurc_only), 1000);
    assert_eq!(s.factory.eurc_raised_per_client(&id, &both), 200);
    assert_eq!(s.factory.funding_progress(&id), 0);
    assert_eq!(s.factory.usdc_raised(&id), 0);
    assert_eq!(s.factory.eurc_raised(&id), 0);
    assert_eq!(usdc_tok.balance(&s.factory_id), 0);
    assert_eq!(eurc_tok.balance(&s.factory_id), 0);
}

/// EURC of every operation sits in one balance, exactly like USDC, and falls
/// back on the admin's EURC the same way.
#[test]
fn test_eurc_refund_after_withdraw_cannot_drain_other_operation() {
    let s = setup();
    let eurc_tok = soroban_sdk::token::Client::new(&s.e, &s.eurc_addr);
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    let (a, _) = create_op(&s, 1000);
    let (b, _) = create_op(&s, 1000);
    s.factory.start_operation(&a);
    s.factory.start_operation(&b);
    let early = Address::generate(&s.e);
    let late = Address::generate(&s.e);
    let other = Address::generate(&s.e);
    for u in [&early, &late, &other] {
        s.eurc.mint(u, &10_000);
    }

    invest_eur_as(&s, a, &early, 100, "a1");
    s.factory.withdraw_funds(&a, &Address::generate(&s.e));
    invest_eur_as(&s, a, &late, 10, "a2");
    invest_eur_as(&s, b, &other, 100, "b1");

    // The admin's USDC does not cover an EURC refund.
    s.usdc.mint(&s.admin, &10_000);
    assert_eq!(
        s.factory.try_refund_user(&a, &early, &100, &0, &1000),
        Err(Ok(Error::InsufficientEscrow.into()))
    );
    s.eurc.mint(&s.admin, &1000);
    s.factory.refund_user(&a, &early, &100, &0, &1000);
    assert_eq!(eurc_tok.balance(&early), 10_000);
    assert_eq!(eurc_tok.balance(&s.admin), 0);
    assert_eq!(usdc_tok.balance(&s.admin), 10_000);
    assert_eq!(s.factory.eurc_raised(&a), 1100);

    // EURC invested after the withdrawal is still escrowed and refundable.
    s.factory.refund_user(&a, &late, &10, &0, &100);
    assert_eq!(eurc_tok.balance(&late), 10_000);
    assert_eq!(eurc_tok.balance(&s.factory_id), 1000); // operation b, intact
}
