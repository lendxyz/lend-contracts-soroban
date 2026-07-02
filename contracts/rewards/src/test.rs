#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _, token::StellarAssetClient, vec, Address, Bytes,
    BytesN, Env, Vec,
};

use crate::contract::{LendRewards, LendRewardsClient};
use crate::merkle;
use crate::storage_types::ClaimData;

const OP_ID: u32 = 2;
const EPOCH: u32 = 1;

struct Setup<'a> {
    e: Env,
    admin: Address,
    rewards: LendRewardsClient<'a>,
    rewards_id: Address,
    usdc: StellarAssetClient<'a>,
    usdc_addr: Address,
}

fn setup<'a>() -> Setup<'a> {
    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let issuer = Address::generate(&e);
    let usdc_sac = e.register_stellar_asset_contract_v2(issuer);
    let usdc_addr = usdc_sac.address();
    let usdc = StellarAssetClient::new(&e, &usdc_addr);

    let rewards_id =
        e.register(LendRewards, (admin.clone(), usdc_addr.clone()));
    let rewards = LendRewardsClient::new(&e, &rewards_id);

    Setup {
        e,
        admin,
        rewards,
        rewards_id,
        usdc,
        usdc_addr,
    }
}

/// Sorted-pair keccak256 — mirrors merkle::hash_pair (which is private).
fn pair(e: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let (lo, hi) = if a.to_array() <= b.to_array() {
        (a, b)
    } else {
        (b, a)
    };
    let mut buf = Bytes::from_slice(e, &lo.to_array());
    buf.append(&Bytes::from_slice(e, &hi.to_array()));
    e.crypto().keccak256(&buf).to_bytes()
}

/// Builds a 2-leaf tree; returns (root, proof_for_index_0, proof_for_index_1).
fn tree2(
    e: &Env,
    u0: &Address,
    b0: i128,
    u1: &Address,
    b1: i128,
) -> (BytesN<32>, Vec<BytesN<32>>, Vec<BytesN<32>>) {
    let l0 = merkle::leaf(e, u0, b0);
    let l1 = merkle::leaf(e, u1, b1);
    let root = pair(e, &l0, &l1);
    (root, vec![e, l1.clone()], vec![e, l0.clone()])
}

#[test]
fn test_distribute_and_claim_op() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (b0, b1) = (100i128, 200i128);
    let (root, proof0, _) = tree2(&s.e, &u0, b0, &u1, b1);

    s.usdc.mint(&s.admin, &(b0 + b1));
    s.rewards
        .distribute_op_rewards(&OP_ID, &EPOCH, &root, &(b0 + b1));
    let usdc = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc.balance(&s.rewards_id), b0 + b1);

    s.rewards.claim_op_epoch(&OP_ID, &u0, &EPOCH, &b0, &proof0);
    assert!(s.rewards.op_claimed(&OP_ID, &EPOCH, &u0));
    assert_eq!(usdc.balance(&u0), b0);
}

#[test]
#[should_panic(expected = "cannot rewrite merkle root")]
fn test_cannot_rewrite_root() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (root, _, _) = tree2(&s.e, &u0, 100, &u1, 200);
    s.usdc.mint(&s.admin, &600);
    s.rewards.distribute_op_rewards(&OP_ID, &EPOCH, &root, &300);
    s.rewards.distribute_op_rewards(&OP_ID, &EPOCH, &root, &300);
}

#[test]
#[should_panic(expected = "epoch already claimed for this user")]
fn test_cannot_claim_op_twice() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (b0, b1) = (100i128, 200i128);
    let (root, proof0, _) = tree2(&s.e, &u0, b0, &u1, b1);
    s.usdc.mint(&s.admin, &(b0 + b1));
    s.rewards
        .distribute_op_rewards(&OP_ID, &EPOCH, &root, &(b0 + b1));

    s.rewards.claim_op_epoch(&OP_ID, &u0, &EPOCH, &b0, &proof0);
    s.rewards.claim_op_epoch(&OP_ID, &u0, &EPOCH, &b0, &proof0);
}

#[test]
#[should_panic(expected = "Incorrect merkle proof")]
fn test_cannot_claim_wrong_balance() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (b0, b1) = (100i128, 200i128);
    let (root, proof0, _) = tree2(&s.e, &u0, b0, &u1, b1);
    s.usdc.mint(&s.admin, &(b0 + b1));
    s.rewards
        .distribute_op_rewards(&OP_ID, &EPOCH, &root, &(b0 + b1));

    // claim a different balance than the leaf encodes
    s.rewards
        .claim_op_epoch(&OP_ID, &u0, &EPOCH, &(b0 + 1), &proof0);
}

#[test]
fn test_claim_multiple_op_epochs() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (b0, b1) = (100i128, 200i128);
    let (root, proof0, _) = tree2(&s.e, &u0, b0, &u1, b1);

    s.usdc.mint(&s.admin, &((b0 + b1) * 3));
    for epoch in 1..=3u32 {
        s.rewards
            .distribute_op_rewards(&OP_ID, &epoch, &root, &(b0 + b1));
    }

    let claims = vec![
        &s.e,
        ClaimData {
            epoch: 1,
            balance: b0,
            merkle_proof: proof0.clone(),
        },
        ClaimData {
            epoch: 2,
            balance: b0,
            merkle_proof: proof0.clone(),
        },
        ClaimData {
            epoch: 3,
            balance: b0,
            merkle_proof: proof0.clone(),
        },
    ];
    s.rewards.claim_op_epochs(&OP_ID, &u0, &claims);

    let usdc = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    assert_eq!(usdc.balance(&u0), b0 * 3);
    assert!(s.rewards.op_claimed(&OP_ID, &1, &u0));
    assert!(s.rewards.op_claimed(&OP_ID, &2, &u0));
    assert!(s.rewards.op_claimed(&OP_ID, &3, &u0));
}

#[test]
fn test_verify_helpers() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (b0, b1) = (100i128, 200i128);
    let (root, proof0, _) = tree2(&s.e, &u0, b0, &u1, b1);
    s.usdc.mint(&s.admin, &(b0 + b1));
    s.rewards
        .distribute_op_rewards(&OP_ID, &EPOCH, &root, &(b0 + b1));

    assert!(s.rewards.verify_op_claim(&OP_ID, &u0, &EPOCH, &b0, &proof0));
    assert!(!s.rewards.verify_op_claim(
        &OP_ID,
        &u0,
        &EPOCH,
        &(b0 + 1),
        &proof0
    ));
}

#[test]
fn test_emergency_withdraw() {
    let s = setup();
    let other_issuer = Address::generate(&s.e);
    let other_sac = s.e.register_stellar_asset_contract_v2(other_issuer);
    let other_addr = other_sac.address();
    let other = StellarAssetClient::new(&s.e, &other_addr);

    other.mint(&s.rewards_id, &500);
    s.rewards.emergency_withdraw(&other_addr);

    let other_tok = soroban_sdk::token::Client::new(&s.e, &other_addr);
    assert_eq!(other_tok.balance(&s.rewards_id), 0);
    assert_eq!(other_tok.balance(&s.admin), 500);
}

#[test]
#[should_panic(expected = "cannot emergency withdraw reward token")]
fn test_emergency_withdraw_reward_token_fails() {
    let s = setup();
    s.rewards.emergency_withdraw(&s.usdc_addr);
}

#[test]
fn test_set_reward_token_and_admin() {
    let s = setup();
    let new_token = Address::generate(&s.e);
    s.rewards.set_reward_token(&new_token);
    assert_eq!(s.rewards.reward_token(), new_token);

    let new_admin = Address::generate(&s.e);
    s.rewards.set_admin(&new_admin);
    // new admin can set reward token back
    s.rewards.set_reward_token(&s.usdc_addr);
    assert_eq!(s.rewards.reward_token(), s.usdc_addr);
}
