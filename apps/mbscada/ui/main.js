// mbSCADA frontend (Modbus Poll style). Config-driven; talks to Rust via IPC.

const hasTauri = !!(window.__TAURI__ && window.__TAURI__.core);
const invoke = hasTauri ? window.__TAURI__.core.invoke : null;

let cfg = { connections: [], polls: [], rules: [], computed: [],
  logfile: { enabled: false, path: "" },
  archive: { enabled: false, path: "", interval_s: 5, all_values: false } };
let snap = null;
let editingPoll = null;
let editingConn = null;
let editingRule = null;
let editingCalc = null;
let guideKey = "";
const $ = (id) => document.getElementById(id);

const FUNC = { 1: "01 Coils", 2: "02 Discr", 3: "03 Holding", 4: "04 Input" };
const FMTL = { u16: "u16", i16: "i16", hex: "hex16", bin: "bin16", ascii16: "ascii16",
  u16_swapped: "u16 swp", u32: "u32", i32: "i32", hex32: "hex32",
  f32_abcd: "f32 ABCD", f32_cdab: "f32 CDAB", f32_badc: "f32 BADC", f32_dcba: "f32 DCBA",
  u64: "u64", i64: "i64", f64: "f64" };
const KINDL = { tcp: "TCP", rtu: "Serial", rtu_over_tcp: "RTU/TCP", ascii: "ASCII", ascii_over_tcp: "ASCII/TCP" };

/* --------------------------------------------------- register formats ---- */
/* JS mirror of crates/mb-engine/src/format.rs: decode/encode register blocks
   per poll fmt. Powers typed writes (float / 32-bit / swapped words), the
   per-cell dec/hex/bin display and the slave-panel grid. */
const FMT_OPTS = Object.entries(FMTL).map(([v, lab]) => `<option value="${v}">${lab}</option>`).join("");

function fmtRegs(fmt) {
  if (fmt === "u64" || fmt === "i64" || fmt === "f64") return 4;
  if (fmt === "u32" || fmt === "i32" || fmt === "hex32") return 2;
  if (fmt && String(fmt).startsWith("f32")) return 2;
  return 1;
}
function swap16(x) { return ((x & 0xff) << 8) | ((x >> 8) & 0xff); }

function f32FromWords(hi, lo) {
  const dv = new DataView(new ArrayBuffer(4));
  dv.setUint16(0, hi); dv.setUint16(2, lo);
  return dv.getFloat32(0);
}

function regDecode(regs, fmt) {
  const f = fmt || "u16";
  const r = (i) => Number(regs[i]) || 0;
  const w32 = (a, b) => (((a << 16) | b) >>> 0);
  switch (f) {
    case "i16": return [(r(0) << 16) >> 16];
    case "u16_swapped": return [swap16(r(0))];
    case "u32": case "hex32": return [w32(r(0), r(1))];
    case "i32": return [(w32(r(0), r(1)) | 0)];
    case "f32_abcd": return [f32FromWords(r(0), r(1))];
    case "f32_cdab": return [f32FromWords(r(1), r(0))];
    case "f32_badc": return [f32FromWords(swap16(r(0)), swap16(r(1)))];
    case "f32_dcba": return [f32FromWords(swap16(r(1)), swap16(r(0)))];
    case "u64": case "i64": case "f64": {
      const dv = new DataView(new ArrayBuffer(8));
      dv.setUint16(0, r(0)); dv.setUint16(2, r(1)); dv.setUint16(4, r(2)); dv.setUint16(6, r(3));
      if (f === "f64") return [dv.getFloat64(0)];
      const bits = dv.getBigUint64(0);
      return [Number(f === "i64" ? BigInt.asIntN(64, bits) : bits)];
    }
    default: return [r(0)]; // u16 / hex / bin / ascii16
  }
}

function regEncode(value, fmt) {
  const f = fmt || "u16";
  const w32 = (bits) => [((Number(bits) >>> 16) & 0xffff), (Number(bits) & 0xffff)];
  switch (f) {
    case "i16": return [(Math.round(Number(value)) << 16) >> 16 & 0xffff];
    case "u16_swapped": return [swap16(Math.round(Number(value)) & 0xffff)];
    case "u32": return w32(Math.trunc(Number(value)) >>> 0);
    case "i32": return w32(Math.trunc(Number(value)) | 0);
    case "hex32": return w32(Number(value) >>> 0); // Number("0xDEADBEEF") parses
    case "f32_abcd": case "f32_cdab": case "f32_badc": case "f32_dcba": {
      const dv = new DataView(new ArrayBuffer(4));
      dv.setFloat32(0, Number(value));
      const b = [dv.getUint8(0), dv.getUint8(1), dv.getUint8(2), dv.getUint8(3)];
      const w = (hi, lo) => (((hi << 8) | lo) & 0xffff);
      if (f === "f32_abcd") return [w(b[0], b[1]), w(b[2], b[3])];
      if (f === "f32_cdab") return [w(b[2], b[3]), w(b[0], b[1])];
      if (f === "f32_badc") return [w(b[1], b[0]), w(b[3], b[2])];
      return [w(b[3], b[2]), w(b[1], b[0])];
    }
    case "u64": case "i64": case "f64": {
      const dv = new DataView(new ArrayBuffer(8));
      if (f === "f64") dv.setFloat64(0, Number(value));
      else {
        let x = BigInt(Math.round(Number(value)));
        if (f === "i64") x = BigInt.asUintN(64, x);
        dv.setBigUint64(0, x & 0xFFFFFFFFFFFFFFFFn);
      }
      return [dv.getUint16(0), dv.getUint16(2), dv.getUint16(4), dv.getUint16(6)];
    }
    case "ascii16": {
      const s = String(value ?? "").padEnd(2).slice(0, 2);
      return [(((s.charCodeAt(0) & 0xff) << 8) | (s.charCodeAt(1) & 0xff))];
    }
    case "hex": case "hex16": return [Number(value) & 0xffff];   // Number("0x1F") parses
    case "bin": case "bin16": return [Number(value) & 0xffff];   // Number("0b101") parses
    default: return [Math.round(Number(value)) & 0xffff];
  }
}

function regBitsText(regs, kind) {
  const rs = (regs && regs.length ? regs : [0]).map((x) => Number(x) || 0);
  if (kind === "hex") return "0x" + rs.map((x) => x.toString(16).padStart(4, "0")).join("").toUpperCase();
  return rs.map((x) => x.toString(2).padStart(16, "0")).join(" ");
}

/* ------------------------------------------------------------ sparklines -- */
/* Per-row value history + inline SVG polyline. Lives only in this session.  */
const SPARK_N = 40;
const SPARK_HIST = {};          // "pollId:addr" -> {vals:[], t0, t1, min, max}
const SPARK_W = 110, SPARK_H = 22;

function pushSpark(pollId, r, value) {
  const key = pollId + ":" + r.addr;
  let h = SPARK_HIST[key];
  if (!h) h = (SPARK_HIST[key] = { vals: [], min: Infinity, max: -Infinity });
  const num = parseFloat(value);
  if (Number.isFinite(num)) {
    h.vals.push(num);
    if (h.vals.length > SPARK_N) h.vals.shift();
    if (num < h.min) h.min = num;
    if (num > h.max) h.max = num;
  }
  if (r.blink) h.blink = Date.now();
  return key;
}

function sparkLineSvg(h) {
  const v = h && h.vals.length ? h.vals : [];
  const span = h ? Math.max(h.max - h.min, 1e-9, Math.abs(h.max || 1) * 1e-6) : 1;
  const pts = v.map((y, i) => `${(i / Math.max(v.length - 1, 1) * SPARK_W).toFixed(1)},${(SPARK_H - 2 - ((y - h.min) / span) * (SPARK_H - 4)).toFixed(1)}`);
  const now = Date.now();
  const fresh = h && h.blink && now - h.blink < 900;
  return `<svg class="spark${fresh ? " blink-spark" : ""}" width="${SPARK_W}" height="${SPARK_H}" viewBox="0 0 ${SPARK_W} ${SPARK_H}">` +
    (pts.length > 1 ? `<polyline points="${pts.join(" ")}" fill="none" stroke="currentColor" stroke-width="1.2"/>` : "") +
    `</svg>`;
}

function sparkline(pollId, r, display) {
  const key = pushSpark(pollId, r, display);
  return sparkLineSvg(SPARK_HIST[key]);
}

/* ------------------------------------------------------------ CSV logging -- */
/* Data archive (historian). Driven by cfg.archive: enabled, path, interval_s,
   all_values. Modes: change-only (default) writes a row when display/quality
   changes; "all values" mode rewrites every tag (plus computed tags) every
   interval_s seconds. The file path is stored in the project and resumes on
   boot. Backend appends raw lines (`csv_log`). */
const CSV = { on: false, path: "", last: {}, lastAll: 0 };

function csvEnabled() {
  return !!(cfg.archive && cfg.archive.enabled && cfg.archive.path);
}

async function csvStart(path) {
  CSV.on = true;
  CSV.path = path;
  CSV.last = {};
  CSV.lastAll = 0;
  cfg.archive.enabled = true;
  cfg.archive.path = path;
  await invoke("csv_log", { path: CSV.path, lines: ["time,poll_id,poll,func,addr,name,display,quality"] });
  csvBtnState();
  await apply();
  footer(t("csv_running") + path);
}

async function csvStop() {
  CSV.on = false;
  cfg.archive.enabled = false;
  csvBtnState();
  await apply();
  footer(t("csv_stopped"));
}

async function csvToggle() {
  if (!hasTauri) { footer(t("csv_only_tauri"), true); return; }
  if (csvEnabled()) { await csvStop(); return; }
  try {
    const path = await invoke("plugin:dialog|save", {
      options: {
        title: t("csv_dialog"),
        defaultPath: (cfg.archive && cfg.archive.path) || "mbscada-log.csv",
        filters: [{ name: "CSV", extensions: ["csv"] }],
      },
    });
    if (!path) { footer(t("csv_cancelled")); return; }
    await csvStart(path);
  } catch (e) { footer(String(e), true); }
}

function csvBtnState() {
  const b = $("csvBtn");
  if (!b) return;
  const on = csvEnabled();
  b.textContent = on ? "■ CSV" : "CSV";
  b.classList.toggle("accent", on);
}

function csvRowsFor(polls, computed) {
  const lines = [];
  const t = new Date().toISOString();
  for (const p of polls) {
    for (const r of p.rows || []) {
      const key = p.id + ":" + r.disp_addr;
      const sig = r.display + "\u0001" + r.quality;
      if (!cfg.archive.all_values && CSV.last[key] === sig) continue;
      CSV.last[key] = sig;
      lines.push(`${t},${p.id},${csvField(p.name)},${p.func},${r.disp_addr},${csvField(r.name)},${csvField(r.display)},${r.quality}`);
    }
  }
  for (const c of computed || []) {
    const key = "calc:" + c.name;
    const sig = c.display + "\u0001" + (c.ok ? "ok" : "bad");
    if (!cfg.archive.all_values && CSV.last[key] === sig) continue;
    CSV.last[key] = sig;
    lines.push(`${t},,${csvField(c.name)},,calc,${csvField(c.name)},${csvField(c.display)},${c.ok ? "good" : "bad"}`);
  }
  return lines;
}

async function csvMaybeAppend(polls, computed) {
  if (!hasTauri || !csvEnabled() || !snap) return;
  // In "all values" mode throttle to cfg.archive.interval_s seconds;
  // change-only mode reacts to every refresh tick.
  if (cfg.archive.all_values) {
    const iv = Math.max(1, Number(cfg.archive.interval_s) || 5) * 1000;
    if (Date.now() - CSV.lastAll < iv) return;
    CSV.lastAll = Date.now();
  }
  const lines = csvRowsFor(polls, computed);
  if (!lines.length) return;
  try { await invoke("csv_log", { path: CSV.path, lines }); } catch { /* keep ticking */ }
}

function csvField(v) {
  const s = String(v == null ? "" : v);
  return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s;
}

/* --------------------------------------------------------- computed tags -- */
/* Virtual tags evaluated by the engine from other tags' values:
   {Label} references, p<poll>:<addr> aliases, + - * / % ^ and min/max/avg/
   abs/sqrt/round/floor/ceil/pow. Shown as a separate card above the tables. */
function calcFromForm() {
  return {
    name: $("kName").value.trim(),
    expr: $("kExpr").value.trim(),
    unit: $("kUnit").value.trim(),
    decimals: Math.max(0, Math.min(6, Number($("kDec").value) || 0)),
  };
}

function editCalc(c) {
  editingCalc = c;
  $("kName").value = c.name;
  $("kExpr").value = c.expr;
  $("kUnit").value = c.unit || "";
  $("kDec").value = c.decimals != null ? c.decimals : 2;
  $("detCalc").open = true;
  $("btnCalcSubmit").textContent = t("edit_calc");
  $("btnCalcCancel").hidden = false;
}

function endCalcEdit() {
  editingCalc = null;
  $("btnCalcSubmit").textContent = t("b_add_calc");
  $("btnCalcCancel").hidden = true;
}

function renderCalcList() {
  const box = $("calcList");
  if (!sigChanged("calcs", [window.LANG, cfg.computed, editingCalc ? cfg.computed.indexOf(editingCalc) : -1])) return;
  if (!box) return;
  box.innerHTML = "";
  if (!(cfg.computed || []).length) { box.innerHTML = `<div class="empty">${esc(t("calc_none"))}</div>`; return; }
  cfg.computed.forEach((c, i) => {
    const el = document.createElement("div");
    el.className = "item" + (editingCalc === c ? " editing" : "");
    el.innerHTML = `<span class="grow"><b>${esc(c.name)}</b> <span class="sub">${esc(c.expr)}${c.unit ? " · " + esc(c.unit) : ""}</span></span>`;
    el.appendChild(miniBtn("✎", () => editCalc(c)));
    el.appendChild(miniBtn("✕", () => {
      if (editingCalc === c) endCalcEdit();
      cfg.computed.splice(i, 1);
      apply();
    }));
    box.appendChild(el);
  });
}

/* --------------------------------------------------------------- alarms -- */
/* Alarm panel: rows with alarm_level != "ok" surface here, not only as row
   colours in the table. State machine (active ↔ history, acknowledgement)
   lives in this session; the engine keeps evaluating levels as before. */
const AL = { active: {}, hist: [], sound: localStorage.getItem("mbscada.alSound") !== "0", tab: "active" };
const JUMP = { key: null, until: 0, scrolled: true };

function alKey(pollId, addr) { return pollId + ":" + addr; }

function updateAlarms() {
  if (!snap) return;
  const seen = new Set();
  for (const p of snap.polls) {
    for (const r of p.rows || []) {
      if (!r.alarm_level || r.alarm_level === "ok") continue;
      const key = alKey(p.id, r.addr);
      seen.add(key);
      let a = AL.active[key];
      if (!a) {
        a = (AL.active[key] = {
          key, pollId: p.id, poll: p.name, addr: r.addr, disp: r.disp_addr,
          name: r.name, level: r.alarm_level, msg: r.alarm_msg || r.alarm_level,
          display: r.display, unit: r.unit, quality: r.quality,
          since: Date.now(), ack: false,
        });
        if (AL.sound) beep();
      } else {
        a.poll = p.name; a.name = r.name; a.display = r.display;
        a.msg = r.alarm_msg || r.alarm_level; a.level = r.alarm_level;
        a.quality = r.quality;
      }
    }
  }
  // Авария пропала — уходит в историю с длительностью.
  for (const key of Object.keys(AL.active)) {
    if (seen.has(key)) continue;
    const a = AL.active[key];
    delete AL.active[key];
    a.cleared = Date.now();
    a.dur = a.cleared - a.since;
    AL.hist.unshift(a);
    if (AL.hist.length > 100) AL.hist.pop();
  }
}

function alarmsSig() {
  return JSON.stringify([window.LANG,
    Object.values(AL.active).map((a) => [a.key, a.ack, a.level, a.display, a.msg, a.quality, a.since, a.poll, a.disp]),
    AL.hist.length,
    AL.hist.map((a) => [a.key, a.since, a.cleared])]);
}

function renderAlarms() {
  if (!sigChanged("alarms", alarmsSig())) return;
  const badge = $("alarmBadge");
  if (badge) {
    const unacked = Object.values(AL.active).filter((a) => !a.ack).length;
    badge.hidden = unacked === 0;
    badge.textContent = unacked;
  }
  const act = $("alarmActive"), hist = $("alarmHist");
  if (!act || !hist) return;
  act.innerHTML = "";
  const items = Object.values(AL.active).sort((a, b) => (a.ack - b.ack) || (b.since - a.since));
  if (!items.length) act.innerHTML = `<div class="empty">${esc(t("al_none"))}</div>`;
  for (const a of items) {
    const el = document.createElement("div");
    el.className = "al-item lv-" + a.level + (a.ack ? " ack" : "");
    el.innerHTML = `<div class="al-top"><span class="al-dot"></span>
        <span class="al-time">${hms(a.since)}</span>
        <b class="grow">${esc(a.name)}</b>
        <span class="mono">${esc(a.display)}${a.unit ? " " + esc(a.unit) : ""}</span></div>
      <div class="al-bot"><span class="grow">${esc(a.msg)}${a.quality === "bad" ? ' · <span class="bad">' + esc(t("al_bad_conn")) + "</span>" : ""}</span>
        <span class="sub">${esc(a.poll)} @${esc(a.disp)}</span></div>`;
    el.title = t("al_jump_hint");
    el.onclick = () => alarmJump(a.key);
    el.appendChild(miniBtn(a.ack ? t("al_acked") : t("al_ack"), (ev) => { ev.stopPropagation(); a.ack = true; renderAlarms(); }));
    act.appendChild(el);
  }
  hist.innerHTML = "";
  if (!AL.hist.length) hist.innerHTML = `<div class="empty">${esc(t("al_hist_empty"))}</div>`;
  for (const a of AL.hist) {
    const el = document.createElement("div");
    el.className = "al-item lv-" + a.level + " ack hist";
    el.innerHTML = `<div class="al-top"><span class="al-dot"></span>
        <b class="grow">${esc(a.name)}</b><span class="sub">${esc(a.msg)}</span></div>
      <div class="al-bot"><span class="sub">${hms(a.since)} → ${hms(a.cleared)} · ${fmtDur(a.dur)}</span>
        <span class="sub">${esc(a.poll)} @${esc(a.disp)}</span></div>`;
    hist.appendChild(el);
  }
}

function alarmJump(key) {
  JUMP.key = key; JUMP.until = Date.now() + 2500; JUMP.scrolled = false;
  const tab = document.querySelector('.tab[data-view="tables"]');
  if (tab) tab.click();
  render(); // перерисовать немедленно — затем подсветка и скролл в renderGrid
}

function beep() {
  try {
    const ctx = beep.ctx || (beep.ctx = new (window.AudioContext || window.webkitAudioContext)());
    const o = ctx.createOscillator(), g = ctx.createGain();
    o.type = "square"; o.frequency.value = 880;
    g.gain.setValueAtTime(0.07, ctx.currentTime);
    g.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.35);
    o.connect(g).connect(ctx.destination);
    o.start(); o.stop(ctx.currentTime + 0.35);
  } catch { /* аудио недоступно */ }
}

function hms(t) {
  const d = new Date(t);
  return [d.getHours(), d.getMinutes(), d.getSeconds()].map((x) => String(x).padStart(2, "0")).join(":");
}
function fmtDur(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return s + t("dur_s");
  const m = Math.floor(s / 60);
  if (m < 60) return m + t("dur_m") + (s % 60) + t("dur_s");
  return Math.floor(m / 60) + t("dur_h") + (m % 60) + t("dur_m");
}

/* --------------------------------------------------------- watch window -- */
/* Floating "viewable parameters" window: pin any register (or a computed tag)
   and get a big live readout with alarm colours, session min/max and
   jump-to-row. Items are references (poll id + addr, or computed tag name)
   resolved from the snapshot at render time, so table renames propagate
   automatically. The item list persists in the project (cfg.watch); window
   visibility and position are per-workstation (localStorage). */
const WATCH = { stats: {}, pos: null, hover: false };
let wdrag = null;

function watchCfg() {
  if (!cfg.watch || typeof cfg.watch !== "object") cfg.watch = {};
  if (!Array.isArray(cfg.watch.items)) cfg.watch.items = [];
  return cfg.watch;
}

function watchKey(it) { return it.kind === "calc" ? "calc:" + it.name : it.poll + ":" + it.addr; }

function watchPlace() {
  const win = $("watchwin");
  if (win && WATCH.pos) { win.style.left = WATCH.pos.x + "px"; win.style.top = WATCH.pos.y + "px"; }
}

function watchSavePos() { localStorage.setItem("mbscada.watchPos", JSON.stringify(WATCH.pos)); }

// Restores window position and (re)opens it if it was open last session.
function watchLoadPos() {
  try { WATCH.pos = JSON.parse(localStorage.getItem("mbscada.watchPos") || "null"); } catch { WATCH.pos = null; }
  try { if (localStorage.getItem("mbscada.watchOpen") === "1") watchToggle(true); } catch { /* no storage */ }
}

function watchToggle(force) {
  const win = $("watchwin");
  if (!win) return;
  const open = force != null ? !!force : !!win.hidden;
  win.hidden = !open;
  localStorage.setItem("mbscada.watchOpen", open ? "1" : "0");
  const b = $("btnWatch");
  if (b) b.classList.toggle("accent", open);
  if (open) {
    if (!WATCH.pos) {
      WATCH.pos = { x: Math.max(10, window.innerWidth - 352), y: 62 };
    }
    watchPlace();
    watchFillPicker();
    renderWatch();
  }
}

function watchFillPicker() {
  const sel = $("watchPick");
  if (!sel) return;
  const cur = sel.value;
  sel.innerHTML = "";
  for (const p of cfg.polls) {
    const step = stepOf(p.fmt);
    const n = Math.max(1, Math.floor((p.qty || 1) / step));
    const g = document.createElement("optgroup");
    g.label = p.name;
    for (let i = 0; i < n; i++) {
      const addr = (p.start || 0) + i * step;
      const o = document.createElement("option");
      o.value = "reg:" + p.id + ":" + addr;
      o.textContent = addr + " — " + regLabel(p, addr);
      g.appendChild(o);
    }
    sel.appendChild(g);
  }
  if ((cfg.computed || []).length) {
    const g = document.createElement("optgroup");
    g.label = t("watch_group_calc");
    for (const c of cfg.computed) {
      const o = document.createElement("option");
      o.value = "calc:" + c.name;
      o.textContent = "∑ " + c.name;
      g.appendChild(o);
    }
    sel.appendChild(g);
  }
  if (cur && [...sel.options].some((o) => o.value === cur)) sel.value = cur;
}

function watchAdd() {
  const v = $("watchPick").value;
  if (!v) { footer(t("pick_reg_first"), true); return; }
  const items = watchCfg().items;
  if (items.length >= 32) { footer(t("watch_limit"), true); return; }
  let it;
  if (v.startsWith("calc:")) it = { kind: "calc", name: v.slice(5) };
  else {
    const [pid, addr] = v.slice(4).split(":").map(Number);
    it = { kind: "reg", poll: pid, addr };
  }
  if (items.some((x) => watchKey(x) === watchKey(it))) { footer(t("watch_already"), true); return; }
  items.push(it);
  apply();
}

function watchRemove(it) {
  const items = watchCfg().items;
  const i = items.findIndex((x) => watchKey(x) === watchKey(it));
  if (i >= 0) items.splice(i, 1);
  delete WATCH.stats[watchKey(it)];
  apply();
}

// Live state of one watched item, resolved from the snapshot every tick.
function watchResolve(it) {
  if (it.kind === "calc") {
    const c = ((snap && snap.computed) || []).find((x) => x.name === it.name);
    return {
      name: it.caption || it.name, src: t("watch_src_calc"),
      display: c ? c.display : "—", unit: c && c.unit ? c.unit : "",
      level: "", fg: null, bg: null, blink: false,
      quality: c ? (c.ok ? "good" : "bad") : "off", stale: !c,
      num: c ? parseFloat(c.display) : NaN, pollId: null, addr: null,
    };
  }
  const p = findCfgPoll(it.poll);
  const sp = snap ? snap.polls.find((x) => x.id === it.poll) : null;
  const row = sp && sp.rows ? sp.rows.find((r) => r.addr === it.addr) : null;
  return {
    name: it.caption || (p ? regLabel(p, it.addr) : `p${it.poll}@${it.addr}`),
    src: p ? p.name + " @" + dispAddr(p, it.addr) : t("watch_src_lost"),
    display: row ? row.display : "—",
    unit: row && row.unit ? row.unit : p && p.unit ? p.unit : "",
    level: row && row.alarm_level && row.alarm_level !== "ok" ? row.alarm_level : "",
    fg: row ? row.fg : null, bg: row ? row.bg : null, blink: row ? !!row.blink : false,
    quality: row ? row.quality : "off", stale: !p || !row,
    num: row ? parseFloat(row.display) : NaN, pollId: it.poll, addr: it.addr,
  };
}

function renderWatch() {
  const win = $("watchwin"), list = $("watchList");
  if (!win || !list || win.hidden) return;
  // Не перерисовываем под курсором — иначе dblclick по имени рвётся.
  if (WATCH.hover) return;
  const items = watchCfg().items;
  list.innerHTML = "";
  if (!items.length) { list.innerHTML = `<div class="empty">${esc(t("watch_none"))}</div>`; return; }
  for (const it of items) {
    const key = watchKey(it);
    const v = watchResolve(it);
    let st = WATCH.stats[key];
    if (!st) st = (WATCH.stats[key] = { min: Infinity, max: -Infinity });
    if (Number.isFinite(v.num) && v.quality !== "off") {
      if (v.num < st.min) st.min = v.num;
      if (v.num > st.max) st.max = v.num;
    }
    const el = document.createElement("div");
    el.className = "wparam" + (v.level ? " lv-" + v.level : "") +
      (v.quality === "bad" ? " q-bad" : "") + (v.stale ? " stale" : "") + (v.blink ? " blink" : "");
    if (v.fg) el.style.color = v.fg;
    if (v.bg) el.style.background = v.bg;
    const unit = v.unit ? `<span class="wp-unit">${esc(v.unit)}</span>` : "";
    const mn = Number.isFinite(st.min) ? String(st.min) : "—";
    const mx = Number.isFinite(st.max) ? String(st.max) : "—";
    const q = v.quality === "bad" ? `<span class="bad">${esc(t("al_bad_conn"))}</span>` : "";
    el.innerHTML = `<div class="wp-top"><span class="wp-name">${esc(v.name)}</span><span class="wp-src">${esc(v.src)}</span></div>
      <div class="wp-val" title="${esc(t("al_jump_hint"))}">${esc(v.display)}${unit}</div>
      <div class="wp-bot"><span>${t("watch_min")} ${esc(mn)}</span><span>${t("watch_max")} ${esc(mx)}</span>${q}</div>`;
    if (v.pollId != null) {
      el.querySelector(".wp-val").onclick = (ev) => { ev.stopPropagation(); watchJump(v.pollId, v.addr); };
    } else {
      el.querySelector(".wp-val").style.cursor = "default";
    }
    el.title = t("watch_cap_hint");
    el.ondblclick = () => {
      openValueModal(t("watch_cap_title") + "\n" + v.name, it.caption || "", (val) => {
        it.caption = String(val).trim();
        if (!it.caption) delete it.caption;
        apply();
      }, t("m_save"));
    };
    el.appendChild(miniBtn("✕", (ev) => { ev.stopPropagation(); watchRemove(it); }));
    list.appendChild(el);
  }
}

function watchJump(pollId, addr) { alarmJump(pollId + ":" + addr); }

// Подстановка {ключей} в словарные строки: tfmt("write_title", {addr: 5, ...})
function tfmt(key, vars) {
  let s = t(key);
  for (const k in vars) s = s.split("{" + k + "}").join(String(vars[k]));
  return s;
}

/* ------------------------------------------------------------ bootstrap -- */
async function boot() {
  if (!Array.isArray(cfg.computed)) cfg.computed = [];
  if (!cfg.logfile) cfg.logfile = { enabled: false, path: "" };
  if (!cfg.archive) cfg.archive = { enabled: false, path: "", interval_s: 5, all_values: false };
  if (!Array.isArray(cfg.mimic)) cfg.mimic = [];
  watchCfg();
  slaveCfg();
  window.LANG = window.i18nLang();
  const ls = $("langSel");
  if (ls) {
    ls.value = window.LANG;
    ls.onchange = () => {
      window.LANG = ls.value;
      localStorage.setItem("mbscada.lang", ls.value);
      applyLangStatic();
      render();
    };
  }
  applyLangStatic();
  await refreshPorts();
  wireForms();
  await loadUiState();
  watchLoadPos();
  setInterval(refresh, 300);
  refresh();
  if (!hasTauri) {
    footer(t("no_tauri"), true);
  }
}

// Restores persisted UI-driven state: CSV archive resume + logging fields +
// suggested file locations (app data dir).
async function loadUiState() {
  if (!hasTauri) return;
  try {
    const sp = await invoke("suggest_paths");
    const lp = $("logPath");
    if (lp && !cfg.logfile.path) lp.placeholder = sp.log;
    if (cfg.logfile.enabled && cfg.logfile.path) {
      $("logEnabled").checked = true;
      $("logPath").value = cfg.logfile.path;
    }
    $("archAll").checked = !!cfg.archive.all_values;
    $("archPeriod").value = cfg.archive.interval_s || 5;
    if (csvEnabled()) {
      // resume the historian from the saved project file
      try {
        await invoke("csv_log", { path: cfg.archive.path, lines: [] });
        CSV.on = true; CSV.path = cfg.archive.path;
        csvBtnState();
        footer(t("csv_resumed") + CSV.path);
      } catch { /* file not available yet */ }
    }
  } catch { /* suggest_paths may be missing in old builds */ }
}

let ports = [];
async function refreshPorts() {
  if (!hasTauri) {
    ports = [{ name: "COM1", detail: "" }, { name: "COM3", detail: "" }];
  } else {
    try { ports = await invoke("list_serial_ports"); } catch { ports = []; }
  }
  const sel = $("cCom");
  if (!sel) return;
  const cur = sel.value;
  sel.innerHTML = ports.map((p) => `<option value="${p.name}">${p.name}${p.detail ? " — " + esc(p.detail) : ""}</option>`).join("") || `<option>COM1</option>`;
  if (cur) sel.value = cur;
  fillSlaveSerialPorts();
}

// Fill the slave panel's COM-port select from the shared port list.
// `rescan` forces a fresh probe of the OS ports (⟳ button).
async function fillSlaveSerialPorts(rescan) {
  const sel = $("slSerPort");
  if (!sel) return;
  if (rescan || !ports.length) {
    if (hasTauri) {
      try { ports = await invoke("list_serial_ports"); } catch { ports = []; }
    } else {
      ports = [{ name: "COM1", detail: "" }, { name: "COM3", detail: "" }];
    }
  }
  const cur = slaveCfg().serial.port || sel.value;
  sel.innerHTML = ports.map((p) => `<option value="${p.name}">${p.name}${p.detail ? " — " + esc(p.detail) : ""}</option>`).join("")
    || `<option value="">${esc(t("sl_ser_none"))}</option>`;
  if (cur) sel.value = cur;
}

function connFromForm() {
  const kind = $("cKind").value;
  return {
    id: "c" + Date.now(),
    name: $("cName").value || "Conn",
    kind,
    host: $("cHost").value,
    port: Number($("cPort").value),
    serial: {
      port: $("cCom").value,
      baud: Number($("cBaud").value),
      data_bits: Number($("cData").value),
      parity: $("cParity").value,
      stop_bits: Number($("cStop").value),
    },
    timeout_ms: Number($("cTimeout").value),
    slave: Math.max(0, Math.min(247, Number($("cSlave").value) || 0)),
  };
}

function editConn(c) {
  editingConn = c;
  $("cName").value = c.name;
  $("cKind").value = c.kind || "tcp";
  kindFields();
  $("cHost").value = c.host || "127.0.0.1";
  $("cPort").value = c.port || 502;
  const s = c.serial || {};
  ensureOption("cCom", s.port);
  $("cCom").value = s.port || "";
  ensureOption("cBaud", s.baud != null ? String(s.baud) : "9600");
  $("cBaud").value = s.baud != null ? String(s.baud) : "9600";
  $("cParity").value = s.parity || "N";
  $("cData").value = s.data_bits != null ? String(s.data_bits) : "8";
  $("cStop").value = s.stop_bits != null ? String(s.stop_bits) : "1";
  $("cSlave").value = c.slave != null ? c.slave : 1;
  $("cTimeout").value = c.timeout_ms || 1000;
  $("detConn").open = true;
  $("btnConnSubmit").textContent = t("edit_conn");
  $("btnConnCancel").hidden = false;
}

function endConnEdit() {
  editingConn = null;
  $("btnConnSubmit").textContent = t("b_add_conn");
  $("btnConnCancel").hidden = true;
}

function resetConnForm() {
  $("cName").value = "Conn" + (cfg.connections.length + 1);
  $("cKind").value = "tcp";
  kindFields();
  $("cSlave").value = 1;
  $("cTimeout").value = 1000;
}

// Гарантирует наличие значения в select (напр. кастомный baud у импорта).
function ensureOption(selId, val) {
  if (val == null || val === "") return;
  const sel = $(selId);
  if (!sel) return;
  if (![...sel.options].some((o) => o.value === String(val))) {
    const o = document.createElement("option");
    o.value = String(val);
    o.textContent = String(val);
    sel.appendChild(o);
  }
}

// Показ/скрытие полей TCP/serial в зависимости от типа соединения.
function kindFields() {
  const k = $("cKind").value;
  $("tcpFields").hidden = !(k === "tcp" || k === "rtu_over_tcp" || k === "ascii_over_tcp");
  $("serFields").hidden = !(k === "rtu" || k === "ascii");
}

async function testConn() {
  if (!hasTauri) { footer("проверка доступна только в приложении", true); return; }
  try {
    const conn = connFromForm();
    const msg = await invoke("test_connection", { conn });
    footer("✓ " + msg);
  } catch (e) { footer("✗ " + e, true); }
}

async function refresh() {
  try {
    if (!hasTauri) { render(); return; } // browser preview: empty, no fake data
    snap = await invoke("snapshot");
    if (csvEnabled() && snap) {
      const pollsForCsv = snap.polls.length ? snap.polls : cfg.polls;
      await csvMaybeAppend(pollsForCsv, snap.computed || []);
    }
    render();
    slaveTick();
  } catch (e) { footer(t("err_prefix") + e, true); }
}

/* ------------------------------------------------------------- rendering -- */
function render() {
  renderChips();
  renderConnList();
  renderPollList();
  renderRuleList();
  renderCalcList();
  renderGrid();
  renderStats();
  renderLog();
  updateAlarms();
  renderAlarms();
  renderWatch();
  fillSelectors();
  renderSchema();
  // Старт/Стоп отражают фактическое состояние движка.
  const running = !!(snap && snap.running);
  $("btnStart").disabled = running;
  $("btnStop").disabled = !running;
  // Guide the user to the next step — once per config change, не мешая
  // редактировать формы (раньше это срабатывало каждые 300 мс и закрывало
  // форму соединения у пользователя из-под рук).
  const dc = $("detConn"), dp = $("detPoll");
  if (dc && dp) {
    const key = cfg.connections.length + ":" + cfg.polls.length;
    if (key !== guideKey) {
      guideKey = key;
      if (!cfg.connections.length) { dc.open = true; }
      else if (!cfg.polls.length) { dp.open = true; if (!editingConn) dc.open = false; }
    }
  }
}

function renderChips() {
  const box = $("connChips");
  const views = snap && snap.connections.length
    ? snap.connections.map((c) => ({ name: c.name, describe: c.describe, online: c.stats.online, err: c.stats.last_error }))
    : cfg.connections.map((c) => ({ name: c.name, describe: describe(c), online: false, err: "" }));
  if (!sigChanged("chips", [window.LANG, views])) return;
  box.innerHTML = "";
  for (const v of views) {
    const el = document.createElement("div");
    el.className = "chip";
    const dot = v.online ? (v.err ? "err" : "on") : "";
    el.innerHTML = `<span class="dot ${dot}"></span>${esc(v.name)} <span class="sub">${esc(v.describe)}</span>`;
    box.appendChild(el);
  }
  if (!views.length) box.innerHTML = `<span class="empty">${esc(t("conns_none"))}</span>`;
}

function renderConnList() {
  const box = $("connList");
  if (!sigChanged("conns", [window.LANG, cfg.connections.map((c) => [c.id, c.name, describe(c)]), editingConn ? editingConn.id : null])) return;
  box.innerHTML = "";
  if (!cfg.connections.length) { box.innerHTML = `<div class="empty">${esc(t("add_conn_first"))}</div>`; return; }
  cfg.connections.forEach((c, i) => {
    const el = document.createElement("div");
    el.className = "item" + (editingConn === c ? " editing" : "");
    el.innerHTML = `<span class="grow"><b>${esc(c.name)}</b> <span class="sub">${esc(describe(c))}</span></span>`;
    el.appendChild(miniBtn("✎", () => editConn(c)));
    el.appendChild(miniBtn("✕", () => {
      if (editingConn === c) { endConnEdit(); resetConnForm(); }
      cfg.connections.splice(i, 1);
      apply();
    }));
    box.appendChild(el);
  });
}

function renderPollList() {
  const box = $("pollList");
  if (!sigChanged("polls", [window.LANG,
    cfg.polls.map((p) => [p.id, p.name, p.func, p.raw_start != null ? p.raw_start : p.start, p.qty, p.slave, p.fmt, p.scan_ms, p.conn_id, p.labels ? Object.keys(p.labels).length : 0]),
    editingPoll ? editingPoll.id : null])) return;
  box.innerHTML = "";
  if (!cfg.polls.length) { box.innerHTML = `<div class="empty">${esc(t("add_poll_first"))}</div>`; return; }
  cfg.polls.forEach((p, i) => {
    const el = document.createElement("div");
    el.className = "item" + (editingPoll === p ? " editing" : "");
    const bound = cfg.connections.some((c) => c.id === p.conn_id);
    const slave = p.slave ? `slave ${p.slave}` : t("slave_from_conn");
    el.innerHTML = `<span class="grow"><b>${esc(p.name)}</b> <span class="sub">${FUNC[p.func] || p.func} @${p.raw_start != null ? p.raw_start : p.start}×${p.qty} · ${slave} · ${FMTL[p.fmt] || p.fmt} · ${p.scan_ms}${t("ms")}${bound ? "" : ' · <span class="bad">' + esc(t("no_conn")) + "</span>"}</span></span>`;
    el.appendChild(miniBtn("✎", () => editPoll(p)));
    el.appendChild(miniBtn("✕", () => {
      if (editingPoll === p) { endPollEdit(); }
      cfg.polls.splice(i, 1);
      apply();
    }));
    box.appendChild(el);
  });
}

function renderRuleList() {
  const box = $("ruleList");
  if (!sigChanged("rules", [window.LANG,
    cfg.rules.map((r) => [r.id, r.poll, r.offset, JSON.stringify(r.cond), r.fg, r.bg, r.blink, r.severity, r.message, r.text]),
    editingRule ? editingRule.id : null])) return;
  box.innerHTML = "";
  if (!cfg.rules.length) { box.innerHTML = `<div class="empty">${esc(t("no_rules"))}</div>`; return; }
  // Номер ветки внутри своего регистра: 1..32 (первая сработавшая выигрывает).
  const ord = {};
  cfg.rules.forEach((r) => {
    const k = `${r.poll}:${r.offset}`;
    ord[k] = (ord[k] || 0) + 1;
    r._ord = ord[k];
  });
  cfg.rules.forEach((r, i) => {
    const el = document.createElement("div");
    el.className = "item" + (editingRule === r ? " editing" : "");
    const sw = document.createElement("span");
    sw.style.cssText = `width:13px;height:13px;border-radius:3px;border:1px solid #334155;background:${r.bg || "transparent"}`;
    el.appendChild(sw);
    const poll = findCfgPoll(r.poll);
    const addr = poll ? (poll.start || 0) + r.offset * stepOf(poll.fmt) : r.offset;
    const name = poll ? regLabel(poll, addr) : `poll ${r.poll}`;
    el.insertAdjacentHTML("beforeend", `<span class="grow"><b>№${r._ord}/32 ${esc(name)}</b> <span class="sub">${condText(r.cond)}${r.text ? " · ⌨" + esc(r.text) : ""}${r.severity ? " · ⚠" + r.severity : ""}${r.blink ? " · " + t("blinking") : ""}</span></span>`);
    el.appendChild(miniBtn("✎", () => editRule(r)));
    el.appendChild(miniBtn("✕", () => {
      if (editingRule === r) endRuleEdit();
      cfg.rules.splice(i, 1);
      apply();
    }));
    box.appendChild(el);
  });
}

function editRule(r) {
  editingRule = r;
  const poll = findCfgPoll(r.poll);
  const addr = poll ? (poll.start || 0) + r.offset * stepOf(poll.fmt) : r.offset;
  $("rReg").value = `${r.poll}:${addr}`;
  const c = r.cond || {};
  const kind = c.kind || "gt";
  $("rKind").value = kind;
  if (kind === "in_range" || kind === "outside_range") {
    $("rValue").value = c.lo;
    $("rValue2").value = c.hi;
  } else if (kind === "bit") {
    $("rValue").value = 0;
    $("rValue2").value = c.n;
  } else {
    $("rValue").value = c.v;
    $("rValue2").value = 0;
  }
  $("rFg").value = r.fg || "#ffffff";
  $("rBg").value = r.bg || "#ef4444";
  $("rSeverity").value = r.severity || "";
  $("rMessage").value = r.message || r.name || "";
  $("rText").value = r.text || "";
  $("rBlink").checked = !!r.blink;
  $("btnRuleSubmit").textContent = t("edit_rule");
  $("btnRuleCancel").hidden = false;
  const det = $("btnRuleSubmit").closest("details");
  if (det) det.open = true;
}

function endRuleEdit() {
  editingRule = null;
  $("btnRuleSubmit").textContent = t("b_add_rule");
  $("btnRuleCancel").hidden = true;
}

/* ------------------------------------------------- tables grid (live) ---- */
/* The grid updates in place: a full DOM rebuild happens only when the table
   structure changes (polls / fmt / row set / language); value cells are
   patched every tick. Stable <tr> nodes are what fixed the flaky
   double-clicks and the 300 ms lag. */
const GRID = { sig: null, cards: new Map(), comp: null };

function gridSig() {
  const computed = (snap && snap.computed) || [];
  const polls = snap && snap.polls.length ? snap.polls : cfg.polls;
  return JSON.stringify([
    window.LANG,
    computed.map((c) => c.name + "\u0001" + c.expr),
    polls.map((p) => [p.id, p.name, p.fmt, p.start, p.qty, p.func,
      (p.rows && p.rows.length ? p.rows : synthRows(p)).map((r) => r.addr)]),
  ]);
}

function dispModeOf(pollId, addr) {
  const key = pollId + ":" + addr;
  return (cfg.rowDisp && cfg.rowDisp[key]) ||
    (cfg.dispMode && cfg.dispMode[pollId]) || "dec";
}

function cycleRowDisp(pollId, addr) {
  if (!cfg.rowDisp) cfg.rowDisp = {};
  const key = pollId + ":" + addr;
  const cur = dispModeOf(pollId, addr);
  const next = cur === "dec" ? "hex" : cur === "hex" ? "bin" : "dec";
  if (next === "dec") delete cfg.rowDisp[key];
  else cfg.rowDisp[key] = next;
  updateGridTick(true);
}

// Value text for one row in the requested display mode (dec / hex / bin).
// hex/bin show the raw bits of the value's registers (independent of fmt).
function rowDispText(cfgP, r, mode) {
  if (mode === "hex") return regBitsText(r.regs, "hex");
  if (mode === "bin") return regBitsText(r.regs, "bin");
  if (cfgP && (cfgP.func === 1 || cfgP.func === 2)) return r.display; // ON/OFF
  const s = (r.scaled != null) ? r.scaled : r.value;
  const fmt = cfgP && cfgP.fmt;
  if (fmt && (String(fmt).startsWith("f32") || fmt === "f64")) return String(Math.round(s * 1000) / 1000);
  return String(Math.round(s * 100) / 100);
}

function renderGrid() {
  const grid = $("grid");
  if (!grid) return;
  const sig = gridSig();
  if (sig !== GRID.sig) {
    GRID.sig = sig;
    GRID.cards.clear();
    buildGrid(grid);
    return;
  }
  updateGridTick(false);
}

function buildGrid(grid) {
  grid.innerHTML = "";
  const computed = (snap && snap.computed) || [];
  if (computed.length) grid.appendChild(computedCard(computed));
  const polls = snap && snap.polls.length ? snap.polls : cfg.polls.map((p) => ({ ...p, rows: [], last_poll_ms: 0 }));
  if (!polls.length) { if (!computed.length) grid.innerHTML = `<div class="empty" style="padding:30px">${esc(t("grid_empty"))}</div>`; return; }
  const cfgById = {};
  cfg.polls.forEach((q) => { cfgById[q.id] = q; });
  for (const p of polls) {
    const card = document.createElement("div");
    card.className = "card";
    const head = document.createElement("div");
    head.className = "card-head";
    const cn = cfg.connections.find((c) => c.id === p.conn_id);
    const connTxt = cn ? esc(cn.name) : `<span class="bad">${esc(t("no_conn"))}</span>`;
    head.innerHTML = `<h2>${esc(p.name)}</h2>
      <span class="meta">${FUNC[p.func] || p.func} · @${p.start}×${p.qty} · ${p.slave ? "slave " + p.slave : esc(t("slave_from_conn"))} · ${p.scan_ms}${t("ms")} · ${FMTL[p.fmt] || p.fmt} · ${connTxt}</span>
      <span class="stat"></span>`;
    // Quick data-type switch (like Modbus Poll Number Type) — one click right
    // after the import; rows = qty / registers-per-value.
    const fs = document.createElement("select");
    fs.className = "fmtsel";
    fs.title = t("fmt_hint");
    fs.innerHTML = FMT_OPTS;
    fs.value = p.fmt;
    fs.onchange = () => { const cp = findCfgPoll(p.id); if (cp) { cp.fmt = fs.value; apply(); } };
    head.appendChild(fs);
    // Default display format for the poll's values (per-cell override: ПКМ).
    const ds = document.createElement("select");
    ds.className = "fmtsel";
    ds.title = t("disp_mode_title");
    ds.innerHTML = `<option value="dec">${t("disp_dec")}</option><option value="hex">${t("disp_hex")}</option><option value="bin">${t("disp_bin")}</option>`;
    ds.value = cfg.dispMode && cfg.dispMode[p.id] ? cfg.dispMode[p.id] : "dec";
    ds.onchange = () => {
      if (!cfg.dispMode) cfg.dispMode = {};
      if (ds.value === "dec") delete cfg.dispMode[p.id]; else cfg.dispMode[p.id] = ds.value;
      updateGridTick(true);
    };
    head.appendChild(ds);
    card.appendChild(head);

    const table = document.createElement("table");
    table.className = "regtable";
    table.innerHTML = `<thead><tr><th>${t("th_addr")}</th><th>${t("th_name")}</th><th>${t("th_value")}</th><th>${t("th_trend")}</th><th>${t("th_unit")}</th><th>${t("th_quality")}</th><th>${t("th_alarm")}</th></tr></thead>`;
    const tb = document.createElement("tbody");
    const rows = (p.rows && p.rows.length) ? p.rows : synthRows(cfgById[p.id] || p);
    const rec = { stat: head.querySelector(".stat"), rows: new Map(), data: new Map(), cells: new Map() };
    for (const r of rows) {
      const rowKey = p.id + ":" + r.addr;
      const tr = document.createElement("tr");
      tr.className = "row" + (r.blink ? " blink" : "");
      tr.dataset.key = rowKey;
      const mode = dispModeOf(p.id, r.addr);
      const alarm = r.alarm_level && r.alarm_level !== "ok" ? "⚠ " + esc(r.alarm_msg || r.alarm_level) : "";
      const spark = sparkline(p.id, r, String(r.scaled != null ? r.scaled : r.raw));
      tr.innerHTML = `<td class="mono">${r.disp_addr}</td><td>${esc(r.name)}</td>
        <td class="mono"><b>${esc(rowDispText(cfgById[p.id] || p, r, mode))}</b></td><td>${spark}</td><td>${esc(r.unit)}</td>
        <td class="${r.quality}">${r.quality}</td><td>${alarm}</td>`;
      tr.title = t("row_write_hint");
      tr.onclick = () => writeRow(p, r);
      // ПКМ по значению — цикл dec → hex → bin для этой ячейки.
      tr.children[2].title = t("disp_hint");
      tr.children[2].oncontextmenu = (ev) => { ev.preventDefault(); cycleRowDisp(p.id, r.addr); };
      rec.rows.set(rowKey, tr);
      rec.cells.set(rowKey, {
        val: tr.children[2].firstChild, name: tr.children[1], spark: tr.children[3],
        q: tr.children[5], al: tr.children[6],
      });
      tb.appendChild(tr);
    }
    if (!rows.length) {
      const tr = document.createElement("tr");
      tr.innerHTML = `<td colspan="7" class="empty">${esc(t("no_data"))}</td>`;
      tb.appendChild(tr);
    }
    table.appendChild(tb);
    card.appendChild(table);
    grid.appendChild(card);
    GRID.cards.set(p.id, rec);
  }
}

// Per-tick in-place update of every row cell (no DOM replacement at all).
function updateGridTick() {
  const cfgById = {};
  cfg.polls.forEach((q) => { cfgById[q.id] = q; });
  const polls = snap && snap.polls.length ? snap.polls : cfg.polls;
  for (const p of polls) {
    const rec = GRID.cards.get(p.id);
    if (!rec) { GRID.sig = null; renderGrid(); return; }
    const cfgP = cfgById[p.id] || p;
    const rows = (p.rows && p.rows.length) ? p.rows : synthRows(cfgP);
    for (const r of rows) {
      const rowKey = p.id + ":" + r.addr;
      const tr = rec.rows.get(rowKey);
      const cell = rec.cells.get(rowKey);
      if (!tr || !cell) { GRID.sig = null; renderGrid(); return; }
      rec.data.set(rowKey, { p: cfgP, r });
      tr.classList.toggle("blink", !!r.blink);
      if (JUMP.key === rowKey && Date.now() < JUMP.until) {
        tr.classList.add("flashrow");
        if (!JUMP.scrolled) {
          JUMP.scrolled = true;
          setTimeout(() => tr.scrollIntoView({ block: "center", behavior: "smooth" }), 0);
        }
      } else {
        tr.classList.remove("flashrow");
      }
      if (r.fg) tr.style.color = r.fg; else tr.style.color = "";
      if (r.bg) tr.style.background = r.bg; else tr.style.background = "";
      cell.val.textContent = rowDispText(cfgP, r, dispModeOf(p.id, r.addr));
      cell.name.textContent = r.name;
      cell.spark.innerHTML = sparklineSvgOnly(p.id, r);
      cell.q.textContent = r.quality;
      cell.q.className = r.quality;
      cell.al.textContent = r.alarm_level && r.alarm_level !== "ok" ? "⚠ " + (r.alarm_msg || r.alarm_level) : "";
    }
    if (rec.stat) rec.stat.textContent = p.last_poll_ms ? p.last_poll_ms.toFixed(1) + " " + t("ms") : "";
  }
  updateComputedTick();
}

function sparklineSvgOnly(pollId, r) {
  const key = pushSpark(pollId, r, String(r.scaled != null ? r.scaled : r.raw));
  return sparkLineSvg(SPARK_HIST[key]);
}

function updateComputedTick() {
  const computed = (snap && snap.computed) || [];
  const cc = GRID.comp;
  if (!cc) return;
  for (const c of computed) {
    const ref = cc.rows.get(c.name);
    if (!ref) continue;
    ref.val.innerHTML = `${esc(c.display)}${c.ok ? "" : ' <span class="calc-err">⚠</span>'}`;
    ref.expr.innerHTML = `${esc(c.expr)}${c.ok ? "" : " — " + esc(c.err)}`;
    ref.expr.className = "mono " + (c.ok ? "sub" : "calc-err");
    ref.tr.title = c.ok ? "" : (c.err || "");
  }
}

// Card with computed (virtual) tags: name / value / unit / expression.
function computedCard(computed) {
  const card = document.createElement("div");
  card.className = "card";
  const head = document.createElement("div");
  head.className = "card-head";
  head.innerHTML = `<h2>${esc(t("p_calc"))}</h2>
    <span class="meta">${esc(t("calc_card_meta"))}</span>`;
  card.appendChild(head);
  const table = document.createElement("table");
  table.className = "regtable";
  table.innerHTML = `<thead><tr><th>${t("th_name")}</th><th>${t("th_value")}</th><th>${t("th_unit")}</th><th>${t("f_expr")}</th></tr></thead>`;
  const tb = document.createElement("tbody");
  const rec = { rows: new Map() };
  for (const c of computed) {
    const tr = document.createElement("tr");
    tr.className = "calc-row";
    if (!c.ok) tr.title = c.err || "";
    tr.innerHTML = `<td><b>${esc(c.name)}</b></td>
      <td class="mono"><b>${esc(c.display)}</b>${c.ok ? "" : ' <span class="calc-err">⚠</span>'}</td>
      <td>${esc(c.unit || "")}</td>
      <td class="mono ${c.ok ? "sub" : "calc-err"}">${esc(c.expr)}${c.ok ? "" : " — " + esc(c.err)}</td>`;
    rec.rows.set(c.name, { tr, val: tr.children[1], expr: tr.children[3] });
    tb.appendChild(tr);
  }
  table.appendChild(tb);
  card.appendChild(table);
  GRID.comp = rec;
  return card;
}

// Cheap signatures: skip DOM work when nothing visible changed.
const SIGS = {};
function sigChanged(key, val) {
  const s = JSON.stringify(val);
  if (SIGS[key] === s) return false;
  SIGS[key] = s;
  return true;
}

function renderStats() {
  const box = $("stats");
  const conns = snap ? snap.connections : [];
  // «N с назад» тикают посекундно — обновляем раз в секунду, не каждый тик.
  const secs = snap ? Math.floor(Number(snap.ts_ms) / 1000) : 0;
  if (!sigChanged("stats", [window.LANG, secs, conns, (snap ? snap.polls : []).map((p) => [p.id, p.stats, p.last_poll_ms])])) return;
  box.innerHTML = "";
  if (!conns.length) { box.innerHTML = `<div class="empty">—</div>`; return; }
  const ago = (ts) => ts ? tfmt("ago", { s: Math.max(0, Math.round((snap.ts_ms - ts) / 1000)) }) : "—";
  for (const c of conns) {
    const s = c.stats;
    box.insertAdjacentHTML("beforeend", `
      <div class="stat-line"><span>${esc(c.name)}</span><b class="${s.online ? "good" : "bad"}">${s.online ? "online" : "offline"}</b></div>
      <div class="stat-line"><span>TX / RX</span><b>${s.tx} / ${s.rx}</b></div>
      <div class="stat-line"><span>${t("err_timeouts")}</span><b>${s.errors} / ${s.timeouts}</b></div>
      <div class="stat-line"><span>${t("reconnects")}</span><b>${s.reconnects || 0}</b></div>
      <div class="stat-line"><span>${t("resp")}</span><b>${s.last_ms.toFixed(1)} / ${s.avg_ms.toFixed(1)}</b></div>
      <div class="stat-line"><span>${t("last_ok")}</span><b>${ago(s.last_ok_ts)}</b></div>
      ${s.last_error ? `<div class="stat-line"><span class="bad">${esc(s.last_error)}</span></div>` : ""}`);
    // Per-poll diagnostics (only polls of this connection)
    const polls = (snap ? snap.polls : []).filter((p) => p.conn_id === c.id);
    for (const p of polls) {
      const st = p.stats || { ok: 0, err: 0, last_ok_ts: 0 };
      box.insertAdjacentHTML("beforeend", `
        <div class="stat-line"><span class="sub">· ${esc(p.name)}</span>
          <b class="sub">ok ${st.ok} / err ${st.err} · ${p.last_poll_ms ? p.last_poll_ms.toFixed(1) + " " + t("ms") : "—"} · ${ago(st.last_ok_ts)}</b></div>`);
    }
  }
}

function renderLog() {
  const box = $("log");
  const log = snap ? snap.log : [];
  const ent = (e) => e.time + "\u0001" + e.dir + "\u0001" + e.text;
  const top = log.length ? ent(log[0]) : "";
  if (!LOGR.n) {
    box.innerHTML = "";
    for (const e of log.slice(0, 150)) appendLogLine(box, e);
    LOGR.n = Math.min(log.length, 150); LOGR.top = top;
    return;
  }
  if (log.length >= LOGR.n && top === LOGR.top) return; // ничего нового
  const knownIdx = log.slice(0, 80).findIndex((e) => ent(e) === LOGR.top);
  if (log.length < LOGR.n || knownIdx === -1) {
    // рестарт движка / слишком большой сдвиг — полная перерисовка
    box.innerHTML = "";
    for (const e of log.slice(0, 150)) appendLogLine(box, e);
    LOGR.n = Math.min(log.length, 150); LOGR.top = top;
    return;
  }
  // Дописываем только новые записи сверху (новые идут первыми).
  const frag = document.createDocumentFragment();
  for (const e of log.slice(0, knownIdx)) appendLogLine(frag, e);
  box.insertBefore(frag, box.firstChild);
  while (box.children.length > 150) box.removeChild(box.lastChild);
  LOGR.n = Math.min(log.length, 150); LOGR.top = top;
}

function appendLogLine(frag, e) {
  const el = document.createElement("div");
  el.className = "e " + (e.ok ? "" : "bad");
  el.innerHTML = `<span class="dir">${e.time}</span><span class="dir">${e.dir}</span> ${esc(e.text)}`;
  frag.appendChild(el);
}
const LOGR = { n: 0, top: "" };

function fillSelectors() {
  const conns = cfg.connections;
  const pConn = $("pConn");
  if (pConn.dataset.n !== String(conns.length)) {
    pConn.dataset.n = conns.length;
    pConn.innerHTML = conns.map((c) => `<option value="${c.id}">${esc(c.name)}</option>`).join("");
  }
  fillRegSelects();
  // Picker окна параметров: перестраиваем при изменении опросов/тэгов.
  const wp = $("watchPick");
  if (wp) {
    const wsig = cfg.polls.map((p) => `${p.id}:${p.start}:${p.qty}`).join("|") +
      "#" + (cfg.computed || []).map((c) => c.name).join(",");
    if (wp.dataset.sig !== wsig) { wp.dataset.sig = wsig; watchFillPicker(); }
  }
}

function stepOf(fmt) { return fmtRegs(fmt); }
function applyAddrMode(mode) {
  for (const p of cfg.polls) {
    const raw = (p.raw_start != null) ? p.raw_start : p.start;
    let start = raw;
    let notation = mode;
    if (mode === "4x" && raw >= 0x4000 && raw <= 0x4fff) start = raw - 0x4000;
    else if (mode === "4xxxx" && raw >= 40001 && raw <= 49999) start = raw - 40001;
    else if (mode === "base1") notation = "";
    p.start = start;
    p.raw_start = raw;
    p.notation = notation;
    p.base_one = (mode === "base1");
  }
}
function dispAddr(cp, addr) {
  const n = cp.notation || "";
  if (n === "4x") return "4x" + addr.toString(16).toUpperCase().padStart(4, "0");
  if (n === "3x") return "3x" + addr.toString(16).toUpperCase().padStart(4, "0");
  if (n === "1x") return "1x" + addr.toString(16).toUpperCase().padStart(4, "0");
  if (n === "4xxxx") return String(40001 + addr);
  if (n === "3xxxx") return String(30001 + addr);
  return String(cp.base_one ? addr + 1 : addr);
}
function findCfgPoll(id) { return cfg.polls.find((p) => p.id === id); }
function editPoll(p) {
  editingPoll = p;
  $("pName").value = p.name;
  $("pConn").value = p.conn_id;
  $("pSlave").value = p.slave;
  $("pFunc").value = p.func;
  $("pStart").value = (p.raw_start != null) ? p.raw_start : p.start;
  $("pQty").value = p.qty;
  $("pScan").value = p.scan_ms;
  $("pFmt").value = p.fmt;
  $("pMult").value = p.mult;
  $("pDiv").value = p.div;
  $("pOffset").value = p.offset;
  $("pUnit").value = p.unit || "";
  $("pBase").checked = !!p.base_one;
  $("detPoll").open = true;
  $("btnPollSubmit").textContent = t("edit_poll");
  $("btnPollCancel").hidden = false;
}
function endPollEdit() {
  editingPoll = null;
  $("pSlave").value = 0; // новый полл по умолчанию следует за соединением
  $("btnPollSubmit").textContent = t("b_add_poll");
  $("btnPollCancel").hidden = true;
}
function regLabel(p, addr) {
  return (p.labels && p.labels[String(addr)]) || `${p.name}[${addr}]`;
}
function regOptionsHTML() {
  const opts = [];
  for (const p of cfg.polls) {
    const step = stepOf(p.fmt);
    const n = Math.max(1, Math.floor((p.qty || 1) / step));
    for (let i = 0; i < n; i++) {
      const addr = (p.start || 0) + i * step;
      opts.push(`<option value="${p.id}:${addr}">${esc(p.name)} · ${addr} — ${esc(regLabel(p, addr))}</option>`);
    }
  }
  return opts.join("");
}
function fillRegSelects() {
  const sig = cfg.polls.map((p) => `${p.id}:${p.start}:${p.qty}`).join("|");
  for (const id of ["rReg", "wReg"]) {
    const el = $(id);
    if (el && el.dataset.sig !== sig) {
      el.dataset.sig = sig;
      el.innerHTML = regOptionsHTML();
    }
  }
}

/* --------------------------------------------------------------- actions -- */
async function apply() {
  try { if (hasTauri) await invoke("apply_config", { cfg }); } catch (e) { footer(String(e), true); }
  render();
}

async function startPolling() {
  try { if (hasTauri) await invoke("start"); refresh(); } catch (e) { footer(String(e), true); }
}
async function stopPolling() {
  try { if (hasTauri) await invoke("stop"); refresh(); } catch (e) { footer(String(e), true); }
}

async function writeRow(poll, row) {
  if (!hasTauri) return;
  const cfgP = findCfgPoll(poll.id) || poll;
  const fmt = cfgP.fmt || "u16";
  if (Number(poll.func) === 4) { footer(t("wr_input_ro"), true); return; }
  const isBits = Number(poll.func) === 1 || Number(poll.func) === 2;
  const regs0 = row.regs && row.regs.length ? row.regs : [row.raw];
  // Текущее значение — в том виде, в котором его удобно ввести:
  // float/32-бит — инженерное значение, hex/bin — биты регистров.
  const cur = isBits ? (row.raw ? "1" : "0")
    : (fmt === "hex" || fmt === "hex16") ? regBitsText(regs0, "hex")
    : (fmt === "hex32") ? regBitsText(regs0, "hex")
    : (fmt === "bin" || fmt === "bin16") ? regBitsText(regs0, "bin")
    : (fmt === "ascii16") ? String.fromCharCode((row.raw >> 8) & 0xff, row.raw & 0xff)
    : String(row.scaled != null ? row.scaled : (row.value != null ? row.value : row.raw));
  openValueModal(
    tfmt("write_title", { func: FUNC[poll.func] || poll.func, addr: row.disp_addr, name: row.name, cur }) + " [" + (FMTL[fmt] || fmt) + "]",
    cur,
    async (val) => {
      try {
        if (isBits) {
          await invoke("write_value", { pollId: poll.id, addr: row.addr, value: Number(val) ? 1 : 0 });
        } else {
          // regEncode пакует значение в регистры по fmt (float -> 2 регистра
          // с учётом порядка слов, i16 -> допкод, hex/bin — парсит префиксы).
          const regs = regEncode(val, fmt);
          if (regs.length === 1) await invoke("write_value", { pollId: poll.id, addr: row.addr, value: regs[0] });
          else await invoke("write_multi", { pollId: poll.id, addr: row.addr, values: regs });
        }
        refresh();
      } catch (e) { footer(String(e), true); }
    }
  );
}

function openValueModal(title, value, cb, okLabel) {
  const m = $("modal"), inp = $("modalInput"), okBtn = $("modalOk");
  const prevOk = okBtn.textContent;
  if (okLabel) okBtn.textContent = okLabel;
  $("modalTitle").textContent = title;
  inp.value = value;
  m.hidden = false;
  setTimeout(() => { inp.focus(); inp.select(); }, 0);
  const done = (okPressed) => {
    m.hidden = true;
    okBtn.textContent = prevOk;
    okBtn.onclick = null;
    $("modalCancel").onclick = null;
    inp.onkeydown = null;
    if (okPressed) cb(inp.value);
  };
  okBtn.onclick = () => done(true);
  $("modalCancel").onclick = () => done(false);
  inp.onkeydown = (e) => {
    if (e.key === "Enter") done(true);
    else if (e.key === "Escape") done(false);
  };
}

async function openDialog(opts) {
  const r = await invoke("plugin:dialog|open", { options: opts });
  return r == null ? null : (Array.isArray(r) ? r : [r]);
}

async function saveProject() {
  if (!hasTauri) return;
  try {
    const path = await invoke("plugin:dialog|save", {
      options: {
        title: t("save_dialog"),
        defaultPath: "project.mbscada.json",
        filters: [{ name: "mbSCADA project", extensions: ["json"] }],
      },
    });
    if (!path) { footer(t("save_cancelled")); return; }
    await invoke("save_project", { path, data: JSON.stringify(cfg, null, 2) });
    footer(t("saved") + path);
  } catch (e) { footer(String(e), true); }
}

async function loadProject() {
  if (!hasTauri) return;
  try {
    const sel = await openDialog({
      title: t("open_dialog"),
      multiple: false,
      filters: [{ name: "mbSCADA project", extensions: ["json"] }],
    });
    if (!sel || !sel.length) { footer(t("open_cancelled")); return; }
    cfg = JSON.parse(await invoke("load_project", { path: sel[0] }));
    // Old projects may lack the newer sections — normalize before use.
    if (!Array.isArray(cfg.computed)) cfg.computed = [];
    if (!cfg.logfile) cfg.logfile = { enabled: false, path: "" };
    if (!cfg.archive) cfg.archive = { enabled: false, path: "", interval_s: 5, all_values: false };
    if (!Array.isArray(cfg.mimic)) cfg.mimic = [];
    watchCfg();
    slaveCfg();
    await apply();
    footer(t("loaded") + sel[0]);
  } catch (e) { footer(String(e), true); }
}

async function importFiles() {
  if (!hasTauri) return;
  try {
    const sel = await openDialog({
      title: t("pick_files"),
      multiple: true,
      filters: [{ name: "Modbus Poll", extensions: ["mbp", "mbw"] }],
    });
    if (!sel || !sel.length) { footer(t("import_cancelled")); return; }
    await doImport(sel);
  } catch (e) { footer(String(e), true); }
}

async function importFolder() {
  if (!hasTauri) return;
  try {
    const sel = await openDialog({
      title: t("pick_folder"),
      directory: true,
      multiple: false,
    });
    if (!sel || !sel.length) { footer(t("import_cancelled")); return; }
    await doImport([sel[0]]);
  } catch (e) { footer(String(e), true); }
}

async function doImport(paths) {
  const res = await invoke("import_paths", { paths });

  // Соединения из файлов НЕ подхватываются: опросы привязываются к
  // соединениям, созданным пользователем. Сначала ищем совпадение по
  // host:port, иначе — первое существующее соединение, иначе — пусто
  // (опрос остаётся без связи, помечаем его в списке).
  const idMap = {};
  (res.connections || []).forEach((c) => {
    const same = cfg.connections.find(
      (u) => (u.host || "") === (c.host || "") && (u.port || 0) === (c.port || 0)
    );
    idMap[c.id] = same ? same.id : (cfg.connections[0] ? cfg.connections[0].id : "");
  });
  res.polls.forEach((p) => {
    if (idMap[p.conn_id] !== undefined) p.conn_id = idMap[p.conn_id];
    else if (cfg.connections.length) p.conn_id = cfg.connections[0].id;
  });

  let nid = cfg.polls.reduce((m, p) => Math.max(m, p.id || 0), 0) + 1;
  res.polls.forEach((p) => { p.id = nid++; });
  cfg.polls.push(...res.polls);

  await apply();
  const ok = res.reports.filter((r) => r.ok).length;
  const bad = res.reports.filter((r) => !r.ok);
  const unbound = res.polls.filter((p) => !p.conn_id).length;
  let msg = tfmt("imp_summary", { n: res.reports.length, ok, bad: bad.length, p: res.polls.length });
  if (unbound) msg += tfmt("imp_unbound", { n: unbound });
  footer(msg, unbound > 0);
  if (bad.length) console.warn("не импортировано:", bad);
  console.log("import reports", res.reports);
}

/* ---------------------------------------------------------------- forms -- */
function wireForms() {
  $("btnStart").onclick = startPolling;
  $("btnStop").onclick = stopPolling;
  $("btnSave").onclick = saveProject;
  $("btnLoad").onclick = loadProject;
  const bc = $("csvBtn");
  if (bc) bc.onclick = csvToggle;
  $("btnImportFiles").onclick = importFiles;
  $("btnImportFolder").onclick = importFolder;
  const bp = $("btnPorts");
  if (bp) bp.onclick = refreshPorts;
  const bt = $("btnTestConn");
  if (bt) bt.onclick = testConn;

  // Панель аварий: звук, квитирование, история, вкладки.
  const bs = $("btnAlarmSound");
  if (bs) {
    bs.textContent = AL.sound ? "🔊" : "🔇";
    bs.onclick = () => {
      AL.sound = !AL.sound;
      localStorage.setItem("mbscada.alSound", AL.sound ? "1" : "0");
      bs.textContent = AL.sound ? "🔊" : "🔇";
      if (AL.sound) beep();
    };
  }
  const ba = $("btnAckAll");
  if (ba) ba.onclick = () => { for (const a of Object.values(AL.active)) a.ack = true; renderAlarms(); };
  const bc2 = $("btnAlarmClear");
  if (bc2) bc2.onclick = () => { AL.hist = []; renderAlarms(); };
  document.querySelectorAll(".atab").forEach((t) => {
    t.onclick = () => {
      document.querySelectorAll(".atab").forEach((x) => x.classList.remove("active"));
      t.classList.add("active");
      AL.tab = t.dataset.at;
      const act = $("alarmActive"), hist = $("alarmHist");
      if (act) act.hidden = AL.tab !== "active";
      if (hist) hist.hidden = AL.tab !== "hist";
    };
  });

  // Окно просматриваемых параметров: открыть/закрыть, сброс min/max,
  // добавить выбранный параметр, пауза перерисовки под курсором.
  const bw = $("btnWatch");
  if (bw) bw.onclick = () => watchToggle();
  const wc = $("watchClose");
  if (wc) wc.onclick = () => watchToggle(false);
  const wr = $("watchReset");
  if (wr) wr.onclick = () => { WATCH.stats = {}; renderWatch(); };
  const wa = $("watchAdd");
  if (wa) wa.onclick = watchAdd;
  const wl = $("watchList");
  if (wl) {
    wl.addEventListener("mouseenter", () => { WATCH.hover = true; });
    wl.addEventListener("mouseleave", () => { WATCH.hover = false; });
  }
  const wd = $("watchDrag");
  if (wd) wd.addEventListener("mousedown", (ev) => {
    if (ev.target.tagName === "BUTTON") return;
    const r = $("watchwin").getBoundingClientRect();
    WATCH.pos = { x: r.left, y: r.top };
    wdrag = { sx: ev.clientX, sy: ev.clientY, ox: r.left, oy: r.top };
    ev.preventDefault();
  });

  const am = $("addrMode");
  if (am) am.onchange = () => { applyAddrMode(am.value); apply(); };

  // Slave-симулятор: старт/стоп + сохранение настроек в проекте.
  const sbs = $("slBtnStart");
  if (sbs) sbs.onclick = slaveStart;
  const sbp = $("slBtnStop");
  if (sbp) sbp.onclick = slaveStop;
  const slaveFields = [
    ["slPort", "port", Number], ["slUnit", "unit", Number], ["slRtu", "mode", null],
    ["slArea", "area", null], ["slAddr", "addr", Number], ["slQty", "qty", Number],
    ["slFmt", "fmt", null], ["slGen", "gen", null],
    ["slGenAmp", "genAmp", Number], ["slGenBase", "genBase", Number], ["slGenPeriod", "genPeriod", Number],
  ];
  for (const [id, key, cast] of slaveFields) {
    const el = $(id);
    if (!el) continue;
    el.onchange = () => {
      const g = slaveCfg();
      g[key] = cast ? cast(el.value) : el.value;
      if (key === "mode") {
        const bar = $("slSerialBar");
        if (bar) bar.hidden = g.mode !== "serial";
        if (g.mode === "serial") fillSlaveSerialPorts();
      }
      if (["area", "addr", "qty", "fmt"].includes(key)) {
        SLV.gridSig = null;
        if (SLV.running) slaveRefreshNow();
      }
    };
  }
  const slaveSerialFields = [
    ["slSerPort", "port", null], ["slSerBaud", "baud", Number],
    ["slSerParity", "parity", null], ["slSerData", "data", Number], ["slSerStop", "stop", Number],
  ];
  for (const [id, key, cast] of slaveSerialFields) {
    const el = $(id);
    if (!el) continue;
    el.onchange = () => {
      const g = slaveCfg();
      g.serial[key] = cast ? cast(el.value) : el.value;
    };
  }
  const srf = $("slSerRefresh");
  if (srf) srf.onclick = () => fillSlaveSerialPorts(true);

  document.querySelectorAll(".tab").forEach((t) => {
    t.onclick = () => {
      document.querySelectorAll(".tab").forEach((x) => x.classList.remove("active"));
      t.classList.add("active");
      const v = t.dataset.view;
      $("viewTables").hidden = v !== "tables";
      $("viewSchema").hidden = v !== "schema";
      if ($("viewSlave")) $("viewSlave").hidden = v !== "slave";
      if (v === "schema") renderSchema();
      if (v === "slave") { slaveSyncForm(); renderSlaveStatus(); slaveRefreshNow(); }
    };
  });
  const addBtn = $("wAdd");
  if (addBtn) addBtn.onclick = addWidget;

  $("cKind").onchange = kindFields;

  $("btnConnCancel").onclick = () => { endConnEdit(); resetConnForm(); };
  $("btnPollCancel").onclick = () => { endPollEdit(); };
  $("btnRuleCancel").onclick = () => { endRuleEdit(); };

  // Вычисляемые тэги: добавление/правка/удаление.
  $("calcForm").onsubmit = (e) => {
    e.preventDefault();
    const item = calcFromForm();
    if (!item.name) { footer(t("calc_need_name"), true); return; }
    if (!item.expr) { footer(t("calc_need_expr"), true); return; }
    if (cfg.computed.length >= 64) { footer(t("calc_limit"), true); return; }
    if (editingCalc) {
      Object.assign(editingCalc, item);
      endCalcEdit();
    } else {
      cfg.computed.push(item);
    }
    apply();
  };
  $("btnCalcCancel").onclick = endCalcEdit;

  // Журнал (движок пишет события в файл) и настройки CSV-архива.
  const le = $("logEnabled"), lp = $("logPath");
  if (le) le.onchange = async () => {
    cfg.logfile.enabled = le.checked;
    if (le.checked && !cfg.logfile.path) {
      try { const sp = await invoke("suggest_paths"); cfg.logfile.path = sp.log; } catch { /* без подсказки */ }
    }
    lp.value = cfg.logfile.path || "";
    await apply();
    footer(le.checked ? tfmt("log_on", { path: cfg.logfile.path }) : t("log_off"));
  };
  if (lp) lp.onchange = () => { cfg.logfile.path = lp.value.trim(); apply(); };
  const aa = $("archAll"), ap = $("archPeriod");
  if (aa) aa.onchange = () => { cfg.archive.all_values = aa.checked; apply(); };
  if (ap) ap.onchange = () => { cfg.archive.interval_s = Math.max(1, Number(ap.value) || 5); ap.value = cfg.archive.interval_s; apply(); };

  // Смена соединения в форме опроса — по умолчанию полл следует за адресом
  // соединения (0), поэтому смена Slave ID в коннекшене подхватывается
  // опросами автоматически; конкретный адрес вводится вручную (1–247).
  $("pConn").onchange = () => {
    $("pSlave").value = 0;
  };

  $("connForm").onsubmit = (e) => {
    e.preventDefault();
    const c = connFromForm();
    c.name = $("cName").value || c.name;
    if (editingConn) {
      // id не меняем — на него ссылаются опросы.
      const oldSlave = editingConn.slave;
      c.id = editingConn.id;
      Object.assign(editingConn, c);
      // Поллы, чей Slave ID просто повторял прежний адрес соединения,
      // переводим в режим «следовать за соединением» (0): смена адреса
      // в коннекшене отныне подхватывается опросами автоматически.
      if (oldSlave != null && oldSlave !== c.slave) {
        for (const p of cfg.polls) {
          if (p.conn_id === c.id && p.slave === oldSlave) p.slave = 0;
        }
      }
      endConnEdit();
      resetConnForm();
    } else {
      cfg.connections.push(c);
    }
    apply();
  };

  $("pollForm").onsubmit = (e) => {
    e.preventDefault();
    const vals = {
      name: $("pName").value || "Poll",
      conn_id: $("pConn").value,
      slave: Number($("pSlave").value),
      func: Number($("pFunc").value),
      start: Number($("pStart").value),
      raw_start: Number($("pStart").value),
      qty: Number($("pQty").value),
      scan_ms: Number($("pScan").value),
      fmt: $("pFmt").value,
      mult: Number($("pMult").value),
      div: Number($("pDiv").value),
      offset: Number($("pOffset").value),
      unit: $("pUnit").value,
      base_one: $("pBase").checked,
      notation: "raw",
    };
    if (editingPoll) {
      Object.assign(editingPoll, vals);
      endPollEdit();
    } else {
      cfg.polls.push(Object.assign({ id: Date.now() % 100000, enabled: true, labels: {}, alarms: {} }, vals));
    }
    apply();
  };

  $("ruleForm").onsubmit = (e) => {
    e.preventDefault();
    const [pid, addr] = ($("rReg").value || "").split(":").map(Number);
    const poll = findCfgPoll(pid);
    if (!poll) { footer(t("pick_reg_first"), true); return; }
    const offset = Math.round((addr - (poll.start || 0)) / stepOf(poll.fmt));
    const kind = $("rKind").value;
    const v = Number($("rValue").value);
    let cond;
    if (kind === "in_range" || kind === "outside_range") cond = { kind, lo: v, hi: Number($("rValue2").value) };
    else if (kind === "bit") cond = { kind, n: Number($("rValue2").value), set: true };
    else cond = { kind, v };
    const sev = $("rSeverity").value;
    const t = $("rText").value.trim();
    const item = {
      name: $("rMessage").value || kind,
      poll: pid, offset,
      cond, fg: $("rFg").value, bg: $("rBg").value, blink: $("rBlink").checked,
      severity: sev || null, message: $("rMessage").value || null,
      text: t || null,
    };
    if (editingRule) {
      // Правку ветки можно перенести и на другой регистр — просто берём
      // значения из формы; id и порядок сохраняются.
      Object.assign(editingRule, item);
      endRuleEdit();
    } else {
      // Up to 32 value-dependent branches per register (Modbus Poll allows 2
      // colour rules per window; we extend this to 32 branches per row).
      const used = cfg.rules.filter((r) => r.poll === pid && r.offset === offset).length;
      if (used >= 32) { footer(t("limit32"), true); return; }
      cfg.rules.push(Object.assign({ id: Date.now() % 100000 }, item));
    }
    apply();
  };
}

/* ------------------------------------------------------ slave simulator -- */
/* The app answers as a Modbus device (mb-sim on a TCP port), so the master
   side (the tables) and the slave side are both local. The register grid
   edits the slave's data model live; the auto-generator animates it. */
const SLV = { running: false, addr: "", unit: 1, rtu: false, mode: "", clients: 0, gridSig: null };

function slaveCfg() {
  if (!cfg.slave || typeof cfg.slave !== "object") cfg.slave = {};
  const g = cfg.slave;
  if (g.port == null) g.port = 10502;
  if (g.unit == null) g.unit = 1;
  if (g.rtu == null) g.rtu = false;
  // v1.4: transport mode — migrated from the legacy rtu bool
  if (!g.mode) g.mode = g.rtu ? "rtutcp" : "tcp";
  if (!g.serial || typeof g.serial !== "object") g.serial = {};
  if (!g.serial.port) g.serial.port = "";
  if (g.serial.baud == null) g.serial.baud = 9600;
  if (!g.serial.parity) g.serial.parity = "E";
  if (g.serial.data == null) g.serial.data = 8;
  if (g.serial.stop == null) g.serial.stop = 1;
  if (!g.area) g.area = "holding";
  if (g.addr == null) g.addr = 0;
  if (g.qty == null) g.qty = 16;
  if (!g.fmt) g.fmt = "u16";
  if (!g.gen) g.gen = "none";
  if (g.genAmp == null) g.genAmp = 100;
  if (g.genBase == null) g.genBase = 0;
  if (g.genPeriod == null) g.genPeriod = 10;
  return g;
}

function slaveSyncForm() {
  const g = slaveCfg();
  const map = {
    slPort: "port", slUnit: "unit", slRtu: "mode", slArea: "area", slAddr: "addr",
    slQty: "qty", slFmt: "fmt", slGen: "gen", slGenAmp: "genAmp",
    slGenBase: "genBase", slGenPeriod: "genPeriod",
  };
  for (const [id, key] of Object.entries(map)) {
    const el = $(id);
    if (!el) continue;
    el.value = String(g[key]);
  }
  const ser = g.serial;
  const smap = { slSerPort: "port", slSerBaud: "baud", slSerParity: "parity", slSerData: "data", slSerStop: "stop" };
  for (const [id, key] of Object.entries(smap)) {
    const el = $(id);
    if (el && ser[key] !== undefined && ser[key] !== "") el.value = String(ser[key]);
  }
  const bar = $("slSerialBar");
  if (bar) bar.hidden = g.mode !== "serial";
  if (g.mode === "serial") fillSlaveSerialPorts();
}

function slaveModeLabel() {
  if (SLV.mode === "serial") return "Serial RTU";
  if (SLV.mode === "rtutcp") return "RTU/TCP";
  if (SLV.mode === "tcp") return "TCP";
  return SLV.rtu ? "RTU/TCP" : "TCP";
}

function renderSlaveStatus() {
  const el = $("slStatus");
  if (el) {
    el.innerHTML = SLV.running
      ? `<b class="good">✓ ${esc(SLV.addr)}</b> · Slave ${esc(String(SLV.unit))} · ${slaveModeLabel()}${SLV.mode === "serial" ? "" : ` · ${esc(t("sl_clients"))}: ${SLV.clients}`}`
      : `<span class="sub">${esc(t("sl_off"))}</span>`;
  }
  const b1 = $("slBtnStart"), b2 = $("slBtnStop");
  if (b1) b1.disabled = SLV.running;
  if (b2) b2.disabled = !SLV.running;
}

async function slaveStart() {
  if (!hasTauri) { footer(t("sl_need_app"), true); return; }
  const g = slaveCfg();
  try {
    const args = {
      port: Math.max(1, Math.min(65535, Number(g.port) || 10502)),
      unit: Math.max(0, Math.min(247, Number(g.unit) || 1)),
      mode: g.mode || "tcp",
      serial: null,
    };
    if (args.mode === "serial") {
      if (!g.serial.port) { footer(t("sl_ser_pick"), true); return; }
      args.serial = {
        port: g.serial.port,
        baud: Number(g.serial.baud) || 9600,
        parity: g.serial.parity || "E",
        dataBits: Number(g.serial.data) || 8,
        stopBits: Number(g.serial.stop) || 1,
      };
    }
    const st = await invoke("slave_start", args);
    SLV.running = st.running; SLV.addr = st.addr; SLV.unit = st.unit;
    SLV.rtu = st.rtu; SLV.mode = st.mode; SLV.clients = st.clients;
    renderSlaveStatus();
    footer(t("sl_started") + st.addr);
    slaveReadAndFill();
  } catch (e) { footer(String(e), true); }
}

async function slaveStop() {
  if (!hasTauri) { footer(t("sl_need_app"), true); return; }
  try { await invoke("slave_stop"); } catch { /* уже остановлен */ }
  SLV.running = false; SLV.clients = 0;
  renderSlaveStatus();
  footer(t("sl_stopped"));
}

// Called from the refresh tick: status + generator + (on the tab) the grid.
async function slaveTick() {
  if (!hasTauri) return;
  const view = $("viewSlave");
  const vis = view && !view.hidden;
  if (!vis && !SLV.running) return;
  try {
    const st = await invoke("slave_status");
    SLV.running = st.running; SLV.addr = st.addr; SLV.unit = st.unit;
    SLV.rtu = st.rtu; SLV.mode = st.mode; SLV.clients = st.clients;
    if (SLV.running) slaveGen();
    if (vis) { renderSlaveStatus(); await slaveReadAndFill(); }
  } catch { /* приложение закрывается */ }
}

async function slaveRefreshNow() {
  if (!hasTauri) return;
  try {
    const st = await invoke("slave_status");
    SLV.running = st.running; SLV.addr = st.addr; SLV.unit = st.unit;
    SLV.rtu = st.rtu; SLV.mode = st.mode; SLV.clients = st.clients;
    renderSlaveStatus();
    await slaveReadAndFill();
  } catch (e) { footer(String(e), true); }
}

// Автогенерация значений (синус/пила/шум) в выбранной области — мастеры
// (внешние и сам мастер этого приложения) видят «живое устройство».
function slaveGen() {
  const g = slaveCfg();
  if (g.gen === "none" || !SLV.running) return;
  const qty = Math.max(1, Math.min(200, Number(g.qty) || 16));
  const start = Math.max(0, Math.min(65535, Number(g.addr) || 0));
  const isBits = g.area === "coils" || g.area === "discrete";
  const amp = Number(g.genAmp) || 0;
  const base = Number(g.genBase) || 0;
  const period = Math.max(1, Number(g.genPeriod) || 10);
  const t = Date.now() / 1000;
  const vals = [];
  for (let i = 0; i < qty; i++) {
    let v;
    if (g.gen === "sine") v = base + amp * Math.sin((t / period) * 2 * Math.PI + i * 0.35);
    else if (g.gen === "ramp") v = base + ((t % period) / period) * amp;
    else v = base + Math.random() * amp;
    vals.push(isBits
      ? (amp > 0 && (v - base) / amp > 0.5 ? 1 : 0)
      : Math.max(0, Math.min(65535, Math.round(v))));
  }
  invoke("slave_write", { area: g.area, start, values: vals }).catch(() => {});
}

async function slaveReadAndFill() {
  const g = slaveCfg();
  const body = $("slBody");
  if (!body) return;
  const qty = Math.max(1, Math.min(200, Number(g.qty) || 16));
  const start = Math.max(0, Math.min(65535, Number(g.addr) || 0));
  const isBits = g.area === "coils" || g.area === "discrete";
  const sig = [g.area, start, qty, g.fmt, window.LANG].join("|");
  if (sig !== SLV.gridSig) {
    SLV.gridSig = sig;
    buildSlaveGrid(body, g, start, qty, isBits);
  }
  try {
    const regs = await invoke("slave_read", { area: g.area, start, qty });
    fillSlaveGrid(body, g, start, qty, isBits, regs);
  } catch { /* slave остановили посреди тика */ }
}

function slaveStep(g, isBits) { return isBits ? 1 : fmtRegs(g.fmt); }

function buildSlaveGrid(body, g, start, qty, isBits) {
  const step = slaveStep(g, isBits);
  const nRows = Math.max(1, Math.ceil(qty / step));
  body.innerHTML = "";
  for (let i = 0; i < nRows; i++) {
    const addr = start + i * step;
    const tr = document.createElement("tr");
    tr.innerHTML = `<td class="mono">${addr}</td><td></td><td class="mono sub"></td>`;
    const valTd = tr.children[1], rawTd = tr.children[2];
    if (isBits) {
      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.onchange = async () => {
        try { await invoke("slave_write", { area: g.area, start: addr, values: [cb.checked ? 1 : 0] }); }
        catch (e) { footer(String(e), true); }
      };
      valTd.appendChild(cb);
      tr._input = cb;
    } else {
      const inp = document.createElement("input");
      inp.className = "slin";
      inp.title = t("row_write_hint");
      inp.onchange = async () => {
        try {
          const regs = regEncode(inp.value, g.fmt);
          await invoke("slave_write", { area: g.area, start: addr, values: regs });
          slaveReadAndFill();
        } catch (e) { footer(String(e), true); }
      };
      valTd.appendChild(inp);
      tr._input = inp;
    }
    tr._rawTd = rawTd;
    body.appendChild(tr);
  }
}

function fillSlaveGrid(body, g, start, qty, isBits, regs) {
  const step = slaveStep(g, isBits);
  const nRows = Math.max(1, Math.ceil(qty / step));
  for (let i = 0; i < nRows; i++) {
    const tr = body.children[i];
    if (!tr) { SLV.gridSig = null; return; }
    const slice = regs.slice(i * step, (i + 1) * step);
    if (isBits) {
      if (tr._input !== document.activeElement) tr._input.checked = (Number(slice[0]) || 0) !== 0;
      tr._rawTd.textContent = "";
    } else {
      const v = regDecode(slice, g.fmt)[0];
      const txt = Number.isFinite(v) ? String(Math.round(v * 10000) / 10000) : "—";
      if (tr._input !== document.activeElement) tr._input.value = txt;
      tr._rawTd.textContent = regBitsText(slice, "hex");
    }
  }
}

/* --------------------------------------------------------------- helpers -- */
function synthRows(cp) {
  const f = cp.fmt || "u16";
  const step = fmtRegs(f);
  const n = Math.max(1, Math.floor((cp.qty || 1) / step));
  const labels = cp.labels || {};
  const out = [];
  for (let i = 0; i < n; i++) {
    const addr = (cp.start || 0) + i * step;
    out.push({
      addr, disp_addr: dispAddr(cp, addr),
      name: labels[String(addr)] || `${cp.name}[${addr}]`,
      display: "—", unit: cp.unit || "", quality: "off",
      alarm_level: "ok", alarm_msg: "", fg: null, bg: null, blink: false,
    });
  }
  return out;
}

function describe(c) {
  const sl = ` slave ${c.slave != null ? c.slave : 1}`;
  if (c.kind === "tcp") return "TCP " + c.host + ":" + c.port + sl;
  if (c.kind === "rtu_over_tcp") return "RTU/TCP " + c.host + ":" + c.port + sl;
  if (c.kind === "ascii_over_tcp") return "ASCII/TCP " + c.host + ":" + c.port + sl;
  if (c.kind === "rtu") return `Serial ${c.serial.port} ${c.serial.baud} ${c.serial.parity}${c.serial.data_bits}${c.serial.stop_bits}${sl}`;
  if (c.kind === "ascii") return `ASCII ${c.serial.port}${sl}`;
  return (KINDL[c.kind] || c.kind) + sl;
}
function condText(c) {
  switch (c.kind) {
    case "gt": return `> ${c.v}`; case "ge": return `≥ ${c.v}`; case "lt": return `< ${c.v}`; case "le": return `≤ ${c.v}`;
    case "eq": return `= ${c.v}`; case "ne": return `≠ ${c.v}`;
    case "in_range": return `${c.lo}..${c.hi}`; case "outside_range": return `вне ${c.lo}..${c.hi}`;
    case "bit": return `бит ${c.n}`; default: return c.kind;
  }
}
function miniBtn(txt, fn) { const b = document.createElement("button"); b.className = "mini"; b.textContent = txt; b.onclick = fn; return b; }
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c])); }
function footer(text, bad) { const b = $("errbar"); b.className = "errbar" + (bad ? " bad" : ""); b.textContent = text; }

/* ------------------------------------------------------------- schema ---- */
let drag = null;

function liveMap() {
  const m = {};
  if (snap) for (const p of snap.polls) for (const row of (p.rows || [])) m[`${p.id}:${row.addr}`] = row;
  return m;
}

function addWidget() {
  const val = $("wReg").value;
  if (!val) { footer(t("no_widgets"), true); return; }
  const [pid, addr] = val.split(":").map(Number);
  const poll = findCfgPoll(pid);
  cfg.mimic = cfg.mimic || [];
  const n = cfg.mimic.length;
  const item = {
    id: Date.now(),
    type: $("wType").value,
    x: 30 + (n % 6) * 150,
    y: 30 + Math.floor(n / 6) * 110,
    poll: pid,
    addr,
    label: poll ? regLabel(poll, addr) : String(addr),
  };
  // Настраиваемые дефолты по типу — всё правится двойным кликом.
  if (item.type === "gauge") { item.min = 0; item.max = 100; item.unit = ""; }
  if (item.type === "bar") { item.min = 0; item.max = 100; }
  if (item.type === "button") { item.onVal = 1; item.offVal = 0; item.btnLabel = ""; }
  cfg.mimic.push(item);
  renderSchema();
}

/* Mimic updates in place too: widgets rebuild only when cfg.mimic changes
   (add/delete/configure), live values patch every tick — перестроение рвёт
   перетаскивание и двойные клики. */
const SC = { sig: null, els: new Map() };

function schemaSig() { return JSON.stringify([window.LANG, cfg.mimic]); }

function renderSchema() {
  const canvas = $("canvas");
  if (!canvas || drag) return;
  const s = schemaSig();
  if (s !== SC.sig) {
    SC.sig = s;
    SC.els.clear();
    canvas.innerHTML = "";
    for (const w of (cfg.mimic || [])) buildWidget(canvas, w);
    return;
  }
  updateSchemaTick();
}

function buildWidget(canvas, w) {
  const el = document.createElement("div");
  el.className = "widget";
  el.style.left = w.x + "px";
  el.style.top = w.y + "px";
  const name = w.label || `${w.type} ${w.addr}`;
  const refs = { el, type: w.type };
  if (w.type === "lamp") {
    el.innerHTML = `<div class="lamp"></div><div class="wlabel">${esc(name)}</div>`;
    refs.lamp = el.querySelector(".lamp");
  } else if (w.type === "value") {
    el.innerHTML = `<div class="wval"></div><div class="wlabel">${esc(name)}</div>`;
    refs.val = el.querySelector(".wval");
  } else if (w.type === "bar") {
    el.innerHTML = `<div class="wlabel"></div><div class="bar"><i></i></div>`;
    refs.lab = el.querySelector(".wlabel");
    refs.bar = el.querySelector(".bar i");
  } else if (w.type === "gauge") {
    el.innerHTML = `<div class="ggauge"></div><div class="wlabel">${esc(name)}${w.unit ? " · " + esc(w.unit) : ""}</div>`;
    refs.gauge = el.querySelector(".ggauge");
  } else if (w.type === "button") {
    el.classList.add("wbutton");
    el.innerHTML = `<div class="mbtn"></div><div class="wlabel">${esc(name)}</div>`;
    refs.btn = el.querySelector(".mbtn");
    el.onclick = (ev) => { if (!ev.target.classList.contains("wdel")) buttonWrite(w); };
  } else {
    el.innerHTML = `<div class="wlabel">${esc(name)}</div>`;
  }
  const del = document.createElement("div");
  del.className = "wdel";
  del.textContent = "✕";
  del.onclick = (ev) => {
    ev.stopPropagation();
    cfg.mimic = cfg.mimic.filter((x) => x.id !== w.id);
    renderSchema();
  };
  el.appendChild(del);
  el.ondblclick = () => openWidgetModal(w);
  el.onmousedown = (ev) => startDrag(ev, w, el);
  canvas.appendChild(el);
  SC.els.set(w.id, refs);
}

// Живые значения виджетов — без перестроения DOM.
function updateSchemaTick() {
  const m = liveMap();
  for (const w of (cfg.mimic || [])) {
    const refs = SC.els.get(w.id);
    if (!refs) { SC.sig = null; renderSchema(); return; }
    const row = m[`${w.poll}:${w.addr}`];
    const name = w.label || `${w.type} ${w.addr}`;
    if (refs.type === "lamp") {
      let color = "#1f2937";
      if (row) {
        const lvl = row.alarm_level;
        if (lvl === "hihi" || lvl === "hi") color = "#ef4444";
        else if (lvl === "lolo" || lvl === "lo") color = "#3b82f6";
        else if ((row.raw || 0) > 0) color = "#22c55e";
      }
      refs.lamp.style.background = color;
      refs.lamp.style.boxShadow = `0 0 10px ${color}`;
    } else if (refs.type === "value") {
      refs.val.textContent = row ? `${row.display} ${row.unit || ""}` : "—";
    } else if (refs.type === "bar") {
      const min = Number.isFinite(w.min) ? w.min : 0;
      const max = Number.isFinite(w.max) ? w.max : 100;
      const v = row ? Math.max(0, Math.min(100, ((Number(row.value) || 0) - min) / Math.max(max - min, 1e-9) * 100)) : 0;
      refs.lab.textContent = `${name} ${row ? row.display : "—"}`;
      refs.bar.style.width = v + "%";
    } else if (refs.type === "gauge") {
      const min = Number.isFinite(w.min) ? w.min : 0;
      const max = Number.isFinite(w.max) ? w.max : 100;
      const v = row ? (Number(row.value) || 0) : min;
      refs.gauge.innerHTML = gaugeSvg(v, min, max, row && row.alarm_level);
    } else if (refs.type === "button") {
      const onV = w.onVal != null ? Number(w.onVal) : 1;
      const on = row && (row.raw || 0) === onV;
      refs.btn.className = "mbtn" + (on ? " on" : "");
      refs.btn.textContent = w.btnLabel || (on ? "ВКЛ" : "ВЫКЛ");
    }
  }
}

function gaugeSvg(v, min, max, level) {
  const span = Math.max(max - min, 1e-9);
  const t = Math.max(0, Math.min(1, ((Number(v) || 0) - min) / span));
  // Margins around the arc inside the viewBox so the colored stroke (with
  // round caps) never touches the widget border — see v1.1 overflow fix.
  const cx = 90, cy = 84, r = 60, sw = 12;
  const th = Math.PI * (1 - t);
  const x = cx + r * Math.cos(th), y = cy - r * Math.sin(th);
  const large = t > 0.5 ? 1 : 0;
  const col = level === "hihi" || level === "hi" ? "#ef4444"
    : level === "lolo" || level === "lo" ? "#3b82f6" : "#22d3ee";
  const txt = Number.isFinite(Number(v)) ? String(Math.round(Number(v) * 100) / 100) : "—";
  // Long values shrink instead of overflowing the widget.
  const fs = txt.length > 9 ? 11 : txt.length > 6 ? 13 : 15;
  return `<svg width="164" height="98" viewBox="0 0 180 106">
    <path d="M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}" fill="none" stroke="#1e293b" stroke-width="${sw}" stroke-linecap="round"/>
    ${t > 0.004 ? `<path d="M ${cx - r} ${cy} A ${r} ${r} 0 ${large} 1 ${x.toFixed(1)} ${y.toFixed(1)}" fill="none" stroke="${col}" stroke-width="${sw}" stroke-linecap="round"/>` : ""}
    <text x="90" y="100" text-anchor="middle" class="gtxt" style="font-size:${fs}px">${esc(txt)}</text>
  </svg>`;
}

async function buttonWrite(w) {
  if (!hasTauri) { footer(t("write_only_app"), true); return; }
  const row = liveMap()[w.poll + ":" + w.addr] || null;
  const onV = w.onVal != null ? Number(w.onVal) : 1;
  const offV = w.offVal != null ? Number(w.offVal) : 0;
  const cur = row ? (row.raw || 0) : null;
  // Тумблер: если сейчас «вкл» — пишем «выкл», и наоборот.
  const target = cur === onV ? offV : onV;
  try {
    await invoke("write_value", { pollId: w.poll, addr: w.addr, value: target & 0xffff });
    footer(tfmt("wrote", { addr: w.addr, v: target }));
    refresh();
  } catch (e) { footer(String(e), true); }
}

/* Двойной клик по элементу схемы — настройка: подпись, диапазоны, значения
   кнопки. Поля собираются по типу; сохраняется в cfg.mimic. */
function openWidgetModal(w) {
  const fields = [["label", "Подпись", w.label || ""]];
  if (w.type === "bar" || w.type === "gauge") {
    fields.push(["min", "Минимум", w.min != null ? w.min : 0]);
    fields.push(["max", "Максимум", w.max != null ? w.max : 100]);
  }
  if (w.type === "gauge") fields.push(["unit", "Единицы", w.unit || ""]);
  if (w.type === "button") {
    fields.push(["onVal", "Значение «вкл»", w.onVal != null ? w.onVal : 1]);
    fields.push(["offVal", "Значение «выкл»", w.offVal != null ? w.offVal : 0]);
    fields.push(["btnLabel", "Надпись", w.btnLabel || ""]);
  }
  const box = $("wfields");
  box.innerHTML = fields.map(([id, lab, val]) =>
    `<label>${esc(lab)} <input id="wf_${id}" value="${esc(val)}" /></label>`).join("");
  $("wmodal").hidden = false;
  $("wmodalOk").onclick = () => {
    for (const [id] of fields) {
      const raw = $("wf_" + id).value;
      if (id === "label" || id === "btnLabel" || id === "unit") w[id] = raw;
      else w[id] = Number(raw);
    }
    $("wmodal").hidden = true;
    renderSchema();
  };
  $("wmodalCancel").onclick = () => { $("wmodal").hidden = true; };
}

function startDrag(ev, w, el) {
  if (ev.target.classList.contains("wdel")) return;
  document.querySelectorAll(".widget").forEach((x) => x.classList.remove("sel"));
  el.classList.add("sel");
  drag = { w, el, sx: ev.clientX, sy: ev.clientY, ox: w.x, oy: w.y };
  ev.preventDefault();
}
window.addEventListener("mousemove", (ev) => {
  if (wdrag) {
    WATCH.pos = {
      x: Math.max(0, Math.min(window.innerWidth - 90, wdrag.ox + ev.clientX - wdrag.sx)),
      y: Math.max(0, Math.min(window.innerHeight - 60, wdrag.oy + ev.clientY - wdrag.sy)),
    };
    watchPlace();
    return;
  }
  if (!drag) return;
  drag.w.x = Math.max(0, drag.ox + (ev.clientX - drag.sx));
  drag.w.y = Math.max(0, drag.oy + (ev.clientY - drag.sy));
  drag.el.style.left = drag.w.x + "px";
  drag.el.style.top = drag.w.y + "px";
});
window.addEventListener("mouseup", () => {
  if (wdrag) { wdrag = null; watchSavePos(); }
  drag = null;
});

boot();
