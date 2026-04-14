use soroban_sdk::{Bytes, String};

pub fn concat_str(s1: String, s2: String) -> String {
    let mut bs1 = Bytes::from(s1);
    let bs2 = Bytes::from(s2);
    bs1.append(&bs2);

    String::from(bs1)
}
