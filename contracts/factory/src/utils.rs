use soroban_sdk::{Bytes, Env, String};

pub fn concat_str(s1: String, s2: String) -> String {
    let mut bs1 = Bytes::from(s1);
    let bs2 = Bytes::from(s2);
    bs1.append(&bs2);

    String::from(bs1)
}

pub fn u32_to_string(env: &Env, mut n: u32) -> String {
    let mut buf = [0u8; 10];
    let mut i = buf.len();

    if n == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
    }

    let digits = Bytes::from_slice(env, &buf[i..]);
    String::from(digits)
}
