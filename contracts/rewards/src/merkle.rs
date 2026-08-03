use soroban_sdk::{Address, Bytes, BytesN, Env, Vec};

const STRKEY_LEN: usize = 56;

pub fn leaf(e: &Env, user: &Address, balance: i128) -> BytesN<32> {
    let mut buf = [0u8; STRKEY_LEN + 16];
    user.to_string().copy_into_slice(&mut buf[..STRKEY_LEN]);
    buf[STRKEY_LEN..].copy_from_slice(&balance.to_be_bytes());

    e.crypto().keccak256(&Bytes::from_slice(e, &buf)).to_bytes()
}

pub fn verify(
    e: &Env,
    proof: &Vec<BytesN<32>>,
    root: &BytesN<32>,
    leaf: BytesN<32>,
) -> bool {
    let mut computed = leaf.to_array();

    for sibling in proof.iter() {
        let sibling = sibling.to_array();
        let mut pair = [0u8; 64];

        let (lo, hi) = if computed <= sibling {
            (&computed, &sibling)
        } else {
            (&sibling, &computed)
        };

        pair[..32].copy_from_slice(lo);
        pair[32..].copy_from_slice(hi);
        computed = e
            .crypto()
            .keccak256(&Bytes::from_slice(e, &pair))
            .to_array();
    }

    computed == root.to_array()
}
