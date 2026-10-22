// Segment a real .mbp: head fields, string markers, gaps, tail.
const fs = require("fs");
const path = require("path");
const dir = "C:/Users/Lab/Documents/mbscada-rs/testdata/mbp";
for (const f of fs.readdirSync(dir).filter(x => x.endsWith(".mbp"))) {
  const d = fs.readFileSync(path.join(dir, f));
  const u32 = o => o + 4 <= d.length ? d.readUInt32LE(o) : null;
  const rows = [];
  let i = 12; // after magic(+ip handled below)
  // head: magic
  const magic = u32(0);
  let headEnd = 4;
  if (magic === 10010) { // v5: ip marker at 4
    const ln = d[7];
    headEnd = 8 + ln * 2;
  }
  headEnd = u32(headEnd) === 144 ? headEnd + 4 : headEnd; // [144]
  // fields after: func, addr, qty (3*4)
  const hdr = { func: u32(headEnd), addr: u32(headEnd + 4), qty: u32(headEnd + 8) };
  let pos = headEnd + 12;
  while (pos + 4 <= d.length) {
    if (d[pos] === 0xFF && d[pos+1] === 0xFE && d[pos+2] === 0xFF) {
      const ln = d[pos+3];
      let s = "";
      if (ln > 0 && ln <= 100 && pos + 4 + ln*2 <= d.length) {
        s = Buffer.from(d.subarray(pos+4, pos+4+ln*2)).toString("utf16le");
      }
      rows.push({ at: pos, ln, s: s.length > 24 ? s.slice(0,24)+"…" : s });
      pos += 4 + (ln > 0 && ln <= 100 ? ln*2 : 0);
      if (ln === 0 && rows[rows.length-1]) pos = pos; // zero-len marker ok
    }
    pos++;
  }
  // row boundaries: gap between end of string i and start of marker i+1
  const out = rows.map((r, k) => {
    const end = r.at + 4 + r.ln * 2;
    const next = k + 1 < rows.length ? rows[k+1].at : d.length;
    return { ...r, gap: next - end, gapHex: d.subarray(end, next).toString("hex").slice(0, 90) };
  });
  console.log("=== " + f + " (" + d.length + "B) magic=" + magic + " hdr=" + JSON.stringify(hdr));
  out.forEach((r, k) => console.log(
    "  [" + k + "] at=" + r.at + " ln=" + r.ln + " '" + r.s + "' gap=" + r.gap + " " + r.gapHex));
}
