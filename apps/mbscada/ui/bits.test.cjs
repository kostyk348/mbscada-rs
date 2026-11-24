// node --test for the pure bit helpers of apps/mbscada/ui/bits.js.
"use strict";
const test = require("node:test");
const assert = require("node:assert");
const B = require("./bits.js");

test("labels are namespaced per poll:addr and empty groups are cleaned up", () => {
  const cfg = {};
  B.setBitLabel(cfg, 1, 100, 5, "Авария");
  B.setBitLabel(cfg, 1, 100, 0, "Сеть");
  assert.equal(B.bitLabel(cfg, 1, 100, 5), "Авария");
  assert.deepEqual(Object.keys(B.bitLabelsOf(cfg, 1, 100)).sort(), ["0", "5"]);
  // other rows stay untouched
  assert.equal(B.bitLabelsOf(cfg, 1, 101), null);
  // clearing the last name removes the group
  B.setBitLabel(cfg, 1, 100, 5, "");
  B.setBitLabel(cfg, 1, 100, 0, "  ");
  assert.equal(B.bitLabelsOf(cfg, 1, 100), null);
  assert.deepEqual(cfg.bitNames, {});
});

test("a cleared name keeps the other names of the same word", () => {
  const cfg = {};
  B.setBitLabel(cfg, 2, 0, 3, "Работа");
  B.setBitLabel(cfg, 2, 0, 7, "Стоп");
  B.setBitLabel(cfg, 2, 0, 3, "");
  assert.equal(B.bitLabel(cfg, 2, 0, 3), "");
  assert.equal(B.bitLabel(cfg, 2, 0, 7), "Стоп");
});

test("word value comes from regs[0] and multi-register rows have no word", () => {
  assert.equal(B.wordValueOf({ regs: [0x8005] }), 0x8005);
  assert.equal(B.wordValueOf({ regs: [0x1234, 0x5678] }), 0x1234);
  assert.equal(B.wordValueOf({ regs: [] }), null);
  assert.equal(B.wordValueOf({}), null);
  assert.equal(B.wordValueOf(null), null);
  assert.equal(B.wordValueOf({ regs: [NaN] }), null);
});

test("bit extraction is masked to 16 bits", () => {
  assert.equal(B.bitOf(0x0004, 2), true);
  assert.equal(B.bitOf(0x0004, 0), false);
  assert.equal(B.bitOf(0x8000, 15), true);
  assert.equal(B.bitOf(0x8000, 16), false); // beyond a word: not a bit
  assert.equal(B.bitOf(null, 0), false);
});

test("bitsTitle lists named bits from high to low with live states", () => {
  const labels = { "0": "Сеть", "5": "Авария" };
  assert.equal(B.bitsTitle(0x0021, labels), "Авария=1, Сеть=1");
  assert.equal(B.bitsTitle(0x0020, labels), "Авария=1, Сеть=0");
  assert.equal(B.bitsTitle(0, labels), "Авария=0, Сеть=0");
  assert.equal(B.bitsTitle(0x0021, null), "");
});

test("bit condition text carries the name and the state", () => {
  const labels = { "5": "Авария" };
  assert.equal(B.bitCondText(5, true, labels, "установлен", "сброшен"),
    "бит 5 — Авария установлен");
  assert.equal(B.bitCondText(5, false, labels, "установлен", "сброшен"),
    "бит 5 — Авария сброшен");
  // an unnamed bit is still readable
  assert.equal(B.bitCondText(3, true, null, "установлен", "сброшен"),
    "бит 3 установлен");
});

test("bitsKey namespaces by poll and address", () => {
  assert.equal(B.bitsKey(12, 40123), "12:40123");
});
