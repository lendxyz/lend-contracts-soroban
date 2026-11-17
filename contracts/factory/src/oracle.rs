use soroban_sdk::{
    contractclient, contracttype, panic_with_error, symbol_short, Address, Env,
    Symbol,
};

use crate::errors::Error;
use crate::storage as st;
use crate::types::DataKey;

pub const PRICE_PRECISION: i128 = 1_000_000; // 1e6
const MAX_PRICE_AGE: u64 = 86_400; // 24h in seconds

/// USDC/USD band (6 decimals, inclusive) within which USDC purchases are
/// accepted. Shares are priced as if 1 USDC were 1 USD, so outside it the
/// factory would sell euro-priced shares for USDC worth less than it assumes.
const USDC_PEG_MIN: i128 = 980_000; // 0.98
const USDC_PEG_MAX: i128 = 1_020_000; // 1.02

/// Reflector SEP-40 asset selector.
#[contracttype(export = false)]
#[derive(Clone)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype(export = false)]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

/// Subset of the Reflector (SEP-40) oracle interface.
#[contractclient(name = "OracleClient")]
#[allow(dead_code)]
pub trait OracleInterface {
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
    fn decimals(env: Env) -> u32;
}

/// Stores the EUR/USD oracle (Reflector forex feed) and caches its decimals.
pub fn set_oracle(e: &Env, oracle: &Address) {
    store_feed(e, oracle, &DataKey::Oracle, &DataKey::OracleDecimals);
}

/// Stores the USDC/USD oracle (Reflector CEX & DEX feed, which prices `USDC`
/// in USD; the forex feed does not) and caches its decimals.
pub fn set_usdc_oracle(e: &Env, oracle: &Address) {
    store_feed(
        e,
        oracle,
        &DataKey::UsdcOracle,
        &DataKey::UsdcOracleDecimals,
    );
}

fn store_feed(
    e: &Env,
    oracle: &Address,
    key: &DataKey,
    decimals_key: &DataKey,
) {
    let decimals = OracleClient::new(e, oracle).decimals();
    let instance = e.storage().instance();
    instance.set(key, oracle);
    instance.set(decimals_key, &decimals);
}

/// Reads the EUR/USD price from the Reflector oracle, scaled to 6 decimals.
pub fn get_eur_usd_price(e: &Env) -> i128 {
    read_price(
        e,
        &DataKey::Oracle,
        &DataKey::OracleDecimals,
        symbol_short!("EUR"),
    )
}

/// Fails with `UsdcOffPeg` unless USDC trades within
/// [`USDC_PEG_MIN`, `USDC_PEG_MAX`] USD on the USDC oracle.
pub fn require_usdc_pegged(e: &Env) {
    let price = read_price(
        e,
        &DataKey::UsdcOracle,
        &DataKey::UsdcOracleDecimals,
        symbol_short!("USDC"),
    );
    if !(USDC_PEG_MIN..=USDC_PEG_MAX).contains(&price) {
        panic_with_error!(e, Error::UsdcOffPeg);
    }
}

/// `symbol`'s USD price from the feed stored under `key` (decimals cached
/// under `decimals_key`): rejects a missing, non-positive or stale price, and
/// scales it to 6 decimals.
fn read_price(
    e: &Env,
    key: &DataKey,
    decimals_key: &DataKey,
    symbol: Symbol,
) -> i128 {
    let instance = e.storage().instance();
    let oracle: Address = match instance.get(key) {
        Some(oracle) => oracle,
        None => panic_with_error!(e, Error::NotInitialized),
    };
    // Written with the oracle (constructor / `update_*oracle_address`); never
    // guess a scale if it is missing.
    let decimals: u32 = match instance.get(decimals_key) {
        Some(decimals) => decimals,
        None => panic_with_error!(e, Error::NotInitialized),
    };

    let asset = Asset::Other(symbol);
    let data = match OracleClient::new(e, &oracle).lastprice(&asset) {
        Some(data) => data,
        None => panic_with_error!(e, Error::OracleNoPrice),
    };

    if data.price <= 0 {
        panic_with_error!(e, Error::OracleInvalidPrice);
    }
    if e.ledger().timestamp().saturating_sub(data.timestamp) > MAX_PRICE_AGE {
        panic_with_error!(e, Error::OracleStalePrice);
    }

    scale_to_6(data.price, decimals)
}

fn scale_to_6(price: i128, decimals: u32) -> i128 {
    if decimals < 6 {
        price * 10i128.pow(6 - decimals)
    } else if decimals > 6 {
        price / 10i128.pow(decimals - 6)
    } else {
        price
    }
}

/// Scale of one whole USDC in the token's own base units. Internal pricing is
/// 6-decimal throughout (`eur_per_shares`, the oracle price, shares); only the
/// leg that actually moves tokens is denominated in the token's decimals.
fn usdc_precision(e: &Env) -> i128 {
    10i128.pow(st::usdc_decimals(e))
}

/// USDC cost for `shares_amount` shares of an operation priced at
/// `eur_per_shares`, in USDC base units.
pub fn amount_in(e: &Env, eur_per_shares: i128, shares_amount: i128) -> i128 {
    if shares_amount <= 0 {
        panic_with_error!(e, Error::InvalidShares);
    }
    let shares_price_eur = eur_per_shares * shares_amount / PRICE_PRECISION;
    // 6-decimal EUR * 6-decimal USD/EUR = 12 decimals. Rescaling straight into
    // the token's precision in one divide keeps every digit a 7-decimal USDC can
    // hold, instead of rounding to 1e-6 first and multiplying the loss back up.
    let usdc_cost = shares_price_eur * get_eur_usd_price(e) * usdc_precision(e)
        / (PRICE_PRECISION * PRICE_PRECISION);
    // A non-positive cost means the order rounds to nothing; minting shares
    // for it would give them away.
    if usdc_cost <= 0 {
        panic_with_error!(e, Error::AmountTooSmall);
    }
    usdc_cost
}

/// Shares obtainable for `usdc_amount` USDC base units at `eur_per_shares`.
pub fn amount_out(e: &Env, eur_per_shares: i128, usdc_amount: i128) -> i128 {
    if usdc_amount <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }
    // Token base units -> 6-decimal USDC, before the 6-decimal share math.
    let usdc_6 = usdc_amount * PRICE_PRECISION / usdc_precision(e);
    // 6-decimal USDC over a 12-decimal divisor (6-decimal EUR/share × 6-decimal
    // USD/EUR): two PRICE_PRECISION factors bring the quotient back to
    // 6-decimal shares.
    let shares = usdc_6 * PRICE_PRECISION * PRICE_PRECISION
        / (eur_per_shares * get_eur_usd_price(e));
    if shares <= 0 {
        panic_with_error!(e, Error::AmountTooSmall);
    }
    shares
}
