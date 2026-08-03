use soroban_sdk::{panic_with_error, Address, Bytes, BytesN, Env, String};

use crate::errors::Error;
use crate::types::DataKey;

const STRKEY_LEN: usize = 56;

pub fn build_invest_message(
    e: &Env,
    id: u32,
    user: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    const TAG: &[u8] = b"ONCHAIN_INVEST";
    const FACTORY: usize = TAG.len();
    const ID: usize = FACTORY + STRKEY_LEN;
    const USER: usize = ID + 4;
    const AMOUNT: usize = USER + STRKEY_LEN;
    const END: usize = AMOUNT + 16;

    let mut buf = [0u8; END];
    buf[..FACTORY].copy_from_slice(TAG);
    e.current_contract_address()
        .to_string()
        .copy_into_slice(&mut buf[FACTORY..ID]);
    buf[ID..USER].copy_from_slice(&id.to_be_bytes());
    user.to_string().copy_into_slice(&mut buf[USER..AMOUNT]);
    buf[AMOUNT..].copy_from_slice(&amount.to_be_bytes());

    with_nonce(e, &buf, nonce)
}

pub fn build_fiat_invest_message(
    e: &Env,
    id: u32,
    user: &Address,
    oplend_holder: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    const TAG: &[u8] = b"FIAT_INVEST";
    const FACTORY: usize = TAG.len();
    const ID: usize = FACTORY + STRKEY_LEN;
    const USER: usize = ID + 4;
    const HOLDER: usize = USER + STRKEY_LEN;
    const AMOUNT: usize = HOLDER + STRKEY_LEN;
    const END: usize = AMOUNT + 16;

    let mut buf = [0u8; END];
    buf[..FACTORY].copy_from_slice(TAG);
    e.current_contract_address()
        .to_string()
        .copy_into_slice(&mut buf[FACTORY..ID]);
    buf[ID..USER].copy_from_slice(&id.to_be_bytes());
    user.to_string().copy_into_slice(&mut buf[USER..HOLDER]);
    oplend_holder
        .to_string()
        .copy_into_slice(&mut buf[HOLDER..AMOUNT]);
    buf[AMOUNT..].copy_from_slice(&amount.to_be_bytes());

    with_nonce(e, &buf, nonce)
}

fn with_nonce(e: &Env, head: &[u8], nonce: &String) -> Bytes {
    let mut msg = Bytes::from_slice(e, head);
    msg.append(&Bytes::from(nonce));
    msg
}

pub fn verify_backend_sig(
    e: &Env,
    signer: &BytesN<32>,
    msg: &Bytes,
    sig: &BytesN<64>,
) {
    e.crypto().ed25519_verify(signer, msg, sig);
}

pub fn consume_nonce(e: &Env, nonce: &String) {
    let key = DataKey::UsedNonce(nonce.clone());
    if e.storage().persistent().has(&key) {
        panic_with_error!(e, Error::NonceAlreadyUsed);
    }
    e.storage().persistent().set(&key, &());
}
