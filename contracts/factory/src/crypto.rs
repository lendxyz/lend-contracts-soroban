use soroban_sdk::{Address, Bytes, BytesN, Env, String};

/// Reads the stored backend ed25519 public key.
pub fn read_backend_signer(e: &Env) -> BytesN<32> {
    e.storage()
        .instance()
        .get(&crate::types::DataKey::BackendSigner)
        .expect("backend signer not set")
}

/// Canonical message for on-chain invest / predeposit, mirroring the EVM
/// `keccak256(abi.encodePacked("ONCHAIN_INVEST", address(this), chainid, id, user, amount, nonce))`
/// minus chainid (contract address domain-separates per deployment).
pub fn build_invest_message(
    e: &Env,
    id: u32,
    user: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    let mut msg = Bytes::from_slice(e, b"ONCHAIN_INVEST");
    msg.append(&Bytes::from(e.current_contract_address().to_string()));
    msg.append(&Bytes::from_slice(e, &id.to_be_bytes()));
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

/// Canonical message for fiat invest.
pub fn build_fiat_invest_message(
    e: &Env,
    id: u32,
    user: &Address,
    oplend_holder: &Address,
    amount: i128,
    nonce: &String,
) -> Bytes {
    let mut msg = Bytes::from_slice(e, b"FIAT_INVEST");
    msg.append(&Bytes::from(e.current_contract_address().to_string()));
    msg.append(&Bytes::from_slice(e, &id.to_be_bytes()));
    msg.append(&Bytes::from(user.to_string()));
    msg.append(&Bytes::from(oplend_holder.to_string()));
    msg.append(&Bytes::from_slice(e, &amount.to_be_bytes()));
    msg.append(&Bytes::from(nonce.clone()));
    msg
}

/// Verifies an ed25519 backend signature over `msg`. Panics if invalid.
pub fn verify_backend_sig(
    e: &Env,
    signer: &BytesN<32>,
    msg: &Bytes,
    sig: &BytesN<64>,
) {
    e.crypto().ed25519_verify(signer, msg, sig);
}

/// Checks a nonce is unused, then marks it used. Panics on replay.
pub fn consume_nonce(e: &Env, nonce: &String) {
    let key = crate::types::DataKey::UsedNonce(nonce.clone());
    if e.storage().persistent().has(&key) {
        panic!("nonce already used");
    }
    e.storage().persistent().set(&key, &true);
}
