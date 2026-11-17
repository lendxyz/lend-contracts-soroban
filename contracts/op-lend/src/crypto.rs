use soroban_sdk::{Address, Bytes, BytesN, Env, String};

/// Builds the canonical message the backend signs to authorize a whitelist.
/// Mirrors the EVM `keccak256(abi.encodePacked(address(this), chainid, user, nonce))`,
/// minus chainid (the contract address already domain-separates per deployment).
pub fn build_whitelist_message(
    e: &Env,
    user: &Address,
    nonce: &String,
) -> Bytes {
    let mut msg = Bytes::from(e.current_contract_address().to_string());
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

/// The message the backend signs to void a whitelist nonce before it is spent.
/// The tag keeps it from ever verifying as a whitelist message (those start
/// with a `C...` strkey), and the token address keeps it from voiding the same
/// nonce on another contract.
pub fn build_void_nonce_message(e: &Env, nonce: &String) -> Bytes {
    let mut msg = Bytes::from_slice(e, b"VOID_NONCE");
    msg.append(&Bytes::from(e.current_contract_address().to_string()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

/// Verifies an ed25519 backend signature over `msg`. Panics if the signature is invalid.
pub fn verify_backend_sig(
    e: &Env,
    signer: &BytesN<32>,
    msg: &Bytes,
    sig: &BytesN<64>,
) {
    e.crypto().ed25519_verify(signer, msg, sig);
}
