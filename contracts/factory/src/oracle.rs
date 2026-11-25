use soroban_sdk::{
    contractclient, contracttype, panic_with_error, symbol_short, Address, Env,
    Symbol,
};

use crate::errors::Error;
use crate::storage as st;
use crate::types::{Coin, DataKey};

pub const PRICE_PRECISION: i128 = 1_000_000; // 1e6
const MAX_PRICE_AGE: u64 = 600; // 10 min in seconds

/// Band (6 decimals, inclusive) within which a coin's value in its reference
/// currency (USD for USDC, EUR for EURC) must stay for purchases in it to be
/// accepted. Shares are priced as if 1 USDC were 1 USD and 1 EURC were 1 EUR,
/// so outside it the factory would sell euro-priced shares for tokens worth
/// less than it assumes.
const PEG_MIN: i128 = 980_000; // 0.98
const PEG_MAX: i128 = 1_020_000; // 1.02

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

/// Stores the stablecoin oracle (Reflector CEX & DEX feed, which prices `USDC`
/// and `EURC` in USD; the forex feed has neither) and caches its decimals.
pub fn set_stablecoin_oracle(e: &Env, oracle: &Address) {
    store_feed(
        e,
        oracle,
        &DataKey::StablecoinOracle,
        &DataKey::StablecoinOracleDecimals,
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

/// `coin`'s USD price from the stablecoin oracle, 6 decimals.
fn stablecoin_usd_price(e: &Env, coin: Coin) -> i128 {
    let symbol = match coin {
        Coin::Usdc => symbol_short!("USDC"),
        Coin::Eurc => symbol_short!("EURC"),
    };
    read_price(
        e,
        &DataKey::StablecoinOracle,
        &DataKey::StablecoinOracleDecimals,
        symbol,
    )
}

/// Fails unless `coin` trades within [`PEG_MIN`, `PEG_MAX`] of its reference
/// currency: USDC/USD straight from the stablecoin oracle, EURC/EUR as its
/// USD price over the EUR/USD forex rate.
pub fn require_pegged(e: &Env, coin: Coin) {
    let (value, error) = match coin {
        Coin::Usdc => (stablecoin_usd_price(e, coin), Error::UsdcOffPeg),
        Coin::Eurc => (
            stablecoin_usd_price(e, coin) * PRICE_PRECISION
                / get_eur_usd_price(e),
            Error::EurcOffPeg,
        ),
    };
    if !(PEG_MIN..=PEG_MAX).contains(&value) {
        panic_with_error!(e, error);
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

/// Scale of one whole `coin` in its token's base units. Internal pricing is
/// 6-decimal throughout (`eur_per_share`, the oracle price, shares); only the
/// leg that actually moves tokens is denominated in the token's decimals.
fn precision(e: &Env, coin: Coin) -> i128 {
    10i128.pow(st::token_decimals(e, coin))
}

/// Price of 1 EUR in `coin`, 6 decimals: the EUR/USD rate for USDC (taken as
/// 1 USD), and par for EURC (taken as 1 EUR, so no oracle leg).
fn eur_rate(e: &Env, coin: Coin) -> i128 {
    match coin {
        Coin::Usdc => get_eur_usd_price(e),
        Coin::Eurc => PRICE_PRECISION,
    }
}

/// Cost of `shares_amount` shares of an operation priced at `eur_per_share`,
/// in `coin` base units.
pub fn amount_in(
    e: &Env,
    coin: Coin,
    eur_per_share: i128,
    shares_amount: i128,
) -> i128 {
    if shares_amount <= 0 {
        panic_with_error!(e, Error::InvalidShares);
    }
    let shares_price_eur = eur_per_share * shares_amount / PRICE_PRECISION;
    // 6-decimal EUR * 6-decimal coin/EUR = 12 decimals. Rescaling straight
    // into the token's precision in one divide keeps every digit a 7-decimal
    // token can hold, instead of rounding to 1e-6 first and multiplying the
    // loss back up.
    let cost = shares_price_eur * eur_rate(e, coin) * precision(e, coin)
        / (PRICE_PRECISION * PRICE_PRECISION);
    // A non-positive cost means the order rounds to nothing; minting shares
    // for it would give them away.
    if cost <= 0 {
        panic_with_error!(e, Error::AmountTooSmall);
    }
    cost
}

/// Shares obtainable for `amount` `coin` base units at `eur_per_share`.
pub fn amount_out(
    e: &Env,
    coin: Coin,
    eur_per_share: i128,
    amount: i128,
) -> i128 {
    if amount <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }
    // Token base units -> 6 decimals, before the 6-decimal share math.
    let amount_6 = amount * PRICE_PRECISION / precision(e, coin);
    // 6-decimal coin over a 12-decimal divisor (6-decimal EUR/share ×
    // 6-decimal coin/EUR): two PRICE_PRECISION factors bring the quotient
    // back to 6-decimal shares.
    let shares = amount_6 * PRICE_PRECISION * PRICE_PRECISION
        / (eur_per_share * eur_rate(e, coin));
    if shares <= 0 {
        panic_with_error!(e, Error::AmountTooSmall);
    }
    shares
}

/// USD value of `amount` `coin` base units, in USDC base units: what
/// `Invested.usdc_amount` reports. USDC is taken at par; EURC is converted at
/// the current EUR/USD rate.
pub fn usd_value(e: &Env, coin: Coin, amount: i128) -> i128 {
    match coin {
        Coin::Usdc => amount,
        Coin::Eurc => {
            amount * get_eur_usd_price(e) * precision(e, Coin::Usdc)
                / (precision(e, coin) * PRICE_PRECISION)
        }
    }
}
