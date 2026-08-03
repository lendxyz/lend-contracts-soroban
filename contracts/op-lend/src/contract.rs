use crate::account::{self, Wl};
use crate::admin::{read_administrator, write_administrator};
use crate::allowance::{read_allowance, spend_allowance, write_allowance};
use crate::crypto::{build_whitelist_message, verify_backend_sig};
use crate::errors::Error;
use crate::metadata::{read_decimal, read_name, read_symbol, write_metadata};
use crate::storage_types::{AllowanceDataKey, AllowanceValue, DataKey};
use crate::storage_types::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};
use soroban_sdk::{
    contract, contractevent, contractimpl, panic_with_error,
    token::TokenInterface, Address, BytesN, Env, MuxedAddress, String,
};
use soroban_token_sdk::events;
use soroban_token_sdk::metadata::TokenMetadata;

fn check_nonnegative_amount(e: &Env, amount: i128) {
    if amount < 0 {
        panic_with_error!(e, Error::NegativeAmount);
    }
}

fn bump_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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

fn reduce_total_supply(e: &Env, amount: i128) {
    write_total_supply(e, read_total_supply(e) - amount);
}

fn read_max_supply(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::MaxSupply).unwrap_or(0)
}

fn read_backend_signer(e: &Env) -> BytesN<32> {
    match e.storage().instance().get(&DataKey::BackendSigner) {
        Some(signer) => signer,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

fn require_admin(e: &Env) -> Address {
    let admin = read_administrator(e);
    admin.require_auth();
    bump_instance(e);
    admin
}

#[contract]
pub struct OpLendToken;

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
            panic_with_error!(&e, Error::DecimalTooLarge);
        }

        check_nonnegative_amount(&e, max_supply);
        write_administrator(&e, &admin);

        e.storage().instance().set(&DataKey::MaxSupply, &max_supply);
        e.storage()
            .instance()
            .set(&DataKey::BackendSigner, &backend_signer);

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
        check_nonnegative_amount(&e, amount);
        require_admin(&e);

        let total = read_total_supply(&e) + amount;

        if total > read_max_supply(&e) {
            panic_with_error!(&e, Error::SupplyCapExceeded);
        }

        write_total_supply(&e, total);

        account::adjust(&e, &to, amount, Wl::Grant);
        events::MintWithAmountOnly { to, amount }.publish(&e);
    }

    pub fn admin_burn(e: Env, user: Address, amount: i128) {
        check_nonnegative_amount(&e, amount);
        require_admin(&e);

        account::adjust(&e, &user, -amount, Wl::Skip);
        reduce_total_supply(&e, amount);
        events::Burn { from: user, amount }.publish(&e);
    }

    pub fn whitelist_user_admin(e: Env, user: Address, state: bool) {
        require_admin(&e);
        account::set_whitelisted(&e, &user, state);
    }

    pub fn whitelist_user(
        e: Env,
        user: Address,
        nonce: String,
        signature: BytesN<64>,
    ) {
        bump_instance(&e);
        let nonce_key = DataKey::UsedNonce(nonce.clone());

        if e.storage().persistent().has(&nonce_key) {
            panic_with_error!(&e, Error::NonceAlreadyUsed);
        }

        let msg = build_whitelist_message(&e, &user, &nonce);
        verify_backend_sig(&e, &read_backend_signer(&e), &msg, &signature);

        e.storage().persistent().set(&nonce_key, &());
        account::set_whitelisted(&e, &user, true);
    }

    pub fn update_backend_signer(e: Env, new_signer: BytesN<32>) {
        require_admin(&e);
        e.storage()
            .instance()
            .set(&DataKey::BackendSigner, &new_signer);
    }

    pub fn is_whitelisted(e: Env, user: Address) -> bool {
        account::is_whitelisted(&e, &user)
    }

    pub fn total_supply(e: Env) -> i128 {
        read_total_supply(&e)
    }

    pub fn max_supply(e: Env) -> i128 {
        read_max_supply(&e)
    }

    pub fn set_admin(e: Env, new_admin: Address) {
        let admin = require_admin(&e);
        write_administrator(&e, &new_admin);

        SetAdmin { admin, new_admin }.publish(&e);
    }

    pub fn get_allowance(
        e: Env,
        from: Address,
        spender: Address,
    ) -> Option<AllowanceValue> {
        e.storage()
            .temporary()
            .get(&DataKey::Allowance(AllowanceDataKey { from, spender }))
    }
}

#[contractimpl(contracttrait)]
impl TokenInterface for OpLendToken {
    fn allowance(e: Env, from: Address, spender: Address) -> i128 {
        bump_instance(&e);
        read_allowance(&e, from, spender)
    }

    fn approve(
        e: Env,
        from: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        from.require_auth();
        check_nonnegative_amount(&e, amount);
        bump_instance(&e);

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
        bump_instance(&e);
        account::balance(&e, &id)
    }

    fn transfer(e: Env, from: Address, to_muxed: MuxedAddress, amount: i128) {
        from.require_auth();
        check_nonnegative_amount(&e, amount);
        bump_instance(&e);

        let to: Address = to_muxed.address();

        account::adjust(&e, &from, -amount, Wl::Require);
        account::adjust(&e, &to, amount, Wl::Require);

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
        check_nonnegative_amount(&e, amount);
        bump_instance(&e);

        spend_allowance(&e, from.clone(), spender, amount);

        account::adjust(&e, &from, -amount, Wl::Require);
        account::adjust(&e, &to, amount, Wl::Require);

        events::Transfer {
            from,
            to,
            to_muxed_id: None,
            amount,
        }
        .publish(&e);
    }

    fn burn(e: Env, from: Address, amount: i128) {
        from.require_auth();
        check_nonnegative_amount(&e, amount);
        bump_instance(&e);

        account::adjust(&e, &from, -amount, Wl::Skip);
        reduce_total_supply(&e, amount);
        events::Burn { from, amount }.publish(&e);
    }

    fn burn_from(e: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        check_nonnegative_amount(&e, amount);
        bump_instance(&e);

        spend_allowance(&e, from.clone(), spender, amount);
        account::adjust(&e, &from, -amount, Wl::Skip);
        reduce_total_supply(&e, amount);
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
