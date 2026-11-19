use crate::account::{self, Wl};
use crate::admin::{read_administrator, write_administrator};
use crate::allowance::{read_allowance, spend_allowance, write_allowance};
use crate::crypto::{build_whitelist_message, verify_backend_sig};
use crate::errors::Error;
use crate::metadata::{read_decimal, read_name, read_symbol, write_metadata};
use crate::storage_types::{AllowanceDataKey, AllowanceValue, DataKey};
use crate::storage_types::{INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD};
use common::build_void_nonce_message;
use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, panic_with_error,
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

/// The one factory function this token calls.
#[contractclient(name = "FactoryClient")]
#[allow(dead_code)]
trait FactoryInterface {
    fn backend_signer(env: Env) -> BytesN<32>;
}

fn read_factory(e: &Env) -> Address {
    match e.storage().instance().get(&DataKey::Factory) {
        Some(factory) => factory,
        None => panic_with_error!(e, Error::NotInitialized),
    }
}

/// The backend key, read from the factory on every check rather than stored
/// here, so rotating it on the factory rotates it for every token.
fn read_backend_signer(e: &Env) -> BytesN<32> {
    FactoryClient::new(e, &read_factory(e)).backend_signer()
}

fn consume_nonce(e: &Env, nonce: &String) {
    let key = DataKey::UsedNonce(nonce.clone());
    if e.storage().persistent().has(&key) {
        panic_with_error!(e, Error::NonceAlreadyUsed);
    }
    e.storage().persistent().set(&key, &());
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

#[contractevent]
pub struct NonceVoided {
    #[topic]
    nonce: String,
}

/// Share decimals. Fixed rather than a constructor argument: the factory prices
/// shares in a 6-decimal unit and never reads `decimals()`, so any other value
/// would misstate every balance.
const DECIMALS: u32 = 6;

#[contractimpl]
impl OpLendToken {
    pub fn __constructor(
        e: Env,
        admin: Address,
        name: String,
        symbol: String,
        max_supply: i128,
        factory: Address,
    ) {
        check_nonnegative_amount(&e, max_supply);
        write_administrator(&e, &admin);

        e.storage().instance().set(&DataKey::MaxSupply, &max_supply);
        e.storage().instance().set(&DataKey::Factory, &factory);

        write_metadata(
            &e,
            TokenMetadata {
                decimal: DECIMALS,
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

        let msg = build_whitelist_message(&e, &user, &nonce);
        verify_backend_sig(&e, &read_backend_signer(&e), &msg, &signature);
        consume_nonce(&e, &nonce);

        account::set_whitelisted(&e, &user, true);
    }

    /// Burns an unspent whitelist `nonce` so the signature carrying it can
    /// never be used. With a `signature`, authorized by the backend signing the
    /// void message (not the original signature, which also binds the user);
    /// without one, by the admin's auth.
    pub fn void_nonce(e: Env, nonce: String, signature: Option<BytesN<64>>) {
        match signature {
            Some(signature) => {
                bump_instance(&e);
                let msg = build_void_nonce_message(&e, &nonce);
                verify_backend_sig(
                    &e,
                    &read_backend_signer(&e),
                    &msg,
                    &signature,
                );
            }
            None => {
                require_admin(&e);
            }
        }
        consume_nonce(&e, &nonce);

        NonceVoided { nonce }.publish(&e);
    }

    pub fn is_whitelisted(e: Env, user: Address) -> bool {
        account::is_whitelisted(&e, &user)
    }

    /// The factory this token reads the backend signer from. Set by the
    /// constructor and never changed: nothing can repoint a token's signer.
    pub fn factory(e: Env) -> Address {
        read_factory(&e)
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

    /// Swaps this token's code, keeping its id, balances and whitelist.
    /// Admin-only; in production the admin is the factory, so this runs
    /// through `factory.oplend_upgrade`.
    pub fn upgrade(e: Env, new_wasm_hash: BytesN<32>) {
        require_admin(&e);
        e.deployer().update_current_contract_wasm(new_wasm_hash);
        // `require_admin` already bumped, but before the swap. Instance TTL
        // extension also covers the code entry, so bump again for the new wasm.
        bump_instance(&e);
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

/// SEP-41, except that only the admin burns: `burn` takes the admin's auth
/// instead of the holder's, and `burn_from` only lets the admin spend an
/// allowance.
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

    /// Admin only: burns from any holder, without the holder's auth.
    fn burn(e: Env, from: Address, amount: i128) {
        check_nonnegative_amount(&e, amount);
        require_admin(&e);

        account::adjust(&e, &from, -amount, Wl::Skip);
        reduce_total_supply(&e, amount);
        events::Burn { from, amount }.publish(&e);
    }

    /// Admin only: `spender` must be the admin, and spends its allowance.
    fn burn_from(e: Env, spender: Address, from: Address, amount: i128) {
        check_nonnegative_amount(&e, amount);
        if spender != require_admin(&e) {
            panic_with_error!(&e, Error::NotAdmin);
        }

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
