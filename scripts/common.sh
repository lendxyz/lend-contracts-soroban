#!/usr/bin/env bash
#
# Shared configuration for every script in scripts/. Sourced, never executed:
#
#   SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   source "$SCRIPT_DIR/common.sh"
#
# Picks a profile from $NETWORK (default: testnet) and fills in the contract
# addresses, the signing identity and the signing method for that network.
# Every value is assigned with `: "${VAR:=default}"`, so the environment always
# wins and one-off overrides keep working:
#
#   NETWORK=mainnet ./scripts/create-operation.sh
#   FACTORY_ID=C... ./scripts/start-operation.sh
#
# Mainnet signs with a Ledger hardware wallet (see the mainnet profile below):
# $SOURCE is only the *address* of the ledger account and every stellar call
# carries "${SIGN_ARGS[@]}", which routes signing to the device.

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  echo "error: common.sh is a config file; source it, don't run it" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WASM_DIR="$REPO_ROOT/target/wasm32v1-none/release"

NETWORK="${NETWORK:-mainnet}"

case "$NETWORK" in
  # ---------------------------------------------------------------- testnet --
  testnet)
    # Local key from `stellar keys ls`; signs and pays.
    : "${SOURCE:=lend-testnet}"
    : "${SIGN_WITH_LEDGER:=0}"

    # Deployments — keep in sync with DEPLOYMENTS.md.
    : "${FACTORY_ID:=CCHD4SJKOLOTMSITJ5KBBWTWKRUH7CJYJB777RPFD3LBHKIMGVAGRYZD}"
    : "${REWARDS_ID:=CASVCVOAEAQCH5M3SYLCKKD3LRPA4JO2776EIKJQ2FJOF4KFONG223YW}"
    : "${DUMMY_USDC_ID:=CCO56ZVZPLGELBZGAVLTNC5GPZUIF4SIAIGPYNHWBRUSKBLC7HPF5QPN}"

    # DummyUSDC stands in for Circle USDC on testnet, and is now deployed with 7
    # decimals to match it. NOTE: the id above is the older 6-decimal token; a
    # 7-decimal redeploy needs a new FACTORY_ID too, since the factory caches
    # the USDC scale at initialize and never re-reads it.
    : "${USDC:=$DUMMY_USDC_ID}"
    # Reflector FX oracle: the fiat/forex feed (base USD, 14 decimals, carries
    # EUR). Verified 2026-06-02 on-chain.
    : "${ORACLE:=CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W}"
    # Backend ed25519 key that authorizes invest / predeposit / fiat-invest.
    : "${BACKEND_SIGNER:=GAOQ67SJWIJSKZXKZTPWIQTRI6EGTDVDLRXSWUZHMMPGS3MVNGCOVEMA}"

    : "${RPC_URL:=https://soroban-rpc.testnet.stellar.gateway.fm}"
    : "${NETWORK_PASSPHRASE:=Test SDF Network ; September 2015}"
    : "${API_BASE:=https://api-staging.lend.xyz/v1}"
    ;;

  # ---------------------------------------------------------------- mainnet --
  mainnet | pubnet | public)
    NETWORK=mainnet

    # Ledger signing. LEDGER_HD_PATH is the only knob: it picks both the account
    # (SOURCE=ledger:<index>, resolved off the device) and the signing key
    # (--hd-path), so the two can never drift apart. The device has to be
    # plugged in and unlocked with the Stellar app open.
    #
    # Freighter's default account is m/44'/148'/0' -> index 0. To find the index
    # of an address Freighter already shows you, match it in this list:
    #   for i in 0 1 2 3 4; do echo "$i $(stellar keys public-key ledger:$i)"; done
    # (Freighter's "Ledger N" account *name* is not the index.)
    : "${LEDGER_HD_PATH:=0}"
    : "${SOURCE:=ledger:$LEDGER_HD_PATH}"
    : "${SIGN_WITH_LEDGER:=1}"
    # To resolve the address without the device, park it in an identity instead
    # and keep LEDGER_HD_PATH pointing at that same index:
    #   stellar keys add lend-mainnet --public-key "$(stellar keys public-key ledger:0)"
    #   SOURCE=lend-mainnet ./scripts/...

    # TODO: fill in after the mainnet deploy; also record in DEPLOYMENTS.md.
    : "${FACTORY_ID:=}"
    : "${REWARDS_ID:=}"

    # Circle USDC SAC + Reflector FX oracle. Verified 2026-06-02.
    : "${USDC:=CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75}"
    : "${ORACLE:=CBKGPWGKSKZF52CFHMTRR23TBWTPMRDIYZ4O2P5VS65BMHYH4DXMCJZC}"
    : "${BACKEND_SIGNER:=GCD42CYVB5P3LSSDTEPGRYNQSVP555B5TFMXU7FZZL65W54NUC7FVILX}"

    : "${RPC_URL:=https://soroban-rpc.mainnet.stellar.gateway.fm}"
    : "${NETWORK_PASSPHRASE:=Public Global Stellar Network ; September 2015}"
    : "${API_BASE:=https://api.lend.xyz/v1}"
    ;;

  *)
    echo "error: unknown NETWORK '$NETWORK' (expected testnet | mainnet)" >&2
    exit 1
    ;;
esac

# Network flags spliced into every stellar call as "${NETWORK_ARGS[@]}". The CLI
# ships `mainnet` with a placeholder rpc url ("Bring Your Own: https://..."), so
# --network mainnet alone fails; and once --rpc-url is given the CLI stops taking
# the passphrase from --network. Passing both bypasses `stellar network ls`
# config entirely, so a run does not depend on machine-local CLI state.
[ -n "$RPC_URL" ] && [ -n "$NETWORK_PASSPHRASE" ] || {
  echo "error: RPC_URL and NETWORK_PASSPHRASE must both be set for $NETWORK" >&2
  exit 1
}
NETWORK_ARGS=(--rpc-url "$RPC_URL" --network-passphrase "$NETWORK_PASSPHRASE")

# Signing flags spliced into every stellar call as "${SIGN_ARGS[@]}". Empty
# means "sign locally with the key behind $SOURCE"; with a ledger the device
# must be plugged in, unlocked, running the Stellar app, and each transaction
# has to be approved on-device.
LEDGER_HD_PATH="${LEDGER_HD_PATH:-0}"
SIGN_ARGS=()
if [ "$SIGN_WITH_LEDGER" = "1" ]; then
  SIGN_ARGS=(--sign-with-ledger --hd-path "$LEDGER_HD_PATH")
  echo "==> Signing with Ledger (hd path $LEDGER_HD_PATH). Please approve on your device" >&2
fi

# req VAR... — abort unless every named variable is set and non-empty.
req() {
  local v
  for v in "$@"; do
    [ -n "${!v:-}" ] || {
      echo "error: \$$v is required (NETWORK=$NETWORK); set it in the env or in scripts/common.sh" >&2
      exit 1
    }
  done
}

# need_bin BIN... — abort unless every binary is on PATH.
need_bin() {
  local b
  for b in "$@"; do
    command -v "$b" >/dev/null || { echo "error: $b is required" >&2; exit 1; }
  done
}

# source_address — the G... address $SOURCE signs with. $SOURCE is a named CLI
# identity on testnet but a bare address in the ledger setup, so resolve both.
source_address() {
  case "$SOURCE" in
    G*) printf '%s' "$SOURCE" ;;
    *) stellar keys address "$SOURCE" ;;
  esac
}

# strkey_to_hex <G...|64 hex> — raw 32-byte ed25519 pubkey as 64 hex chars,
# which is what the CLI wants for BytesN<32> args. A G... strkey is
# version byte + 32-byte payload + 2-byte crc; hex input passes through.
strkey_to_hex() {
  case "$1" in
    G*)
      need_bin python3
      python3 - "$1" <<'PY'
import base64, sys
s = sys.argv[1]
raw = base64.b32decode(s + "=" * ((8 - len(s) % 8) % 8))
sys.stdout.write(raw[1:33].hex())
PY
      ;;
    *) printf '%s' "${1#0x}" ;;
  esac
}
