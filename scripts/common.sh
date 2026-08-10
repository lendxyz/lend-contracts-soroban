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

NETWORK="${NETWORK:-testnet}"

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

    # Ledger signing. Register the device once as a named identity — that stores
    # the derived public key, the hd path and the fact that it is ledger-backed,
    # so later runs resolve the address without the device and signing routes to
    # it automatically:
    #
    #   stellar keys add lend-mainnet --ledger --hd-path 0
    #
    # Needs stellar-cli >= 26.1.0 (guarded below). The hd path belongs to the
    # identity from then on: passing --hd-path to a signing call is an error, and
    # there is no LEDGER_HD_PATH knob here. To move to another index, re-register:
    #
    #   stellar keys add lend-mainnet --ledger --hd-path 3 --overwrite
    #
    # Freighter's default account is m/44'/148'/0' -> --hd-path 0. To find the
    # index of an address Freighter already shows you, match it in this list
    # (device unlocked, Stellar app open) — its "Ledger N" account *name* is not
    # the index:
    #
    #   for i in 0 1 2 3 4; do echo "$i $(stellar keys address --ledger --hd-path $i)"; done
    #
    # Do NOT use a `ledger:N` source spec: 26.0.0 accepted it for reads but
    # cannot sign with it, and 27.x rejects it outright as an invalid name.
    : "${SOURCE:=lend-mainnet}"
    : "${SIGN_WITH_LEDGER:=1}"

    : "${FACTORY_ID:=CBJ6ECNXWX3JEIDV35KL3LB3AS4BOO23E5I4AHWNTPLEMEVAHBV4WBYM}"
    : "${REWARDS_ID:=CCSA4PQNTOZXJTFHHLBLBVU2YLNJMIFBKQAGE4IDGNJBHRRXDXO7QKN6}"

    # Circle USDC SAC + Reflector FX oracle. Verified on 10th Aug. 2026.
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

# Inclusion-fee bid, in stroops. The CLI defaults to 100, which loses the race
# whenever mainnet is above that (`stellar fees stats` reported a 200 max while
# an upload was timing out), and a Soroban submission that never gets included
# surfaces as "transaction submission timeout" — signed, sent, never in a ledger.
# This is only a cap: you pay the clearing price, and 0.01 XLM of headroom is
# noise next to a multi-XLM resource fee. Exported so every stellar call picks it
# up without threading a flag through 16 call sites.
: "${INCLUSION_FEE:=$([ "$NETWORK" = mainnet ] && echo 100000 || echo 1000)}"
export STELLAR_INCLUSION_FEE="$INCLUSION_FEE"

# Signing flags spliced into every stellar call as "${SIGN_ARGS[@]}". Empty means
# "sign locally with the key behind $SOURCE"; with a ledger the device must be
# plugged in, unlocked, running the Stellar app, and every transaction approved
# on it.
SIGN_ARGS=()
if [ "$SIGN_WITH_LEDGER" = "1" ]; then
  # Signing Soroban auth entries from a device landed in stellar-cli 26.1.0
  # (#2569). Before that, the contract subcommands ask $SOURCE for a secret and
  # die with "Ledger cannot reveal private keys" (or "Address cannot be used to
  # sign" for a plain G...), no matter what --sign-with-* says — that path only
  # covers `tx sign`, not the auth entries every contract call carries.
  CLI_VERSION="$(stellar --version 2>/dev/null | head -1 | awk '{print $2}')"
  if [ "$(printf '%s\n26.1.0\n' "$CLI_VERSION" | sort -V | head -1)" != "26.1.0" ]
  then
    echo "error: stellar-cli $CLI_VERSION cannot sign contract calls with a Ledger." >&2
    echo "       Upgrade to >= 26.1.0, then register the device once:" >&2
    echo "         stellar keys add $SOURCE --ledger --hd-path 0" >&2
    exit 1
  fi
  # No --hd-path: the identity owns it, and passing it here is rejected with
  # "--hd-path is fixed at the time a Ledger identity is added".
  SIGN_ARGS=(--sign-with-ledger)
  if [ -n "${LEDGER_HD_PATH:-}" ]; then
    echo "error: LEDGER_HD_PATH=$LEDGER_HD_PATH has no effect; the path is baked into" >&2
    echo "       the '$SOURCE' identity. Re-register to change it:" >&2
    echo "         stellar keys add $SOURCE --ledger --hd-path $LEDGER_HD_PATH --overwrite" >&2
    exit 1
  fi
  echo "==> Signing with Ledger via '$SOURCE'. Please approve on your device" >&2
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
