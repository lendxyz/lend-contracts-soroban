use soroban_sdk::{Bytes, Env, String};

/// `prefix || suffix` as a `String`. The suffix is caller-supplied and
/// unbounded, so this keeps the one append.
pub fn prefixed(e: &Env, prefix: &[u8], suffix: &String) -> String {
    let mut bytes = Bytes::from_slice(e, prefix);
    bytes.append(&Bytes::from(suffix));
    String::from(bytes)
}

/// `"opLEND-<id>"`, built in linear memory as a single host allocation.
pub fn op_symbol(e: &Env, mut id: u32) -> String {
    const PREFIX: &[u8] = b"opLEND-";
    // u32::MAX is 10 digits.
    let mut buf = [0u8; PREFIX.len() + 10];
    buf[..PREFIX.len()].copy_from_slice(PREFIX);

    let mut digits = [0u8; 10];
    let mut i = digits.len();
    if id == 0 {
        i -= 1;
        digits[i] = b'0';
    }
    while id > 0 {
        i -= 1;
        digits[i] = b'0' + (id % 10) as u8;
        id /= 10;
    }

    let end = PREFIX.len() + (digits.len() - i);
    buf[PREFIX.len()..end].copy_from_slice(&digits[i..]);
    String::from_bytes(e, &buf[..end])
}
