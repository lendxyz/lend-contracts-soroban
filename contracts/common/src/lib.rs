//! Helpers shared by the lend contracts.
#![no_std]

use soroban_sdk::{Bytes, BytesN, Env, String};

/// Length of an account (`G...`) or contract (`C...`) strkey.
pub const STRKEY_LEN: usize = 56;

/// The message the backend signs to void `nonce` on the current contract
/// before it is spent: `"VOID_NONCE" || <contract strkey> || nonce`.
///
/// The tag keeps it from ever verifying as any other backend-signed message,
/// and the contract address keeps it from voiding the same nonce on another
/// contract. The fixed-width head is built on the stack so the message costs
/// one host `Bytes` allocation plus the nonce append.
pub fn build_void_nonce_message(e: &Env, nonce: &String) -> Bytes {
    const TAG: &[u8] = b"VOID_NONCE";
    const CONTRACT: usize = TAG.len();
    const END: usize = CONTRACT + STRKEY_LEN;

    let mut buf = [0u8; END];
    buf[..CONTRACT].copy_from_slice(TAG);
    e.current_contract_address()
        .to_string()
        .copy_into_slice(&mut buf[CONTRACT..]);

    let mut msg = Bytes::from_slice(e, &buf);
    msg.append(&Bytes::from(nonce));
    msg
}

/// Verifies the backend signer's ed25519 signature over `msg`. Panics if it
/// does not check out, which reverts the whole call, including any nonce
/// spend.
pub fn verify_backend_sig(
    e: &Env,
    signer: &BytesN<32>,
    msg: &Bytes,
    sig: &BytesN<64>,
) {
    e.crypto().ed25519_verify(signer, msg, sig);
}
