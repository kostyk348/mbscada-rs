#!/usr/bin/env node
// Hex-dump a byte range of a file: offset, hex, ascii.
const fs = require("fs");
const path = require("path");

function hexdump(b, from, to) {
  for (let i = from; i < to; i += 16) {
    let line = `@${String(i).padStart(5)}  `;
    let ascii = "";
    for (let j = 0; j < 16; j++) {
      if (i + j >= to) {
        line += "   ".repeat(16 - j);
        break;
      }
      const v = b[i + j];
      line += v.toString(16).padStart(2, "0") + " ";
      ascii += v >= 0x20 && v <= 0x7e ? String.fromCharCode(v) : (v >= 0x80 ? "." : ".");
    }
    console.log(line + "|" + ascii + "|");
  }
}

const file = process.argv[2];
const from = parseInt(process.argv[3] || "0", 10);
const to = parseInt(process.argv[4] || `${fs.statSync(file).size}`, 10);
const b = fs.readFileSync(file);
console.log(`${path.basename(file)} size=${b.length} range ${from}..${to}`);
hexdump(b, from, Math.min(to, b.length));
// utf16 decode helper for a run starting at off with byte length L
if (process.argv[5] === "--u16") {
  console.log("=== utf16 runs in range ===");
  let i = from;
  const n = Math.min(to, b.length);
  while (i < n) {
    if (b[i] >= 0x20 && b[i + 1] === 0) {
      // any printable incl. >0x7e
    }
    i++;
  }
  // improved: print every utf16le pair where second byte is 0x00-0x04 (latin+cyrillic)
  for (let i = from; i < n; i += 2) {
    const cp = b[i] | (b[i + 1] << 8);
    if (cp >= 0x20 && cp <= 0x04ff) {
      let s = "";
      let j = i;
      while (j < n) {
        const c = b[j] | (b[j + 1] << 8);
        if (c >= 0x20 && c <= 0x04ff) {
          s += String.fromCharCode(c);
          j += 2;
        } else break;
      }
      if (s.length >= 1) {
        console.log(`  @${i}: ${JSON.stringify(s)}`);
        i = j;
      }
    }
  }
}
