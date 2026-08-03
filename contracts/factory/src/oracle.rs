use soroban_sdk::{
    contractclient, contracttype, panic_with_error, symbol_short, Address, Env,
    Symbol,
};

use crate::errors::Error;
use crate::types::DataKey;

pub const PRICE_PRECISION: i128 = 1_000_000; // 1e6
pub const SHARE_PRECISION: i128 = 1_000_000_000_000; // 1e12
const MAX_PRICE_AGE: u64 = 86_400; // 24h in seconds

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

/// Stores the oracle address and caches its `decimals()`.
pub fn set_oracle(e: &Env, oracle: &Address) {
    let decimals = OracleClient::new(e, oracle).decimals();
    e.storage().instance().set(&DataKey::Oracle, oracle);
    e.storage()
        .instance()
        .set(&DataKey::OracleDecimals, &decimals);
}

/// Reads the EUR/USD price from the Reflector oracle, scaled to 6 decimals.
pub fn get_eur_usd_price(e: &Env) -> i128 {
    let instance = e.storage().instance();
    let oracle: Address = match instance.get(&DataKey::Oracle) {
        Some(oracle) => oracle,
        None => panic_with_error!(e, Error::NotInitialized),
    };
    let decimals: u32 = instance.get(&DataKey::OracleDecimals).unwrap_or(6);

    let asset = Asset::Other(symbol_short!("EUR"));
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

/// USDC cost for `shares_amount` shares of operation priced at `eur_per_shares`.
pub fn amount_in(e: &Env, eur_per_shares: i128, shares_amount: i128) -> i128 {
    if shares_amount <= 0 {
        panic_with_error!(e, Error::InvalidShares);
    }
    let shares_price_eur = eur_per_shares * shares_amount / PRICE_PRECISION;
    let usdc_cost = shares_price_eur * get_eur_usd_price(e) / PRICE_PRECISION;
    if usdc_cost <= 0 {
        1
    } else {
        usdc_cost
    }
}

/// Shares obtainable for `usdc_amount` at `eur_per_shares`.
pub fn amount_out(e: &Env, eur_per_shares: i128, usdc_amount: i128) -> i128 {
    if usdc_amount <= 0 {
        panic_with_error!(e, Error::InvalidAmount);
    }
    let shares =
        usdc_amount * SHARE_PRECISION / (eur_per_shares * get_eur_usd_price(e));
    if shares <= 0 {
        1
    } else {
        shares
    }
}
