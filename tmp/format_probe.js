#!/usr/bin/env node
// Probe Modbus Poll .mbp/.mbw byte layouts (no Python dependency).
// Reports magic, utf16 string runs, candidate trailing metadata blocks.

const fs = require("fs");
const path = require("path");

const DESK = "C:\\Users\\Lab\\Desktop";

function utf16Runs(b) {
  const runs = [];
  let i = 0;
  const n = b.length;
  while (i < n - 1) {
    if (b[i] >= 0x20 && b[i] <= 0x7e && b[i + 1] === 0) {
      const start = i;
      let txt = "";
      while (i < n - 1 && b[i] >= 0x20 && b[i] <= 0x7e && b[i + 1] === 0) {
        txt += String.fromCharCode(b[i]);
        i += 2;
      }
      if (txt.length >= 2) runs.push({ off: start, text: txt });
    } else {
      i++;
    }
  }
  return runs;
}

function u32(b, off) {
  return off + 4 <= b.length ? b.readUInt32LE(off) : null;
}

function scanTail(b) {
  for (let off = b.length - 24; off >= 16; off -= 4) {
    const pe = u32(b, off),
      sl = u32(b, off + 4),
      a1 = u32(b, off + 8),
      a2 = u32(b, off + 12),
      a3 = u32(b, off + 16),
      q = u32(b, off + 20);
    if (pe === null) break;
    if (
      pe >= 50 && pe <= 60000 &&
      sl >= 1 && sl <= 247 &&
      a1 === 1 && a2 === 0 && a3 === 0 &&
      q >= 1 && q <= 2000
    ) {
      return { off, pe, sl, q };
    }
  }
  return null;
}

for (const f of fs.readdirSync(DESK).filter((f) => f.toLowerCase().endsWith(".mbp")).sort()) {
  const full = path.join(DESK, f);
  const b = fs.readFileSync(full);
  console.log("=".repeat(70));
  console.log(`${f}  size=${b.length}`);
  if (b.length < 24) {
    console.log("  too small");
    continue;
  }
  const magic = b.readUInt32LE(0);
  console.log(`  magic=${magic} (v4=7000, v5=10010)`);
  if (magic === 7000 || magic === 10010) {
    console.log(`  [4]=${u32(b, 4)} [8]func=${u32(b, 8)} [12]addr=0x${u32(b, 12).toString(16)} [16]qty=${u32(b, 16)}`);
  }
  const tail = scanTail(b);
  if (tail) {
    console.log(`  tail@${tail.off} period=${tail.pe} slave=${tail.sl} qty=${tail.q}  (bytes to EOF: ${b.length - tail.off})`);
  } else {
    console.log("  tail: none");
  }
  const runs = utf16Runs(b);
  console.log(`  utf16 runs (${runs.length}):`);
  for (const r of runs.slice(0, 45)) console.log(`    @${String(r.off).padStart(6)}  ${JSON.stringify(r.text)}`);
  if (runs.length > 45) console.log(`    ... +${runs.length - 45} more`);
  // mmd if .mbw: list tables
}
console.log("\n--- .mbw files ---");
for (const f of fs.readdirSync(DESK).filter((f) => f.toLowerCase().endsWith(".mbw")).sort()) {
  const full = path.join(DESK, f);
  const b = fs.readFileSync(full);
  console.log(`${f}  size=${b.length}`);
  const runs = utf16Runs(b);
  for (const r of runs.slice(0, 25)) console.log(`    @${String(r.off).padStart(6)}  ${JSON.stringify(r.text)}`);
  if (runs.length > 25) console.log(`    ... +${runs.length - 25} more`);
}
