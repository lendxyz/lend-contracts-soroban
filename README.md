# Lend contracts soroban

This project is meant to be a fundraising contract for on-chain securities and is constitued of two core contracts: the `Factory` and `OpLend` contracts.

- The `Factory` can handle operation and funding management and also acts as an `OpLend` deployer.
- The `OpLend` is basically a token contract with a few more methods to control certain permissions in order to comply with the legal framework associated with the tokenized securities.

It also ships some supporting contracts:

- `LendRewards` — merkle-based reward distribution (see `contracts/rewards`).
- `DummyUSDC` — a testnet-only SEP-41 token that emulates Circle USDC: open `mint`, and **no transfer restrictions**. Use it when you want a USDC-like token you fully control on testnet instead of the shared Circle SAC. Deploy with [`scripts/deploy-dummy-usdc.sh`](scripts/README.md#deploy-dummy-usdcsh).

## Work in progress

The project is still under development, but for now you can see the [DEPLOYMENTS.md](DEPLOYMENTS.md) file for some PoC deployments based on this repository.

## Project Structure

This repository uses the recommended structure for a Soroban project:

- New Soroban contracts can be put in `contracts`, each in their own directory.
- Contracts should have their own `Cargo.toml` files that rely on the top-level `Cargo.toml` workspace for their dependencies.
- Frontend libraries can be added to the top-level directory as well.
