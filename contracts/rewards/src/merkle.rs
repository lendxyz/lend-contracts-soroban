use soroban_sdk::{Address, Bytes, BytesN, Env, Vec};

/// Computes the leaf hash for a (user, balance) pair.
///
/// EVM: `keccak256(abi.encodePacked(user, balance))` over a 20-byte address +
/// 32-byte uint256. On Soroban we hash the address strkey (its `to_string`
/// bytes, e.g. "G..."/"C...") followed by the 16-byte big-endian i128 balance.
/// The backend must build leaves the same way.
pub fn leaf(e: &Env, user: &Address, balance: i128) -> BytesN<32> {
    let mut buf = Bytes::from(user.to_string());
    buf.append(&Bytes::from_slice(e, &balance.to_be_bytes()));
    e.crypto().keccak256(&buf).to_bytes()
}

/// Sorted-pair keccak256, matching OpenZeppelin `MerkleProof._hashPair`
/// (commutative: the two children are ordered before hashing).
fn hash_pair(e: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let (lo, hi) = if a.to_array() <= b.to_array() {
        (a, b)
    } else {
        (b, a)
    };
    let mut buf = Bytes::from_slice(e, &lo.to_array());
    buf.append(&Bytes::from_slice(e, &hi.to_array()));
    e.crypto().keccak256(&buf).to_bytes()
}

/// Verifies a merkle proof for `leaf` against `root` (OZ-compatible).
pub fn verify(
    e: &Env,
    proof: &Vec<BytesN<32>>,
    root: &BytesN<32>,
    leaf: BytesN<32>,
) -> bool {
    let mut computed = leaf;
    for p in proof.iter() {
        computed = hash_pair(e, &computed, &p);
    }
    computed == *root
}
