use soroban_sdk::{contractclient, contracttype, Address, Env, Symbol};

use crate::types::DataKey;

pub const PRICE_PRECISION: i128 = 1_000_000; // 1e6
pub const SHARE_PRECISION: i128 = 1_000_000_000_000; // 1e12
const MAX_PRICE_AGE: u64 = 86_400; // 24h in seconds

/// Reflector SEP-40 asset selector.
#[contracttype]
#[derive(Clone)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
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

fn read_oracle(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&DataKey::Oracle)
        .expect("oracle not set")
}

/// Reads the EUR/USD price from the Reflector oracle, scaled to 6 decimals.
pub fn get_eur_usd_price(e: &Env) -> i128 {
    let oracle = read_oracle(e);
    let client = OracleClient::new(e, &oracle);

    let asset = Asset::Other(Symbol::new(e, "EUR"));
    let data = client.lastprice(&asset).expect("oracle: no price");

    if data.price <= 0 {
        panic!("oracle: non-positive price");
    }
    if e.ledger().timestamp().saturating_sub(data.timestamp) > MAX_PRICE_AGE {
        panic!("oracle: stale price");
    }

    let decimals = client.decimals();
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
        panic!("input cannot be zero");
    }
    let shares_price_eur = eur_per_shares * shares_amount / PRICE_PRECISION;
    let mut usdc_cost = shares_price_eur * get_eur_usd_price(e) / PRICE_PRECISION;
    if usdc_cost <= 0 {
        usdc_cost = 1;
    }
    usdc_cost
}

/// Shares obtainable for `usdc_amount` at `eur_per_shares`.
pub fn amount_out(e: &Env, eur_per_shares: i128, usdc_amount: i128) -> i128 {
    if usdc_amount <= 0 {
        panic!("input cannot be zero");
    }
    let oracle_price = get_eur_usd_price(e);
    let mut shares = usdc_amount * SHARE_PRECISION / (eur_per_shares * oracle_price);
    if shares <= 0 {
        shares = 1;
    }
    shares
}
