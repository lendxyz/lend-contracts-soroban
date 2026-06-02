use crate::admin::{read_administrator, write_administrator};
use crate::allowance::{read_allowance, spend_allowance, write_allowance};
use crate::balance::{read_balance, receive_balance, spend_balance};
use crate::crypto::{build_whitelist_message, verify_backend_sig};
use crate::metadata::{read_decimal, read_name, read_symbol, write_metadata};
use crate::storage_types::{AllowanceDataKey, AllowanceValue, DataKey};
use crate::storage_types::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};
use crate::whitelist::{is_whitelisted, require_whitelisted, set_whitelisted};
use soroban_sdk::{
    contract, contractevent, contractimpl, token::TokenInterface, Address,
    BytesN, Env, MuxedAddress, String,
};
use soroban_token_sdk::events;
use soroban_token_sdk::metadata::TokenMetadata;

fn check_nonnegative_amount(amount: i128) {
    if amount < 0 {
        panic!("negative amount is not allowed: {}", amount)
    }
}

fn read_total_supply(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::TotalSupply)
        .unwrap_or(0)
}

fn write_total_supply(e: &Env, amount: i128) {
    e.storage().instance().set(&DataKey::TotalSupply, &amount);
}

fn read_max_supply(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::MaxSupply).unwrap_or(0)
}

fn read_backend_signer(e: &Env) -> BytesN<32> {
    e.storage().instance().get(&DataKey::BackendSigner).unwrap()
}

#[contract]
pub struct OpLendToken;

// SetAdmin is not a standardized token event, so we just define a custom event
// for our token.
#[contractevent(data_format = "single-value")]
pub struct SetAdmin {
    #[topic]
    admin: Address,
    new_admin: Address,
}

#[contractimpl]
impl OpLendToken {
    pub fn __constructor(
        e: Env,
        admin: Address,
        decimal: u32,
        name: String,
        symbol: String,
        max_supply: i128,
        backend_signer: BytesN<32>,
    ) {
        if decimal > 6 {
            panic!("Decimal must not be greater than 6");
        }
        check_nonnegative_amount(max_supply);
        write_administrator(&e, &admin);
        e.storage()
            .instance()
            .set(&DataKey::MaxSupply, &max_supply);
        e.storage()
            .instance()
            .set(&DataKey::BackendSigner, &backend_signer);
        write_total_supply(&e, 0);
        write_metadata(
            &e,
            TokenMetadata {
                decimal,
                name,
                symbol,
            },
        )
    }

    pub fn mint(e: Env, to: Address, amount: i128) {
        check_nonnegative_amount(amount);
        let admin = read_administrator(&e);
        admin.require_auth();

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        let total = read_total_supply(&e);
        if total + amount > read_max_supply(&e) {
            panic!("Total supply cap exceeded");
        }
        write_total_supply(&e, total + amount);

        // Minting whitelists the recipient (matches EVM `LendOperation.mint`).
        set_whitelisted(&e, &to, true);
        receive_balance(&e, to.clone(), amount);
        events::MintWithAmountOnly { to, amount }.publish(&e);
    }

    /// Admin burns tokens from any holder without requiring an allowance.
    pub fn admin_burn(e: Env, user: Address, amount: i128) {
        check_nonnegative_amount(amount);
        let admin = read_administrator(&e);
        admin.require_auth();

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        spend_balance(&e, user.clone(), amount);
        write_total_supply(&e, read_total_supply(&e) - amount);
        events::Burn { from: user, amount }.publish(&e);
    }

    /// Admin sets a user's whitelist state directly.
    pub fn whitelist_user_admin(e: Env, user: Address, state: bool) {
        let admin = read_administrator(&e);
        admin.require_auth();

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        set_whitelisted(&e, &user, state);
    }

    /// Anyone may whitelist `user` given a valid backend signature + unused nonce.
    pub fn whitelist_user(
        e: Env,
        user: Address,
        nonce: String,
        signature: BytesN<64>,
    ) {
        let nonce_key = DataKey::UsedNonce(nonce.clone());
        if e.storage().persistent().has(&nonce_key) {
            panic!("nonce already used");
        }

        let msg = build_whitelist_message(&e, &user, &nonce);
        verify_backend_sig(&e, &read_backend_signer(&e), &msg, &signature);

        e.storage().persistent().set(&nonce_key, &true);
        set_whitelisted(&e, &user, true);
    }

    pub fn update_backend_signer(e: Env, new_signer: BytesN<32>) {
        let admin = read_administrator(&e);
        admin.require_auth();

        e.storage()
            .instance()
            .set(&DataKey::BackendSigner, &new_signer);
    }

    pub fn is_whitelisted(e: Env, user: Address) -> bool {
        is_whitelisted(&e, &user)
    }

    pub fn total_supply(e: Env) -> i128 {
        read_total_supply(&e)
    }

    pub fn max_supply(e: Env) -> i128 {
        read_max_supply(&e)
    }

    pub fn set_admin(e: Env, new_admin: Address) {
        let admin = read_administrator(&e);
        admin.require_auth();

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        write_administrator(&e, &new_admin);
        SetAdmin { admin, new_admin }.publish(&e);
    }

    pub fn get_allowance(
        e: Env,
        from: Address,
        spender: Address,
    ) -> Option<AllowanceValue> {
        let key = DataKey::Allowance(AllowanceDataKey { from, spender });
        let allowance = e.storage().temporary().get::<_, AllowanceValue>(&key);
        allowance
    }
}

#[contractimpl(contracttrait)]
impl TokenInterface for OpLendToken {
    fn allowance(e: Env, from: Address, spender: Address) -> i128 {
        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        read_allowance(&e, from, spender).amount
    }

    fn approve(
        e: Env,
        from: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        from.require_auth();

        check_nonnegative_amount(amount);

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        write_allowance(
            &e,
            from.clone(),
            spender.clone(),
            amount,
            expiration_ledger,
        );
        events::Approve {
            from,
            spender,
            amount,
            expiration_ledger,
        }
        .publish(&e);
    }

    fn balance(e: Env, id: Address) -> i128 {
        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        read_balance(&e, id)
    }

    fn transfer(e: Env, from: Address, to_muxed: MuxedAddress, amount: i128) {
        from.require_auth();

        check_nonnegative_amount(amount);

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        let to: Address = to_muxed.address();
        require_whitelisted(&e, &from);
        require_whitelisted(&e, &to);

        spend_balance(&e, from.clone(), amount);
        receive_balance(&e, to.clone(), amount);
        events::Transfer {
            from,
            to,
            to_muxed_id: to_muxed.id(),
            amount,
        }
        .publish(&e);
    }

    fn transfer_from(
        e: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) {
        spender.require_auth();

        check_nonnegative_amount(amount);

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        require_whitelisted(&e, &from);
        require_whitelisted(&e, &to);

        spend_allowance(&e, from.clone(), spender, amount);
        spend_balance(&e, from.clone(), amount);
        receive_balance(&e, to.clone(), amount);
        events::Transfer {
            from,
            to,
            // `transfer_from` does not support muxed destination.
            to_muxed_id: None,
            amount,
        }
        .publish(&e);
    }

    fn burn(e: Env, from: Address, amount: i128) {
        from.require_auth();

        check_nonnegative_amount(amount);

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        spend_balance(&e, from.clone(), amount);
        write_total_supply(&e, read_total_supply(&e) - amount);
        events::Burn { from, amount }.publish(&e);
    }

    fn burn_from(e: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();

        check_nonnegative_amount(amount);

        e.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        spend_allowance(&e, from.clone(), spender, amount);
        spend_balance(&e, from.clone(), amount);
        write_total_supply(&e, read_total_supply(&e) - amount);
        events::Burn { from, amount }.publish(&e);
    }

    fn decimals(e: Env) -> u32 {
        read_decimal(&e)
    }

    fn name(e: Env) -> String {
        read_name(&e)
    }

    fn symbol(e: Env) -> String {
        read_symbol(&e)
    }
}
