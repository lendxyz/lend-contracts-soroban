# Deploy scripts

Bash helpers around the `stellar` CLI. Require `stellar` (v26+) installed and an
identity created (`stellar keys generate <name> --network testnet --fund`).

## `deploy.sh`

Builds the wasms, uploads the op-lend wasm, deploys the factory, and calls
`initialize`.

```bash
SOURCE=alice \
USDC=CB...           \  # USDC token contract
ORACLE=CA...         \  # Reflector EUR/USD oracle
BACKEND_SIGNER=ab12..\  # backend ed25519 pubkey, 64 hex chars (32 bytes)
NETWORK=testnet      \  # optional, default testnet
./scripts/deploy.sh
```

Prints `FACTORY_ID` and `OPLEND_WASM_HASH` on success.

The op-lend wasm is uploaded once; every operation the factory creates is a new
op-lend instance deployed from that hash.

## `create-operation.sh`

Admin-only. Deploys + registers a new operation (and its op-lend token).

```bash
SOURCE=alice \
FACTORY_ID=CC... \
OP_NAME="Alpha Fund" \
TOTAL_SHARES=1000000 \    # supply cap, 6 decimals
EUR_PER_SHARES=1000000 \  # 1 EUR per share, 6 decimals
./scripts/create-operation.sh
```

Prints the deployed op-lend token address.

## Notes

- `BACKEND_SIGNER` is the ed25519 **public** key the backend signs invest /
  whitelist messages with. The message format the backend must reproduce is in
  `contracts/factory/src/crypto.rs` and `contracts/op-lend/src/crypto.rs`.
- Confirm the live Reflector oracle address and that its `Asset::Other("EUR")`
  feed exists for your network before relying on `invest` / `get_amount_in`.
