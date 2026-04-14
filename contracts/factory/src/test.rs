#![cfg(test)]

use crate::contract::LendFactoryClient;

use super::*;
use soroban_sdk::Env;

#[test]
fn test() {
    let env = Env::default();
    let contract_id = env.register(LendFactory, ());
    let client = LendFactoryClient::new(&env, &contract_id);

    // TODO: make test suite for factory

    assert_eq!(true, true);
}
