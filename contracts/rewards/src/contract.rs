use soroban_sdk::{
    contract, contractimpl, panic_with_error, token, Address, BytesN, Env, Vec,
};

use crate::errors::Error;
use crate::events;
use crate::merkle;
use crate::storage_types::{
    ClaimData, DataKey, CLAIM_BUMP_AMOUNT, CLAIM_LIFETIME_THRESHOLD,
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD,
};

#[contract]
pub struct LendRewards;

fn read_admin(e: &Env) -> Address {
    match e.storage().instance().get(&DataKey::Admin) {
        Some(admin) => admin,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

fn require_admin(e: &Env) -> Address {
    let admin = read_admin(e);
    admin.require_auth();
    admin
}

fn read_reward_token(e: &Env) -> Address {
    match e.storage().instance().get(&DataKey::RewardToken) {
        Some(token) => token,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

fn bump_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn read_root(e: &Env, op_id: u32, epoch: u32) -> Option<BytesN<32>> {
    e.storage()
        .persistent()
        .get(&DataKey::OpMerkleRoot(op_id, epoch))
}

fn verify(
    e: &Env,
    op_id: u32,
    user: &Address,
    epoch: u32,
    balance: i128,
    proof: &Vec<BytesN<32>>,
) -> bool {
    match read_root(e, op_id, epoch) {
        Some(root) => {
            merkle::verify(e, proof, &root, merkle::leaf(e, user, balance))
        }
        None => false,
    }
}

fn settle_claim(
    e: &Env,
    op_id: u32,
    user: &Address,
    epoch: u32,
    balance: i128,
    proof: &Vec<BytesN<32>>,
) {
    if !verify(e, op_id, user, epoch, balance, proof) {
        panic_with_error!(e, Error::InvalidProof);
    }

    let persistent = e.storage().persistent();

    persistent.extend_ttl(
        &DataKey::OpMerkleRoot(op_id, epoch),
        CLAIM_LIFETIME_THRESHOLD,
        CLAIM_BUMP_AMOUNT,
    );

    let key = DataKey::OpClaimed(op_id, epoch, user.clone());
    persistent.set(&key, &true);
    persistent.extend_ttl(&key, CLAIM_LIFETIME_THRESHOLD, CLAIM_BUMP_AMOUNT);
}

fn is_claimed(e: &Env, op_id: u32, epoch: u32, user: &Address) -> bool {
    e.storage().persistent().has(&DataKey::OpClaimed(
        op_id,
        epoch,
        user.clone(),
    ))
}

fn transfer_rewards(e: &Env, op_id: u32, user: &Address, balance: i128) {
    token::Client::new(e, &read_reward_token(e)).transfer(
        &e.current_contract_address(),
        user,
        &balance,
    );

    events::Claimed {
        op_id,
        user: user.clone(),
        balance,
    }
    .publish(e);
}

#[contractimpl]
impl LendRewards {
    pub fn __constructor(e: Env, admin: Address, reward_token: Address) {
        e.storage().instance().set(&DataKey::Admin, &admin);
        e.storage()
            .instance()
            .set(&DataKey::RewardToken, &reward_token);
    }

    // ********** Admin: distribution **********

    pub fn distribute_op_rewards(
        e: Env,
        op_id: u32,
        epoch: u32,
        merkle_root: BytesN<32>,
        total_allocation: i128,
    ) {
        let admin = require_admin(&e);
        bump_instance(&e);

        let key = DataKey::OpMerkleRoot(op_id, epoch);

        if e.storage().persistent().has(&key) {
            panic_with_error!(&e, Error::RootAlreadySet);
        }

        token::Client::new(&e, &read_reward_token(&e)).transfer(
            &admin,
            e.current_contract_address(),
            &total_allocation,
        );

        e.storage().persistent().set(&key, &merkle_root);
        e.storage().persistent().extend_ttl(
            &key,
            CLAIM_LIFETIME_THRESHOLD,
            CLAIM_BUMP_AMOUNT,
        );

        events::RewardsDistributed {
            op_id,
            epoch,
            amount: total_allocation,
        }
        .publish(&e);
    }

    // ********** Admin: config **********

    pub fn set_reward_token(e: Env, new_token: Address) {
        require_admin(&e);
        bump_instance(&e);
        e.storage()
            .instance()
            .set(&DataKey::RewardToken, &new_token);
        events::RewardTokenUpdated { new_token }.publish(&e);
    }

    pub fn set_admin(e: Env, new_admin: Address) {
        require_admin(&e);
        bump_instance(&e);
        e.storage().instance().set(&DataKey::Admin, &new_admin);
    }

    /// Withdraw the full balance of any non-reward token to the admin.
    pub fn emergency_withdraw(e: Env, token_addr: Address) {
        let admin = require_admin(&e);
        bump_instance(&e);

        if token_addr == read_reward_token(&e) {
            panic_with_error!(&e, Error::RewardTokenNotWithdrawable);
        }

        let client = token::Client::new(&e, &token_addr);
        let amount = client.balance(&e.current_contract_address());

        if amount > 0 {
            client.transfer(&e.current_contract_address(), &admin, &amount);
        }

        events::EmergencyWithdrawn {
            token: token_addr,
            amount,
        }
        .publish(&e);
    }

    pub fn upgrade(e: Env, new_wasm_hash: BytesN<32>) {
        require_admin(&e);
        e.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    // ********** Read **********

    pub fn reward_token(e: Env) -> Address {
        read_reward_token(&e)
    }

    pub fn op_merkle_root(e: Env, op_id: u32, epoch: u32) -> BytesN<32> {
        read_root(&e, op_id, epoch)
            .unwrap_or_else(|| BytesN::from_array(&e, &[0u8; 32]))
    }

    pub fn op_claimed(e: Env, op_id: u32, epoch: u32, user: Address) -> bool {
        is_claimed(&e, op_id, epoch, &user)
    }

    pub fn verify_op_claim(
        e: Env,
        op_id: u32,
        user: Address,
        epoch: u32,
        claimed_balance: i128,
        merkle_proof: Vec<BytesN<32>>,
    ) -> bool {
        verify(&e, op_id, &user, epoch, claimed_balance, &merkle_proof)
    }

    // ********** Operation rewards **********

    pub fn claim_op_epoch(
        e: Env,
        op_id: u32,
        user: Address,
        epoch: u32,
        claimed_balance: i128,
        merkle_proof: Vec<BytesN<32>>,
    ) {
        if claimed_balance <= 0 {
            panic_with_error!(&e, Error::InvalidClaimBalance);
        }

        bump_instance(&e);

        if is_claimed(&e, op_id, epoch, &user) {
            panic_with_error!(&e, Error::AlreadyClaimed);
        }

        settle_claim(&e, op_id, &user, epoch, claimed_balance, &merkle_proof);
        transfer_rewards(&e, op_id, &user, claimed_balance);
    }

    pub fn claim_op_epochs(
        e: Env,
        op_id: u32,
        user: Address,
        claims: Vec<ClaimData>,
    ) {
        bump_instance(&e);
        let mut total: i128 = 0;

        for claim in claims.iter() {
            if is_claimed(&e, op_id, claim.epoch, &user) {
                continue;
            }

            settle_claim(
                &e,
                op_id,
                &user,
                claim.epoch,
                claim.balance,
                &claim.merkle_proof,
            );
            total += claim.balance;
        }

        if total > 0 {
            transfer_rewards(&e, op_id, &user, total);
        }
    }
}
