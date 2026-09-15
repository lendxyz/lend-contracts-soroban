#!/usr/bin/env node
//
// Produce a backend `FIAT_INVEST` authorisation: the ed25519 signature the
// factory's `fiat_invest` verifies, signed with the backend signer's secret.
//
// Message = "FIAT_INVEST" || factory_strkey_ascii(56) || id_u32_be(4)
//           || user_strkey_ascii(56) || holder_strkey_ascii(56)
//           || shares_i128_be(16) || nonce_utf8
// This mirrors build_fiat_invest_message in contracts/factory/src/crypto.rs
// byte for byte (fixed-width buffer, so every address must be a 56-char
// strkey), and the signature is raw ed25519 over those bytes — what
// e.crypto().ed25519_verify expects.
//
// Usage — everything comes from the environment so the secret never lands in
// argv (visible in `ps`):
//
//   SIGNER_SECRET=S... FACTORY_ID=C... OP_ID=1 SHARES=1000000 \
//   INVESTOR=G... HOLDER=G... [NONCE=fiat-1] node scripts/sign-fiat-invest.js
//
// Output JSON on stdout:
//   {
//     "nonce":       "fiat-1-1750000000-a1b2c3",  // generated when NONCE unset
//     "signature":   "<128 hex>",                 // BytesN<64> for the CLI
//     "signer_hex":  "<64 hex>",                  // pubkey derived from SIGNER_SECRET
//     "message_hex": "<hex>"                      // the signed bytes, for audit
//   }
//
// fiat-invest.sh consumes this and forwards nonce + signature to the contract.

"use strict";

const crypto = require("crypto");

const STRKEY_LEN = 56; // G... / C... strkey, as copied into the message buffer

// ---- strkey ---------------------------------------------------------------

const B32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

function b32decode(s) {
  let bits = 0;
  let val = 0;
  const out = [];
  for (const c of s) {
    if (c === "=") continue;
    const i = B32.indexOf(c);
    if (i < 0) throw new Error(`invalid base32 character ${JSON.stringify(c)}`);
    val = (val << 5) | i;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((val >>> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

// CRC16-XModem over version byte + payload, stored little-endian. Checking it
// turns a truncated or mistyped secret into an error here instead of an
// "ED25519 verification failed" from the contract.
function crc16(data) {
  let crc = 0;
  for (const b of data) {
    crc ^= b << 8;
    for (let i = 0; i < 8; i++) {
      crc = crc & 0x8000 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff;
    }
  }
  return crc;
}

// Decode a strkey to its 32-byte payload. version: 0x90 = S... seed.
function strkeyPayload(s, version, label) {
  const raw = b32decode(s);
  if (raw.length !== 35) throw new Error(`${label}: bad strkey length`);
  if (raw[0] !== version) throw new Error(`${label}: unexpected strkey type`);
  if (raw.readUInt16LE(33) !== crc16(raw.subarray(0, 33))) {
    throw new Error(`${label}: strkey checksum mismatch`);
  }
  return raw.subarray(1, 33);
}

// ---- message fields -------------------------------------------------------

function strkeyField(value, label) {
  const s = (value || "").trim();
  if (s.length !== STRKEY_LEN) {
    throw new Error(
      `${label}: expected a ${STRKEY_LEN}-char G.../C... strkey, got ${s.length} chars`,
    );
  }
  return Buffer.from(s, "ascii");
}

function u32be(value, label) {
  const n = Number(value);
  if (!Number.isInteger(n) || n < 0 || n > 0xffffffff) {
    throw new Error(`${label}: expected a u32, got ${JSON.stringify(value)}`);
  }
  const buf = Buffer.alloc(4);
  buf.writeUInt32BE(n);
  return buf;
}

function i128be(value, label) {
  if (!/^-?\d+$/.test((value || "").trim())) {
    throw new Error(`${label}: expected an integer, got ${JSON.stringify(value)}`);
  }
  let x = BigInt(value.trim());
  if (x < -(2n ** 127n) || x > 2n ** 127n - 1n) {
    throw new Error(`${label}: out of i128 range`);
  }
  if (x < 0n) x += 1n << 128n;
  const buf = Buffer.alloc(16);
  for (let i = 15; i >= 0; i--) {
    buf[i] = Number(x & 0xffn);
    x >>= 8n;
  }
  return buf;
}

// The nonce is a Soroban String appended verbatim, and it also travels through
// the shell to `stellar contract invoke`. Restrict it to printable ASCII
// without spaces so the bytes signed here are exactly the bytes the contract
// hashes, whatever the locale or quoting.
function nonceField(nonce, label) {
  if (!/^[\x21-\x7e]+$/.test(nonce)) {
    throw new Error(`${label}: must be non-empty printable ASCII without spaces`);
  }
  return Buffer.from(nonce, "utf8");
}

// ---- main -----------------------------------------------------------------

function main() {
  const env = process.env;
  const need = (name) => {
    const v = (env[name] || "").trim();
    if (!v) throw new Error(`$${name} is required`);
    return v;
  };

  const seed = strkeyPayload(need("SIGNER_SECRET"), 0x90, "SIGNER_SECRET");
  const opId = need("OP_ID");
  const nonce =
    (env.NONCE || "").trim() ||
    `fiat-${opId}-${Math.floor(Date.now() / 1000)}-${crypto
      .randomBytes(3)
      .toString("hex")}`;

  const message = Buffer.concat([
    Buffer.from("FIAT_INVEST", "ascii"),
    strkeyField(need("FACTORY_ID"), "FACTORY_ID"),
    u32be(opId, "OP_ID"),
    strkeyField(need("INVESTOR"), "INVESTOR"),
    strkeyField(need("HOLDER"), "HOLDER"),
    i128be(need("SHARES"), "SHARES"),
    nonceField(nonce, "NONCE"),
  ]);

  // ed25519 seed -> pkcs8 DER, the only private-key shape node's crypto takes.
  const der = Buffer.concat([
    Buffer.from("302e020100300506032b657004220420", "hex"),
    seed,
  ]);
  const key = crypto.createPrivateKey({ key: der, format: "der", type: "pkcs8" });
  const pub = crypto.createPublicKey(key);
  const signature = crypto.sign(null, message, key);

  // Cheap self-check: a signature the signer's own public key rejects would
  // otherwise surface as a failed simulation with no clue as to why.
  if (!crypto.verify(null, message, pub, signature)) {
    throw new Error("self-verification of the signature failed");
  }

  // SPKI DER for ed25519 is a 12-byte prefix + the raw 32-byte public key.
  const signerHex = pub
    .export({ format: "der", type: "spki" })
    .subarray(12)
    .toString("hex");

  process.stdout.write(
    `${JSON.stringify(
      {
        nonce,
        signature: signature.toString("hex"),
        signer_hex: signerHex,
        message_hex: message.toString("hex"),
      },
      null,
      2,
    )}\n`,
  );
}

try {
  main();
} catch (e) {
  process.stderr.write(`error: ${e.message}\n`);
  process.exit(1);
}
