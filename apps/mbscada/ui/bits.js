// mbSCADA: bit-level helpers for 16-bit WORD rows.
// Pure functions only: no DOM, no Tauri — testable with node --test (bits.test.cjs).
// In the browser these are plain globals loaded before main.js.
"use strict";

const WORD_BITS = 16;

function bitsKey(pollId, addr) {
  return pollId + ":" + addr;
}

/// Named bits of one WORD row: cfg.bitNames["poll:addr"] = { "5": "Авария" }.
/// Returns null when the row has no named bits (callers skip the UI then).
function bitLabelsOf(cfg, pollId, addr) {
  if (!cfg || !cfg.bitNames) return null;
  const m = cfg.bitNames[bitsKey(pollId, addr)];
  return m && Object.keys(m).length ? m : null;
}

function bitLabel(cfg, pollId, addr, n) {
  const m = bitLabelsOf(cfg, pollId, addr);
  return m && m[n] ? String(m[n]) : "";
}

/// Sets (or clears, with an empty name) one named bit; empty groups are removed
/// so the saved project JSON carries no husks.
function setBitLabel(cfg, pollId, addr, n, name) {
  if (!cfg.bitNames) cfg.bitNames = {};
  const k = bitsKey(pollId, addr);
  if (!cfg.bitNames[k]) cfg.bitNames[k] = {};
  const name0 = String(name || "").trim();
  if (name0) cfg.bitNames[k][n] = name0;
  else delete cfg.bitNames[k][n];
  if (!Object.keys(cfg.bitNames[k]).length) delete cfg.bitNames[k];
}

/// Word value of a row: 16-bit formats carry it in regs[0]. Multi-register rows
/// have no single word — null means "no bit view" (the panel stays hidden).
function wordValueOf(row) {
  if (!row || !Array.isArray(row.regs) || !row.regs.length) return null;
  const v = Number(row.regs[0]);
  return Number.isFinite(v) ? (v & 0xffff) : null;
}

function bitOf(word, n) {
  if (word == null || n < 0 || n >= WORD_BITS) return false;
  return ((word >>> n) & 1) === 1;
}

/// Compact title line "Авария=1, Работа=0" — named bits only, high to low.
function bitsTitle(word, labels) {
  if (!labels) return "";
  const parts = [];
  const ns = Object.keys(labels).map(Number).sort((a, b) => b - a);
  for (const n of ns) parts.push(labels[n] + "=" + (bitOf(word, n) ? "1" : "0"));
  return parts.join(", ");
}

/// Human condition text for a bit rule; state words come from i18n in main.js.
function bitCondText(n, set, labels, setWord, unsetWord) {
  const name = labels && labels[n] ? " — " + labels[n] : "";
  return "бит " + n + name + (set ? " " + setWord : " " + unsetWord);
}

if (typeof module !== "undefined" && module.exports) {
  module.exports = {
    WORD_BITS, bitsKey, bitLabelsOf, bitLabel, setBitLabel,
    wordValueOf, bitOf, bitsTitle, bitCondText,
  };
}
