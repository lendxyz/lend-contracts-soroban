#!/usr/bin/env bash
#
# The pinned Stellar CLI. Single source of truth: scripts/common.sh sources this
# and refuses to run under any other version, and .github/workflows/ci.yml
# installs exactly this release and checks it the same way.
#
#   source "$SCRIPT_DIR/stellar-cli.sh"
#   require_stellar_cli || exit 1

STELLAR_CLI_VERSION=30.0.1
STELLAR_XDR_VERSION=30.0.0

# require_stellar_cli — fail unless `stellar --version` reports exactly the
# pinned CLI and stellar-xdr versions.
require_stellar_cli() {
  local out cli xdr
  if ! out="$(stellar --version 2>/dev/null)"; then
    echo "error: stellar CLI not found; install v$STELLAR_CLI_VERSION:" >&2
    echo "         cargo install --locked stellar-cli --version $STELLAR_CLI_VERSION" >&2
    return 1
  fi
  cli="$(awk '$1 == "stellar" { print $2 }' <<<"$out")"
  xdr="$(awk '$1 == "stellar-xdr" { print $2 }' <<<"$out")"
  if [ "$cli" != "$STELLAR_CLI_VERSION" ] || [ "$xdr" != "$STELLAR_XDR_VERSION" ]; then
    echo "error: need stellar $STELLAR_CLI_VERSION (stellar-xdr $STELLAR_XDR_VERSION)," >&2
    echo "       found stellar ${cli:-?} (stellar-xdr ${xdr:-?}). Install the pinned one:" >&2
    echo "         cargo install --locked stellar-cli --version $STELLAR_CLI_VERSION" >&2
    return 1
  fi
}
