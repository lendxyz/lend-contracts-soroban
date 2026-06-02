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

    let signer = BytesN::from_array(&e, &signer_key().verifying_key().to_bytes());
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
    // 100 shares * 1 EUR * 1.1 USD = 110 USDC units
    assert_eq!(s.factory.get_amount_in(&id, &100), 110);
    // inverse-ish
    assert!(s.factory.get_amount_out(&id, &110) > 0);
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
    assert_eq!(s.factory.usdc_raised(&id), 110);
    assert_eq!(s.factory.usdc_raised_per_client(&id, &user), 110);

    let op_token = oplend::Client::new(&s.e, &op_addr);
    assert_eq!(op_token.balance(&user), 100);
    // user paid 110 USDC
    let usdc_tok = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc_tok.balance(&user), 9_890);
    assert_eq!(usdc_tok.balance(&s.factory_id), 110);
}

#[test]
#[should_panic(expected = "operation not started")]
fn test_invest_before_start_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
}

#[test]
#[should_panic(expected = "nonce already used")]
fn test_invest_nonce_replay_fails() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
}

#[test]
#[should_panic(expected = "user is blacklisted")]
fn test_blacklisted_user_cannot_invest() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    s.factory.blacklist(&user, &true);

    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
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
    assert_eq!(usdc_tok.balance(&dest), 110);
    assert!(s.factory.usdc_withdrawn(&id));
}

#[test]
#[should_panic(expected = "operation canceled")]
fn test_canceled_blocks_invest() {
    let s = setup();
    let (id, _) = create_op(&s, 1000);
    s.factory.start_operation(&id);
    s.factory.cancel_operation(&id);

    let user = Address::generate(&s.e);
    s.usdc.mint(&user, &10_000);
    let nonce = String::from_str(&s.e, "n1");
    let sig = sign_invest(&s.e, &s.factory_id, id, &user, 100, &nonce);
    s.factory.invest(&user, &id, &100, &nonce, &sig);
}

#[test]
#[should_panic(expected = "operation does not exist")]
fn test_get_operation_nonexistent() {
    let s = setup();
    s.factory.get_operation(&99);
}
