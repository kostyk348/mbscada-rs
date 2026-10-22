// Generate synthetic也能 .mbp corpus (canonical v4 layout, mirrors write_mbp).
// Runs a mini-table through the shapes real files use: named / empty / cyrillic aliases.
const fs = require("fs");
const dir = "C:/Users/Lab/Documents/mbscada-rs/crates/mb-format/testdata/mbp";
fs.mkdirSync(dir, { recursive: true });

const MARKER = Buffer.from([0xFF, 0xFE, 0xFF]);
const PAD = Buffer.from([0x55, 0x55, 0x55, 0x55]);

function u32(v) { const b = Buffer.alloc(4); b.writeUInt32LE(v >>> 0, 0); return b; }
function strMarker(s) {
  if (!s) return Buffer.concat([MARKER, Buffer.alloc(1)]);
  const units = Buffer.from(s, "utf16le");
  return Buffer.concat([MARKER, Buffer.alloc(1, units.length / 2), units]);
}
function rowGap() {
  // mid-list row record gap: PAD + 6×(FF000000) + 24 zero bytes
  const b = Buffer.alloc(4 + 24 + 24);
  PAD.copy(b, 0);
  for (let i = 0; i < 6; i++) b.writeUInt32LE(0xFF000000, 4 + i * 4);
  return b;
}
function table({ name, func, addr, rows, period, slave }) {
  const head = Buffer.concat([
    u32(7000), u32(144), u32(func), u32(addr), u32(rows.length),
  ]);
  let body = head;
  for (const alias of rows) body = Buffer.concat([body, strMarker(alias), rowGap()]);
  const tail = Buffer.concat([
    u32(period), u32(slave), u32(1), u32(0), u32(0), u32(rows.length),
    Buffer.alloc(16),
  ]);
  const closing = Buffer.concat([MARKER, Buffer.alloc(1), Buffer.alloc(4)]);
  const out = Buffer.concat([body, tail, closing]);
  fs.writeFileSync(`${dir}/${name}.mbp`, out);
  return out;
}

const files = [
  table({ name: "syn-basic", func: 3, addr: 760, rows: ["Value one", "Value two", "", "Value four"], period: 1000, slave: 7 }),
  table({ name: "syn-cyr", func: 3, addr: 16450, rows: ["Статус", "Режим работы", "Авария", "", ""], period: 500, slave: 128 }),
  table({ name: "syn-single", func: 3, addr: 16491, rows: ["Контроль связи"], period: 1000, slave: 1 }),
  table({ name: "syn-empty-aliases", func: 4, addr: 100, rows: ["", "", ""], period: 1000, slave: 3 }),
  table({ name: "syn-many", func: 3, addr: 16384, rows: [...Array(12)].map((_, i) => `Параметр ${i + 1}`), period: 2000, slave: 44 }),
];

for (const [i, f] of files.entries()) console.log(`${f.length}B`, fs.readdirSync(dir).filter(x => x.endsWith(".mbp") && x.startsWith("syn-")).length, "synthetic files ok");
