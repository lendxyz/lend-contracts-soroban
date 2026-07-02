use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, Vec};

use crate::events;
use crate::merkle;
use crate::storage_types::{
    ClaimData, DataKey, INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD,
};

#[contract]
pub struct LendRewards;

fn read_admin(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("not initialized")
}

fn require_admin(e: &Env) -> Address {
    let admin = read_admin(e);
    admin.require_auth();
    admin
}

fn read_reward_token(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::RewardToken).unwrap()
}

fn zero_root(e: &Env) -> BytesN<32> {
    BytesN::from_array(e, &[0u8; 32])
}

fn bump_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
            panic!("cannot rewrite merkle root");
        }

        token::Client::new(&e, &read_reward_token(&e)).transfer(
            &admin,
            &e.current_contract_address(),
            &total_allocation,
        );

        e.storage().persistent().set(&key, &merkle_root);
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
        e.storage()
            .instance()
            .set(&DataKey::RewardToken, &new_token);
        events::RewardTokenUpdated { new_token }.publish(&e);
    }

    pub fn set_admin(e: Env, new_admin: Address) {
        require_admin(&e);
        e.storage().instance().set(&DataKey::Admin, &new_admin);
    }

    /// Withdraw the full balance of any non-reward token to the admin.
    pub fn emergency_withdraw(e: Env, token_addr: Address) {
        let admin = require_admin(&e);
        if token_addr == read_reward_token(&e) {
            panic!("cannot emergency withdraw reward token");
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

    /// Admin-gated wasm upgrade (UUPS-equivalent).
    pub fn upgrade(e: Env, new_wasm_hash: BytesN<32>) {
        require_admin(&e);
        e.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    // ********** Read **********

    pub fn reward_token(e: Env) -> Address {
        read_reward_token(&e)
    }

    pub fn op_merkle_root(e: Env, op_id: u32, epoch: u32) -> BytesN<32> {
        e.storage()
            .persistent()
            .get(&DataKey::OpMerkleRoot(op_id, epoch))
            .unwrap_or_else(|| zero_root(&e))
    }

    pub fn op_claimed(e: Env, op_id: u32, epoch: u32, user: Address) -> bool {
        e.storage()
            .persistent()
            .get(&DataKey::OpClaimed(op_id, epoch, user))
            .unwrap_or(false)
    }

    pub fn verify_op_claim(
        e: Env,
        op_id: u32,
        user: Address,
        epoch: u32,
        claimed_balance: i128,
        merkle_proof: Vec<BytesN<32>>,
    ) -> bool {
        let root = Self::op_merkle_root(e.clone(), op_id, epoch);
        let leaf = merkle::leaf(&e, &user, claimed_balance);
        merkle::verify(&e, &merkle_proof, &root, leaf)
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
            panic!("claim balance must be more than 0");
        }
        let claimed_key = DataKey::OpClaimed(op_id, epoch, user.clone());
        if e.storage().persistent().get(&claimed_key).unwrap_or(false) {
            panic!("epoch already claimed for this user");
        }
        if !Self::verify_op_claim(
            e.clone(),
            op_id,
            user.clone(),
            epoch,
            claimed_balance,
            merkle_proof,
        ) {
            panic!("Incorrect merkle proof");
        }

        e.storage().persistent().set(&claimed_key, &true);
        transfer_rewards(&e, op_id, &user, claimed_balance);
    }

    pub fn claim_op_epochs(
        e: Env,
        op_id: u32,
        user: Address,
        claims: Vec<ClaimData>,
    ) {
        let mut total: i128 = 0;
        for claim in claims.iter() {
            let claimed_key =
                DataKey::OpClaimed(op_id, claim.epoch, user.clone());
            if !e.storage().persistent().get(&claimed_key).unwrap_or(false) {
                if !Self::verify_op_claim(
                    e.clone(),
                    op_id,
                    user.clone(),
                    claim.epoch,
                    claim.balance,
                    claim.merkle_proof.clone(),
                ) {
                    panic!("Incorrect merkle proof");
                }
                total += claim.balance;
                e.storage().persistent().set(&claimed_key, &true);
            }
        }
        if total > 0 {
            transfer_rewards(&e, op_id, &user, total);
        }
    }
}

/// Transfers `balance` reward tokens from the contract to `user` and emits the
/// matching claim event.
fn transfer_rewards(e: &Env, op_id: u32, user: &Address, balance: i128) {
    if balance > 0 {
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
}
