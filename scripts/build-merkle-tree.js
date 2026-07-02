#!/usr/bin/env node
//
// Build a LendRewards merkle tree from a list of (address, balance) recipients
// and emit the root, the per-recipient proofs, and the total allocation.
//
// Leaf     = keccak256(user_strkey_ascii || balance_i128_be16)
// Internal = sorted-pair keccak256 (OpenZeppelin MerkleProof._hashPair)
// This mirrors contracts/rewards/src/merkle.rs exactly (verified against the
// contract's own keccak256 leaf output).
//
// Usage:
//   node scripts/build-merkle-tree.js <recipients.json> [out.json]
//
// Input JSON — either an object { "G...": "1000000", ... } or an array
// [ { "address": "G...", "balance": "1000000" }, ... ]. Balances are reward-
// token base units (integers; strings recommended to avoid float loss).
//
// Output JSON (also written to <out.json> when given):
//   {
//     "root": "0x<64 hex>",
//     "total_allocation": "3500000",
//     "count": 2,
//     "claims": [
//       { "address": "G...", "balance": "1000000", "proof": ["0x..", ...] },
//       ...
//     ]
//   }
//
// The `root` is what you pass to distribute_op_rewards / distribute_ref_rewards;
// each claim's { balance, proof } is what a user passes to claim_op_epoch /
// claim_ref_epoch. distribute-op-rewards.sh consumes this file end to end.

"use strict";

// ---- keccak256 (pure JS, no deps) -----------------------------------------
// Verified against known vectors and the contract's on-chain leaf output.
function keccak256(input) {
  const data = Buffer.from(input);
  const RC = [
    0x1n, 0x8082n, 0x800000000000808an, 0x8000000080008000n, 0x808bn,
    0x80000001n, 0x8000000080008081n, 0x8000000000008009n, 0x8an, 0x88n,
    0x80008009n, 0x8000000an, 0x8000808bn, 0x800000000000008bn,
    0x8000000000008089n, 0x8000000000008003n, 0x8000000000008002n,
    0x8000000000000080n, 0x800an, 0x800000008000000an, 0x8000000080008081n,
    0x8000000000008080n, 0x80000001n, 0x8000000080008008n,
  ];
  const ROT = [0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45,
    15, 21, 8, 18, 2, 61, 56, 14];
  const MASK = (1n << 64n) - 1n;
  const rol = (x, n) => (n === 0n ? x : ((x << n) | (x >> (64n - n))) & MASK);
  const rate = 136; // keccak256 rate in bytes
  const padlen = Math.ceil((data.length + 1) / rate) * rate;
  const buf = Buffer.alloc(padlen);
  data.copy(buf);
  buf[data.length] ^= 0x01; // keccak (not SHA3) domain padding
  buf[padlen - 1] ^= 0x80;
  const A = new Array(25).fill(0n);
  for (let off = 0; off < padlen; off += rate) {
    for (let i = 0; i < rate / 8; i++) A[i] ^= buf.readBigUInt64LE(off + i * 8);
    for (let round = 0; round < 24; round++) {
      const C = new Array(5);
      for (let x = 0; x < 5; x++) C[x] = A[x] ^ A[x + 5] ^ A[x + 10] ^ A[x + 15] ^ A[x + 20];
      const D = new Array(5);
      for (let x = 0; x < 5; x++) D[x] = C[(x + 4) % 5] ^ rol(C[(x + 1) % 5], 1n);
      for (let x = 0; x < 5; x++) for (let y = 0; y < 25; y += 5) A[x + y] ^= D[x];
      const B = new Array(25).fill(0n);
      for (let x = 0; x < 5; x++)
        for (let y = 0; y < 5; y++)
          B[y + ((2 * x + 3 * y) % 5) * 5] = rol(A[x + y * 5], BigInt(ROT[x + y * 5]));
      for (let x = 0; x < 5; x++)
        for (let y = 0; y < 5; y++)
          A[x + y * 5] = B[x + y * 5] ^ (~B[((x + 1) % 5) + y * 5] & B[((x + 2) % 5) + y * 5]);
      A[0] ^= RC[round];
      for (let i = 0; i < 25; i++) A[i] &= MASK;
    }
  }
  const out = Buffer.alloc(32);
  for (let i = 0; i < 4; i++) out.writeBigUInt64LE(A[i] & MASK, i * 8);
  return out;
}

// ---- leaf / tree ----------------------------------------------------------
function i128be(v) {
  // two's-complement 16-byte big-endian, matching i128::to_be_bytes()
  let n = BigInt(v);
  if (n >= 1n << 127n || n < -(1n << 127n)) {
    throw new Error(`balance out of i128 range: ${v}`);
  }
  if (n < 0n) n = (1n << 128n) + n;
  const b = Buffer.alloc(16);
  for (let i = 15; i >= 0; i--) {
    b[i] = Number(n & 0xffn);
    n >>= 8n;
  }
  return b;
}

function leaf(addr, balance) {
  return keccak256(Buffer.concat([Buffer.from(addr, "ascii"), i128be(balance)]));
}

function hashPair(a, b) {
  const [lo, hi] = Buffer.compare(a, b) <= 0 ? [a, b] : [b, a];
  return keccak256(Buffer.concat([lo, hi]));
}

function buildTree(entries) {
  if (entries.length === 0) throw new Error("no recipients");
  const leaves = entries.map((e) => leaf(e.address, e.balance));
  const layers = [leaves];
  let level = leaves;
  while (level.length > 1) {
    const next = [];
    for (let i = 0; i < level.length; i += 2) {
      next.push(i + 1 < level.length ? hashPair(level[i], level[i + 1]) : level[i]);
    }
    layers.push(next);
    level = next;
  }
  const root = layers[layers.length - 1][0];
  const claims = entries.map((e, idx) => {
    const proof = [];
    let index = idx;
    for (let l = 0; l < layers.length - 1; l++) {
      const sib = index ^ 1;
      if (sib < layers[l].length) proof.push("0x" + layers[l][sib].toString("hex"));
      index = Math.floor(index / 2);
    }
    return { address: e.address, balance: String(e.balance), proof };
  });
  return { root: "0x" + root.toString("hex"), claims };
}

// ---- self-verify (defense in depth) ---------------------------------------
function verifyClaim(claim, rootHex) {
  let c = leaf(claim.address, claim.balance);
  for (const p of claim.proof) c = hashPair(c, Buffer.from(p.replace(/^0x/, ""), "hex"));
  return "0x" + c.toString("hex") === rootHex;
}

// ---- input parsing ---------------------------------------------------------
function parseEntries(raw) {
  const data = JSON.parse(raw);
  let entries;
  if (Array.isArray(data)) {
    entries = data.map((e) => ({ address: e.address, balance: e.balance }));
  } else if (data && typeof data === "object") {
    entries = Object.entries(data).map(([address, balance]) => ({ address, balance }));
  } else {
    throw new Error("input must be an object or array");
  }
  const seen = new Set();
  for (const e of entries) {
    if (typeof e.address !== "string" || !/^[GC][A-Z2-7]{55}$/.test(e.address)) {
      throw new Error(`invalid Stellar address: ${JSON.stringify(e.address)}`);
    }
    if (seen.has(e.address)) throw new Error(`duplicate address: ${e.address}`);
    seen.add(e.address);
    const b = BigInt(e.balance); // throws on non-integer
    if (b <= 0n) throw new Error(`balance must be > 0: ${e.address} = ${e.balance}`);
    e.balance = b;
  }
  return entries;
}

function main() {
  const [, , inPath, outPath] = process.argv;
  if (!inPath) {
    process.stderr.write("usage: build-merkle-tree.js <recipients.json> [out.json]\n");
    process.exit(2);
  }
  const fs = require("fs");
  const entries = parseEntries(fs.readFileSync(inPath, "utf8"));
  const { root, claims } = buildTree(entries);
  const total = entries.reduce((a, e) => a + e.balance, 0n);
  for (const c of claims) {
    if (!verifyClaim(c, root)) throw new Error(`self-verify failed for ${c.address}`);
  }
  const result = {
    root,
    total_allocation: String(total),
    count: entries.length,
    claims,
  };
  const json = JSON.stringify(result, null, 2);
  if (outPath) fs.writeFileSync(outPath, json + "\n");
  process.stdout.write(json + "\n");
}

main();
