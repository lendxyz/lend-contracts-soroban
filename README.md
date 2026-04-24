# Lend contracts soroban

This project is meant to be a fundraising contract for on-chain securities and is constitued of two contracts: the `Factory` and `OpLend` contracts.

- The `Factory` can handle operation and funding management and also acts as an `OpLend` deployer.
- The `OpLend` is basically a token contract with a few more methods to control certain permissions in order to comply with the legal framework associated with the tokenized securities.

## Work in progress

The project is still missing a few important features, but for now here is some PoC deployments based on this repository:

- Factory address: [CATQIEC3UAAEPYBPFBJWHGY3WYQJJZ344NXAADZ7HWICA2SWG7NU5III](https://testnet.stellarchain.io/contracts/CATQIEC3UAAEPYBPFBJWHGY3WYQJJZ344NXAADZ7HWICA2SWG7NU5III)
- OpLend address: [CCW5RC53PE4DOL6IS6D34DEKRDELTB63CJ3A5OWOLCLVM43CL7TYJZRL](https://testnet.stellarchain.io/contracts/CCW5RC53PE4DOL6IS6D34DEKRDELTB63CJ3A5OWOLCLVM43CL7TYJZRL)

## Project Structure

This repository uses the recommended structure for a Soroban project:

```text
.
├── contracts
│   ├── factory
│   │   ├── src
│   │   │   ├── lib.rs
│   │   │   └── test.rs
│   │   └── Cargo.toml
│   └── op-lend
│       ├── src
│       │   ├── lib.rs
│       │   └── test.rs
│       └── Cargo.toml
├── Cargo.toml
└── README.md
```

- New Soroban contracts can be put in `contracts`, each in their own directory.
- Contracts should have their own `Cargo.toml` files that rely on the top-level `Cargo.toml` workspace for their dependencies.
- Frontend libraries can be added to the top-level directory as well.
