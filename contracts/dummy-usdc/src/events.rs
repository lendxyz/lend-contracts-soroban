use soroban_sdk::{contractevent, Address, Env};

// Event structs use the `contractevent` macro (replaces `Events::publish`).
// The struct name becomes the leading topic in snake_case, mirroring the
// Stellar Asset Contract: `transfer`, `mint`, `burn`, `approve`, `set_admin`.

#[contractevent(data_format = "vec")]
pub struct Approve {
    #[topic]
    pub from: Address,
    #[topic]
    pub spender: Address,
    pub amount: i128,
    pub expiration_ledger: u32,
}

#[contractevent(data_format = "single-value")]
pub struct Transfer {
    #[topic]
    pub from: Address,
    #[topic]
    pub to: Address,
    pub amount: i128,
}

#[contractevent(data_format = "single-value")]
pub struct Mint {
    #[topic]
    pub admin: Address,
    #[topic]
    pub to: Address,
    pub amount: i128,
}

#[contractevent(data_format = "single-value")]
pub struct Burn {
    #[topic]
    pub from: Address,
    pub amount: i128,
}

#[contractevent(data_format = "single-value")]
pub struct SetAdmin {
    #[topic]
    pub admin: Address,
    pub new_admin: Address,
}

pub fn approve(
    e: &Env,
    from: Address,
    spender: Address,
    amount: i128,
    expiration_ledger: u32,
) {
    Approve {
        from,
        spender,
        amount,
        expiration_ledger,
    }
    .publish(e);
}

pub fn transfer(e: &Env, from: Address, to: Address, amount: i128) {
    Transfer { from, to, amount }.publish(e);
}

pub fn mint(e: &Env, admin: Address, to: Address, amount: i128) {
    Mint { admin, to, amount }.publish(e);
}

pub fn burn(e: &Env, from: Address, amount: i128) {
    Burn { from, amount }.publish(e);
}

pub fn set_admin(e: &Env, admin: Address, new_admin: Address) {
    SetAdmin { admin, new_admin }.publish(e);
}
