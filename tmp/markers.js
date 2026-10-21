#!/usr/bin/env node
// List all string markers (FF FE FF + len) with their offsets and texts.
const fs = require("fs");
const path = require("path");

const file = process.argv[2];
const b = fs.readFileSync(file);
const MARK = [0xff, 0xfe, 0xff];
const marks = [];
for (let i = 0; i + 4 <= b.length; i++) {
  if (b[i] === MARK[0] && b[i + 1] === MARK[1] && b[i + 2] === MARK[2]) {
    const ln = b[i + 3];
    let text = "";
    if (ln >= 1 && ln <= 100) {
      const a = i + 4;
      const end = a + ln * 2;
      if (end <= b.length) {
        for (let j = a; j < end; j += 2) {
          text += String.fromCharCode(b[j] | (b[j + 1] << 8));
        }
      }
    }
    marks.push({ o: i, ln, text });
    i += 4 + Math.max(ln, 0) * 2;
  }
}
marks.forEach((m, idx) => console.log(`#${String(idx + 1).padStart(3)} @${String(m.o).padStart(6)} len=${String(m.ln).padStart(3)} ${JSON.stringify(m.text)}`));
console.log(`total markers: ${marks.length}`);

// Detect the record rhythm between markers: bytes before current marker, after prev marker's string.
console.log("\nrecord boundaries (prev-marker-end -> marker):");
for (let k = 1; k < marks.length; k++) {
  const prevEnd = marks[k - 1].o + 4 + (marks[k - 1].ln >= 1 ? marks[k - 1].ln * 2 : 0);
  const cur = marks[k].o;
  const seg = b.slice(prevEnd, cur);
  // compress as "NN*U32" groups
  const groups = [];
  let i = 0;
  while (i + 4 <= seg.length) {
    const word = seg.readUInt32LE(i);
    groups.push(word);
    i += 4;
  }
  // summarize: count leading equal words
  const lines = [[groups[0], 1]];
  for (let g = 1; g < groups.length; g++) {
    if (lines[lines.length - 1][0] === groups[g]) lines[lines.length - 1][1]++;
    else lines.push([groups[g], 1]);
  }
  const sum = lines.map(([w, n]) => `0x${w.toString(16).padStart(8, "0")}${n > 1 ? "x" + n : ""}`).join(" ");
  console.log(`  ${String(k).padStart(4)}: prevEnd=${prevEnd} -> cur=${cur}  ${sum}`);
}
