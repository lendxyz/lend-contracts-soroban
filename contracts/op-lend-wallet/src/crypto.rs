use soroban_sdk::{panic_with_error, Address, Bytes, BytesN, Env, String};

use crate::errors::Error;
use crate::types::DataKey;

const STRKEY_LEN: usize = 56;

/// The canonical message the backend signs to authorize a redeem.
///
/// Mirrors the EVM
/// `keccak256(abi.encodePacked("OP_REDEEM", opId, chainid, destination, amount, nonce))`
/// with two Soroban changes. The wallet's own address replaces `chainid`: a
/// contract id is derived from the network passphrase, so it separates
/// deployments *and* networks in one field. And the bytes are signed raw —
/// no keccak, no `"\x19Ethereum Signed Message:\n32"` envelope — because
/// `ed25519_verify` hashes the message itself.
///
/// Laid out in one fixed-size stack buffer: every field but the trailing nonce
/// has a known width, so the head costs a single host `Bytes` allocation
/// instead of five appends.
pub fn build_redeem_message(
    e: &Env,
    op_id: u32,
    destination: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    const TAG: &[u8] = b"OP_REDEEM";
    const WALLET: usize = TAG.len();
    const ID: usize = WALLET + STRKEY_LEN;
    const DEST: usize = ID + 4;
    const AMOUNT: usize = DEST + STRKEY_LEN;
    const END: usize = AMOUNT + 16;

    let mut buf = [0u8; END];
    buf[..WALLET].copy_from_slice(TAG);
    e.current_contract_address()
        .to_string()
        .copy_into_slice(&mut buf[WALLET..ID]);
    buf[ID..DEST].copy_from_slice(&op_id.to_be_bytes());
    destination
        .to_string()
        .copy_into_slice(&mut buf[DEST..AMOUNT]);
    buf[AMOUNT..].copy_from_slice(&amount.to_be_bytes());

    let mut msg = Bytes::from_slice(e, &buf);
    msg.append(&Bytes::from(nonce));
    msg
}

/// Verifies an ed25519 backend signature over `msg`. Panics if it does not
/// check out, which reverts the whole redeem — including the nonce spend.
pub fn verify_backend_sig(
    e: &Env,
    signer: &BytesN<32>,
    msg: &Bytes,
    sig: &BytesN<64>,
) {
    e.crypto().ed25519_verify(signer, msg, sig);
}

/// Spends a redeem nonce, permanently. The key's presence is the whole value,
/// so a spent nonce is the smallest entry the ledger can hold rather than a
/// `bool` payload.
pub fn consume_nonce(e: &Env, nonce: &String) {
    let key = DataKey::UsedNonce(nonce.clone());
    if e.storage().persistent().has(&key) {
        panic_with_error!(e, Error::NonceAlreadyUsed);
    }
    e.storage().persistent().set(&key, &());
}

pub fn nonce_used(e: &Env, nonce: &String) -> bool {
    e.storage()
        .persistent()
        .has(&DataKey::UsedNonce(nonce.clone()))
}
