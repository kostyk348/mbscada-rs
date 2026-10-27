// mbSCADA frontend (Modbus Poll style). Config-driven; talks to Rust via IPC.

const hasTauri = !!(window.__TAURI__ && window.__TAURI__.core);
const invoke = hasTauri ? window.__TAURI__.core.invoke : null;

let cfg = { connections: [], polls: [], rules: [] };
let snap = null;
let editingPoll = null;
let editingConn = null;
let editingRule = null;
let guideKey = "";
const $ = (id) => document.getElementById(id);

const FUNC = { 1: "01 Coils", 2: "02 Discr", 3: "03 Holding", 4: "04 Input" };
const FMTL = { u16: "u16", i16: "i16", hex: "hex16", bin: "bin16", ascii16: "ascii16",
  u16_swapped: "u16 swp", u32: "u32", i32: "i32", hex32: "hex32",
  f32_abcd: "f32 ABCD", f32_cdab: "f32 CDAB", f32_badc: "f32 BADC", f32_dcba: "f32 DCBA",
  u64: "u64", i64: "i64", f64: "f64" };
const KINDL = { tcp: "TCP", rtu: "Serial", rtu_over_tcp: "RTU/TCP", ascii: "ASCII", ascii_over_tcp: "ASCII/TCP" };

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
/* Frontend accumulates per-row values each refresh tick and appends only the
   changed ones through the backend (`csv_log`). One file, timestamped rows.  */
const CSV = { on: false, path: "", last: {} };

async function csvToggle() {
  if (!hasTauri) { footer(t("csv_only_tauri"), true); return; }
  if (CSV.on) {
    CSV.on = false;
    CSV.path = "";
    csvBtnState();
    footer(t("csv_stopped"));
    return;
  }
  try {
    const path = await invoke("plugin:dialog|save", {
      options: {
        title: t("csv_dialog"),
        defaultPath: "mbscada-log.csv",
        filters: [{ name: "CSV", extensions: ["csv"] }],
      },
    });
    if (!path) { footer(t("csv_cancelled")); return; }
    CSV.on = true;
    CSV.path = path;
    CSV.last = {};
    await invoke("csv_log", { path: CSV.path, lines: ["time,poll_id,poll,func,addr,name,display,quality"] });
    csvBtnState();
    footer(t("csv_running") + path);
  } catch (e) { footer(String(e), true); }
}

function csvBtnState() {
  const b = $("csvBtn");
  if (!b) return;
  b.textContent = CSV.on ? "■ CSV" : "CSV";
  b.classList.toggle("accent", CSV.on);
}

async function csvMaybeAppend(polls) {
  if (!hasTauri || !CSV.on || !snap) return;
  const lines = [];
  const t = new Date().toISOString();
  for (const p of polls) {
    for (const r of p.rows || []) {
      const key = p.id + ":" + r.disp_addr;
      const sig = r.display + "\u0001" + r.quality;
      if (CSV.last[key] === sig) continue;
      CSV.last[key] = sig;
      lines.push(`${t},${p.id},${csvField(p.name)},${p.func},${r.disp_addr},${csvField(r.name)},${csvField(r.display)},${r.quality}`);
    }
  }
  if (!lines.length) return;
  try { await invoke("csv_log", { path: CSV.path, lines }); } catch { /* keep ticking */ }
}

function csvField(v) {
  const s = String(v == null ? "" : v);
  return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s;
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

function renderAlarms() {
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

// Подстановка {ключей} в словарные строки: tfmt("write_title", {addr: 5, ...})
function tfmt(key, vars) {
  let s = t(key);
  for (const k in vars) s = s.split("{" + k + "}").join(String(vars[k]));
  return s;
}

/* ------------------------------------------------------------ bootstrap -- */
async function boot() {
  if (!Array.isArray(cfg.mimic)) cfg.mimic = [];
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
  setInterval(refresh, 300);
  refresh();
  if (!hasTauri) {
    footer(t("no_tauri"), true);
  }
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
    if (CSV.on && snap) {
      const pollsForCsv = snap.polls.length ? snap.polls : cfg.polls;
      await csvMaybeAppend(pollsForCsv);
    }
    render();
  } catch (e) { footer(t("err_prefix") + e, true); }
}

/* ------------------------------------------------------------- rendering -- */
function render() {
  renderChips();
  renderConnList();
  renderPollList();
  renderRuleList();
  renderGrid();
  renderStats();
  renderLog();
  updateAlarms();
  renderAlarms();
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

function renderGrid() {
  const grid = $("grid");
  grid.innerHTML = "";
  const polls = snap && snap.polls.length ? snap.polls : cfg.polls.map((p) => ({ ...p, rows: [], last_poll_ms: 0 }));
  if (!polls.length) { grid.innerHTML = `<div class="empty" style="padding:30px">${esc(t("grid_empty"))}</div>`; return; }
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
      <span class="meta">${FUNC[p.func] || p.func} · @${p.start}×${p.qty} · slave ${p.slave} · ${p.scan_ms}${t("ms")} · ${FMTL[p.fmt] || p.fmt} · ${connTxt}</span>
      <span class="stat">${p.last_poll_ms ? p.last_poll_ms.toFixed(1) + " " + t("ms") : ""}</span>`;
    card.appendChild(head);

    const table = document.createElement("table");
    table.className = "regtable";
    table.innerHTML = `<thead><tr><th>${t("th_addr")}</th><th>${t("th_name")}</th><th>${t("th_value")}</th><th>${t("th_trend")}</th><th>${t("th_unit")}</th><th>${t("th_quality")}</th><th>${t("th_alarm")}</th></tr></thead>`;
    const tb = document.createElement("tbody");
    const rows = (p.rows && p.rows.length) ? p.rows : synthRows(cfgById[p.id] || p);
    for (const r of rows) {
      const tr = document.createElement("tr");
      tr.className = "row" + (r.blink ? " blink" : "");
      const rowKey = p.id + ":" + r.addr;
      tr.dataset.key = rowKey;
      // Подсветка строки, к которой перешли из панели аварий.
      if (JUMP.key === rowKey && Date.now() < JUMP.until) {
        tr.classList.add("flashrow");
        if (!JUMP.scrolled) {
          JUMP.scrolled = true;
          setTimeout(() => tr.scrollIntoView({ block: "center", behavior: "smooth" }), 0);
        }
      }
      if (r.fg) tr.style.color = r.fg;
      if (r.bg) tr.style.background = r.bg;
      const alarm = r.alarm_level && r.alarm_level !== "ok" ? "⚠ " + esc(r.alarm_msg || r.alarm_level) : "";
      const spark = sparkline(p.id, r, r.display);
      tr.innerHTML = `<td class="mono">${r.disp_addr}</td><td>${esc(r.name)}</td>
        <td class="mono"><b>${esc(r.display)}</b></td><td>${spark}</td><td>${esc(r.unit)}</td>
        <td class="${r.quality}">${r.quality}</td><td>${alarm}</td>`;
      tr.title = t("row_write_hint");
      tr.onclick = () => writeRow(p, r);
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
  }
}

function renderStats() {
  const box = $("stats");
  box.innerHTML = "";
  const conns = snap ? snap.connections : [];
  if (!conns.length) { box.innerHTML = `<div class="empty">—</div>`; return; }
  for (const c of conns) {
    const s = c.stats;
    box.insertAdjacentHTML("beforeend", `
      <div class="stat-line"><span>${esc(c.name)}</span><b class="${s.online ? "good" : "bad"}">${s.online ? "online" : "offline"}</b></div>
      <div class="stat-line"><span>TX / RX</span><b>${s.tx} / ${s.rx}</b></div>
      <div class="stat-line"><span>${t("err_timeouts")}</span><b>${s.errors} / ${s.timeouts}</b></div>
      <div class="stat-line"><span>${t("resp")}</span><b>${s.last_ms.toFixed(1)} / ${s.avg_ms.toFixed(1)}</b></div>
      ${s.last_error ? `<div class="stat-line"><span class="bad">${esc(s.last_error)}</span></div>` : ""}`);
  }
}

function renderLog() {
  const box = $("log");
  box.innerHTML = "";
  const log = snap ? snap.log : [];
  if (!log.length) { box.innerHTML = `<div class="empty">—</div>`; return; }
  for (const e of log.slice(0, 150)) {
    const el = document.createElement("div");
    el.className = "e " + (e.ok ? "" : "bad");
    el.innerHTML = `<span class="dir">${e.time}</span><span class="dir">${e.dir}</span> ${esc(e.text)}`;
    box.appendChild(el);
  }
}

function fillSelectors() {
  const conns = cfg.connections;
  const pConn = $("pConn");
  if (pConn.dataset.n !== String(conns.length)) {
    pConn.dataset.n = conns.length;
    pConn.innerHTML = conns.map((c) => `<option value="${c.id}">${esc(c.name)}</option>`).join("");
  }
  fillRegSelects();
}

function stepOf(fmt) {
  return (fmt && (String(fmt).startsWith("f32") || fmt === "u32" || fmt === "i32")) ? 2 : 1;
}
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
  openValueModal(
    tfmt("write_title", { func: FUNC[poll.func] || poll.func, addr: row.disp_addr, name: row.name, cur: row.display }),
    row.raw,
    async (val) => {
      try {
        await invoke("write_value", { pollId: poll.id, addr: row.addr, value: Number(val) & 0xffff });
        refresh();
      } catch (e) { footer(String(e), true); }
    }
  );
}

function openValueModal(title, value, cb) {
  const m = $("modal"), inp = $("modalInput");
  $("modalTitle").textContent = title;
  inp.value = value;
  m.hidden = false;
  setTimeout(() => { inp.focus(); inp.select(); }, 0);
  const done = (ok) => {
    m.hidden = true;
    $("modalOk").onclick = null;
    $("modalCancel").onclick = null;
    inp.onkeydown = null;
    if (ok) cb(inp.value);
  };
  $("modalOk").onclick = () => done(true);
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

  const am = $("addrMode");
  if (am) am.onchange = () => { applyAddrMode(am.value); apply(); };

  document.querySelectorAll(".tab").forEach((t) => {
    t.onclick = () => {
      document.querySelectorAll(".tab").forEach((x) => x.classList.remove("active"));
      t.classList.add("active");
      const v = t.dataset.view;
      $("viewTables").hidden = v !== "tables";
      $("viewSchema").hidden = v !== "schema";
      if (v === "schema") renderSchema();
    };
  });
  const addBtn = $("wAdd");
  if (addBtn) addBtn.onclick = addWidget;

  $("cKind").onchange = kindFields;

  $("btnConnCancel").onclick = () => { endConnEdit(); resetConnForm(); };
  $("btnPollCancel").onclick = () => { endPollEdit(); };
  $("btnRuleCancel").onclick = () => { endRuleEdit(); };

  // Смена соединения в форме опроса — Slave ID подтягивается из соединения
  // (можно переопределить вручную).
  $("pConn").onchange = () => {
    const c = cfg.connections.find((x) => x.id === $("pConn").value);
    if (c && c.slave != null) $("pSlave").value = c.slave;
  };

  $("connForm").onsubmit = (e) => {
    e.preventDefault();
    const c = connFromForm();
    c.name = $("cName").value || c.name;
    if (editingConn) {
      // id не меняем — на него ссылаются опросы.
      c.id = editingConn.id;
      Object.assign(editingConn, c);
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

/* --------------------------------------------------------------- helpers -- */
function synthRows(cp) {
  const f = cp.fmt || "u16";
  const step = (f.startsWith("f32") || f === "u32" || f === "i32") ? 2 : 1;
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

function renderSchema() {
  const canvas = $("canvas");
  if (!canvas || drag) return;
  const m = liveMap();
  canvas.innerHTML = "";
  for (const w of (cfg.mimic || [])) {
    const el = document.createElement("div");
    el.className = "widget";
    el.style.left = w.x + "px";
    el.style.top = w.y + "px";
    const row = m[`${w.poll}:${w.addr}`];
    const name = w.label || `${w.type} ${w.addr}`;
    if (w.type === "lamp") {
      let color = "#1f2937";
      if (row) {
        const lvl = row.alarm_level;
        if (lvl === "hihi" || lvl === "hi") color = "#ef4444";
        else if (lvl === "lolo" || lvl === "lo") color = "#3b82f6";
        else if ((row.raw || 0) > 0) color = "#22c55e";
      }
      el.innerHTML = `<div class="lamp" style="background:${color};box-shadow:0 0 10px ${color}"></div><div class="wlabel">${esc(name)}</div>`;
    } else if (w.type === "value") {
      const txt = row ? `${row.display} ${row.unit || ""}` : "—";
      el.innerHTML = `<div class="wval">${esc(txt)}</div><div class="wlabel">${esc(name)}</div>`;
    } else if (w.type === "bar") {
      const min = Number.isFinite(w.min) ? w.min : 0;
      const max = Number.isFinite(w.max) ? w.max : 100;
      const v = row ? Math.max(0, Math.min(100, ((Number(row.value) || 0) - min) / Math.max(max - min, 1e-9) * 100)) : 0;
      el.innerHTML = `<div class="wlabel">${esc(name)} ${row ? esc(row.display) : "—"}</div><div class="bar"><i style="width:${v}%"></i></div>`;
    } else if (w.type === "gauge") {
      const min = Number.isFinite(w.min) ? w.min : 0;
      const max = Number.isFinite(w.max) ? w.max : 100;
      const v = row ? (Number(row.value) || 0) : min;
      el.innerHTML = gaugeSvg(v, min, max, row && row.alarm_level) +
        `<div class="wlabel">${esc(name)}${w.unit ? " · " + esc(w.unit) : ""}</div>`;
    } else if (w.type === "button") {
      const onV = w.onVal != null ? Number(w.onVal) : 1;
      const on = row && (row.raw || 0) === onV;
      el.classList.add("wbutton");
      el.innerHTML = `<div class="mbtn${on ? " on" : ""}">${esc(w.btnLabel || (on ? "ВКЛ" : "ВЫКЛ"))}</div><div class="wlabel">${esc(name)}</div>`;
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
  }
}

function gaugeSvg(v, min, max, level) {
  const span = Math.max(max - min, 1e-9);
  const t = Math.max(0, Math.min(1, ((Number(v) || 0) - min) / span));
  const cx = 90, cy = 82, r = 68;
  const th = Math.PI * (1 - t);
  const x = cx + r * Math.cos(th), y = cy - r * Math.sin(th);
  const large = t > 0.5 ? 1 : 0;
  const col = level === "hihi" || level === "hi" ? "#ef4444"
    : level === "lolo" || level === "lo" ? "#3b82f6" : "#22d3ee";
  const txt = Number.isFinite(Number(v)) ? String(Math.round(Number(v) * 100) / 100) : "—";
  return `<svg width="150" height="86" viewBox="0 0 180 100">
    <path d="M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}" fill="none" stroke="#1e293b" stroke-width="13" stroke-linecap="round"/>
    ${t > 0.004 ? `<path d="M ${cx - r} ${cy} A ${r} ${r} 0 ${large} 1 ${x.toFixed(1)} ${y.toFixed(1)}" fill="none" stroke="${col}" stroke-width="13" stroke-linecap="round"/>` : ""}
    <text x="90" y="96" text-anchor="middle" class="gtxt">${esc(txt)}</text>
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
  if (!drag) return;
  drag.w.x = Math.max(0, drag.ox + (ev.clientX - drag.sx));
  drag.w.y = Math.max(0, drag.oy + (ev.clientY - drag.sy));
  drag.el.style.left = drag.w.x + "px";
  drag.el.style.top = drag.w.y + "px";
});
window.addEventListener("mouseup", () => { if (drag) drag = null; });

boot();
