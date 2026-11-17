#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Deployer as _},
    token::StellarAssetClient,
    vec, Address, Bytes, BytesN, Env, Vec,
};

use crate::contract::{LendRewards, LendRewardsClient};
use crate::errors::Error;
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

/// Any other wasm, as an upgrade target.
const OTHER_WASM: &[u8] =
    include_bytes!("../../../target/wasm32v1-none/release/lend_wallet.wasm");

/// `upgrade` extends the TTL after the swap: instance TTL extension also covers
/// the code entry, so the *new* wasm is bumped along with the instance.
#[test]
fn test_upgrade_extends_the_new_code_ttl() {
    let s = setup();
    let d = s.e.deployer();
    let before = d.get_contract_instance_ttl(&s.rewards_id);

    let new_hash = d.upload_contract_wasm(OTHER_WASM);
    s.rewards.upgrade(&new_hash);

    let code_ttl = d.get_contract_code_ttl(&s.rewards_id);
    assert!(code_ttl > before);
    assert_eq!(code_ttl, d.get_contract_instance_ttl(&s.rewards_id));
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
fn test_cannot_rewrite_root() {
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let (root, _, _) = tree2(&s.e, &u0, 100, &u1, 200);
    s.usdc.mint(&s.admin, &600);
    s.rewards.distribute_op_rewards(&OP_ID, &EPOCH, &root, &300);
    assert_eq!(
        s.rewards
            .try_distribute_op_rewards(&OP_ID, &EPOCH, &root, &300),
        Err(Ok(Error::RootAlreadySet.into()))
    );
}

#[test]
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
    assert_eq!(
        s.rewards
            .try_claim_op_epoch(&OP_ID, &u0, &EPOCH, &b0, &proof0),
        Err(Ok(Error::AlreadyClaimed.into()))
    );
}

#[test]
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
    assert_eq!(
        s.rewards
            .try_claim_op_epoch(&OP_ID, &u0, &EPOCH, &(b0 + 1), &proof0),
        Err(Ok(Error::InvalidProof.into()))
    );
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

/// AUDIT Rewards C1: a negative entry in one epoch rejects the whole batch
/// instead of being netted against the others, which stay claimable.
#[test]
fn test_claim_op_epochs_rejects_a_negative_epoch() {
    let s = setup();
    let alice = Address::generate(&s.e);
    let bob = Address::generate(&s.e);
    let usdc = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    s.usdc.mint(&s.admin, &1_000);

    // Alice: 100 in epoch 1, 50 in epoch 2, -80 in epoch 3 (the mistake).
    let mut claims = Vec::new(&s.e);
    for (epoch, balance) in [(1u32, 100i128), (2, 50), (3, -80)] {
        let (root, proof, _) = tree2(&s.e, &alice, balance, &bob, 10);
        s.rewards.distribute_op_rewards(&OP_ID, &epoch, &root, &110);
        claims.push_back(ClaimData {
            epoch,
            balance,
            merkle_proof: proof,
        });
    }

    assert_eq!(
        s.rewards.try_claim_op_epochs(&OP_ID, &alice, &claims),
        Err(Ok(Error::InvalidClaimBalance.into()))
    );
    assert_eq!(usdc.balance(&alice), 0);
    assert!(!s.rewards.op_claimed(&OP_ID, &1, &alice));
    assert!(!s.rewards.op_claimed(&OP_ID, &2, &alice));

    // Without the bad epoch she gets the full 150.
    claims.pop_back();
    s.rewards.claim_op_epochs(&OP_ID, &alice, &claims);
    assert_eq!(usdc.balance(&alice), 150);

    // The single-epoch path refuses it too.
    let (_, proof, _) = tree2(&s.e, &alice, -80, &bob, 10);
    assert_eq!(
        s.rewards
            .try_claim_op_epoch(&OP_ID, &alice, &3, &-80, &proof),
        Err(Ok(Error::InvalidClaimBalance.into()))
    );
}

/// AUDIT Rewards T1: claims are kept apart per operation and per user. Two
/// users each claim epoch 1 of two operations; every claim pays its own amount,
/// and neither a first claim nor another operation's proof gets in the way.
#[test]
fn test_claims_are_separate_per_operation_and_user() {
    const OP_A: u32 = 2;
    const OP_B: u32 = 3;
    let s = setup();
    let u0 = Address::generate(&s.e);
    let u1 = Address::generate(&s.e);
    let usdc = soroban_sdk::token::Client::new(&s.e, &s.usdc_addr);
    s.usdc.mint(&s.admin, &1_000);

    // Different amounts per operation, so a mix-up would show.
    let (root_a, a0, a1) = tree2(&s.e, &u0, 100, &u1, 200);
    let (root_b, b0, b1) = tree2(&s.e, &u0, 30, &u1, 40);
    s.rewards
        .distribute_op_rewards(&OP_A, &EPOCH, &root_a, &300);
    s.rewards.distribute_op_rewards(&OP_B, &EPOCH, &root_b, &70);

    // An operation-A proof is no good on operation B.
    assert_eq!(
        s.rewards.try_claim_op_epoch(&OP_B, &u0, &EPOCH, &100, &a0),
        Err(Ok(Error::InvalidProof.into()))
    );

    s.rewards.claim_op_epoch(&OP_A, &u0, &EPOCH, &100, &a0);
    s.rewards.claim_op_epoch(&OP_B, &u0, &EPOCH, &30, &b0);
    s.rewards.claim_op_epoch(&OP_A, &u1, &EPOCH, &200, &a1);
    s.rewards.claim_op_epoch(&OP_B, &u1, &EPOCH, &40, &b1);

    assert_eq!(usdc.balance(&u0), 130);
    assert_eq!(usdc.balance(&u1), 240);
    assert_eq!(usdc.balance(&s.rewards_id), 0);
    for op in [OP_A, OP_B] {
        assert!(s.rewards.op_claimed(&op, &EPOCH, &u0));
        assert!(s.rewards.op_claimed(&op, &EPOCH, &u1));
    }

    // Each (operation, epoch, user) still pays only once.
    assert_eq!(
        s.rewards.try_claim_op_epoch(&OP_A, &u0, &EPOCH, &100, &a0),
        Err(Ok(Error::AlreadyClaimed.into()))
    );
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
fn test_emergency_withdraw_reward_token_fails() {
    let s = setup();
    assert_eq!(
        s.rewards.try_emergency_withdraw(&s.usdc_addr),
        Err(Ok(Error::RewardTokenNotWithdrawable.into()))
    );
}

#[test]
fn test_set_admin_hands_over_the_admin_paths() {
    let s = setup();
    let new_admin = Address::generate(&s.e);
    s.rewards.set_admin(&new_admin);

    // Admin calls now need the new admin's auth.
    s.rewards.set_admin(&s.admin);
    assert_eq!(s.e.auths()[0].0, new_admin);
}

/// Leaves and roots must stay byte-compatible with the off-chain builder
/// (`scripts/build-merkle-tree.js`), which hashes the ASCII strkey followed by
/// the 16-byte big-endian balance. This fixture is that script's recorded output
/// (`scripts/merkle.json`), so a change to `merkle::leaf` or the pair ordering
/// breaks here rather than on mainnet.
#[test]
fn test_verifies_offchain_tree_fixture() {
    let s = setup();
    let root = BytesN::from_array(
        &s.e,
        &hex32(
            "77ab6b661e5aef17038cafaf9b5d6dee56a2ecd32cf6340da39413f18b36f963",
        ),
    );
    s.usdc.mint(&s.admin, &4_213_967_000);
    s.rewards
        .distribute_op_rewards(&OP_ID, &EPOCH, &root, &4_213_967_000);

    let user = Address::from_str(
        &s.e,
        "GAIOQM6QINN427MWFQUHJZGG6T6KOE2ZGLRS2DVYIUGUOBSREDHJNTQM",
    );
    let proof = vec![
        &s.e,
        BytesN::from_array(
            &s.e,
            &hex32("adbe40d74fb873fb743282a63cff57ddee6bcfdb491804a2b1954c373c1efe73"),
        ),
        BytesN::from_array(
            &s.e,
            &hex32("99e06b1d3f324a5e7f71f5a18c52652b45a32046d4f21d208cac116c763a9a64"),
        ),
        BytesN::from_array(
            &s.e,
            &hex32("67c02d42c437b092896b161d50673e700302981763d1e4cb1b987e0641e90fd0"),
        ),
    ];

    assert!(s
        .rewards
        .verify_op_claim(&OP_ID, &user, &EPOCH, &1_000_000, &proof));
    // Same proof, wrong balance => different leaf => no match.
    assert!(!s
        .rewards
        .verify_op_claim(&OP_ID, &user, &EPOCH, &1_000_001, &proof));
}

fn hex32(s: &str) -> [u8; 32] {
    let bytes = s.as_bytes();
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(
            core::str::from_utf8(&bytes[i * 2..i * 2 + 2]).unwrap(),
            16,
        )
        .unwrap();
    }
    out
}
