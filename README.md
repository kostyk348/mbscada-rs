<div align="center">

# 📡 mbSCADA

### Modbus Poll, reimagined — in pure Rust.

**A clean-room Modbus Poll reimplementation with a SCADA mode on top.**
Poll Modbus/TCP & RTU devices into live value tables, import real Modbus Poll `.mbp/.mbw` files byte-exactly,
style any row from a 32-branch rule tree and build mimic diagrams —
all in a ~6 MB single executable. No Modbus Poll sources were read, ever.

[![CI](https://github.com/kostyk348/mbscada-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/kostyk348/mbscada-rs/actions/workflows/ci.yml)
[![Tests](https://img.shields.io/badge/tests-57%20passing-22c55e)](https://github.com/kostyk348/mbscada-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust&logoColor=black)](https://rustup.rs/)
[![Tauri](https://img.shields.io/badge/Tauri‑2-FFC131?logo=tauri&logoColor=black)](https://v2.tauri.app/)
[![Byte‑exact .mbp](https://img.shields.io/badge/.mbp-byte%E2%80%91exact%20round‑trip-0ea5e9)](#-modbus-poll-compatibility)
[![ASCII](https://img.shields.io/badge/modbus‑TCP·RTU·ASCII-success)](#%EF%B8%8F-feature-tour)

**English** · [Русский](README.ru.md)

<img src="docs/img/app.png" alt="mbSCADA — live poll tables with quality badges and per-row sparklines" width="100%">

</div>

---

## 🧭 TL;DR

- 🖥 **Modbus Poll, but better.** Polls 01/02/03/04, writes 05/06/0F/10, per-poll slave / base-0·1 address / scaling / engineering units — and every connection gets a live traffic log with per-response turnaround.
- 🤝 **Drop-in for your existing configs.** Real Modbus Poll `.mbp` / `.mbw` files import by folder — and a byte-exact round-trip layer (`write(parse(bytes)) == bytes`, proven on real 10.5 files) keeps them intact.
- 🎨 **32 styled branches per row.** The original gives you two colour rules; here every row gets a first-match rule tree with per-branch dynamic row text («0 → Закрыт, 1 → Открыт»).
- 📈 **Trends, alarms, CSV.** Per-row sparklines, hi/hi‑hi/lo/lo‑lo alarms, blink styles — and a one-click CSV value log.
- ⚡ **Fast & local.** A multithreaded engine (`mb-engine`) drives all connections concurrently; no telemetry, no cloud, nothing leaves the machine.

<details open>
<summary><b>👀 30-second tour</b></summary>

<br>

| | |
|---|---|
| **Live tables** | <img src="docs/img/app.png" width="100%"> |
| **SCADA + traffic log** | <img src="docs/img/scada.png" width="100%"> |

</details>

## ✨ Feature tour

### 📊 Live tables

<img src="docs/img/app.png" alt="Live tables: imported KAT2 poll flowing at 1000 ms with quality badges" width="100%">

- Every poll is an independent **view of the address space**: own function, own register window, own scaling `×mult ÷div +offset`, own 0/1 address base.
- Values render in **Modbus-Poll-style formats**: `u16 / i16 / hex16 / bin16 / ascii16 / u16(swapped)` · `u32 / i32 / hex32` · `float32` in word orders **ABCD / CDAB / BADC / DCBA** · 64-bit `u64 / i64 / f64`.
- **Per-row sparklines** keep the last 40 samples as inline SVG — the last buffer change blinks.
- Click a row to write a value (FC 05/06 for bits, FC 16 for word arrays).

### 🚦 Rules & alarms

- Threshold alarms exactly like Modbus Poll: **hi / hi‑hi / lo / lo‑lo** with messages.
- Styling rules with **up to 32 branches per row** and operators `> ≥ < ≤ = ≠`, in-range / out-of-range, **bit test**.
- First matching branch wins — the classic «Rule 1 over Rule 2» becomes a full priority list.
- **Dynamic row text** per branch: the row label is replaced when a branch fires (state machines live in the table header).

### 🗺 SCADA mode (mimic diagrams)

| | |
|---|---|
| <img src="docs/img/scada.png" width="100%"> | Editor with shapes and lamp widgets bound to live registers… plus the **traffic log** on the right: every TX/RX, every error and per-response turnaround |

Modbus Poll has no SCADA mode. That is the point of mbSCADA.

### 🤝 Modbus Poll compatibility

- **Import** of `.mbp` (single window) and `.mbw` (workspace with the window list) — one by one or by folder. Tested on a real Modbus Poll 10.5 corpus (7 files, RU aliases with UTF-16 markers).
- **Byte-exact round-trip** (`mbp_doc`): the file is segmented (`head` / row records / closing marker) and reassembled **bit-for-bit**; editing an alias touches nothing else. `write(parse(bytes)) == bytes` is asserted by CI on the whole corpus.
- Format, multiplier, address base, aliases and colours carry over into polls; connections are reconstructed from file IPs.

<details>
<summary><b>📐 Real corpus layout (reverse-engineered)</b></summary>

```text
[head]                     magic v4=7000 (v5=10010), IP marker (v5), [144][func][addr][qty]
[row record] × qty         marker FF FE FF + len + UTF-16LE alias → PAD 55555555
                           + 6× colour u32 (FF000000 AVG) + zeros
[last row + settings]      the last row's gap holds the settings blob & tail
                           [period][slave][1][0][0][…]
[closing marker]           final empty marker FF FE FF 00 + trailer bytes
```

</details>

## 🚀 Getting started

### Build from source

**Requirements:** [Rust](https://rustup.rs/) 1.85+, Windows (WebView2 is preinstalled on Win10/11), or Linux with `libudev-dev pkg-config libwebkit2gtk-4.1-dev` for GUI work.

```bash
git clone https://github.com/kostyk348/mbscada-rs.git
cd mbscada-rs
cargo test                # 57 engine tests

# demo slave (no hardware needed): serves registers on 127.0.0.1:502
cargo run -p mb-sim --example demo_server -- 502

# the app (Tauri 2)
cd apps/mbscada/src-tauri
cargo build --release --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/mbscada.exe
```

Launch `mbscada.exe`, «Импорт папки…» your `.mbp` folder and hit **Старт** — real Modbus Poll windows appear as polls and start flowing.

## 🏗 Architecture

```text
┌────────────────────────────────────────────────────────────────────┐
│                    Tauri 2 desktop shell (Rust)                    │
│  src-tauri: dialogs, project JSON, CSV append, engine restarts     │
├────────────────────────────────────────────────────────────────────┤
│  UI (vanilla JS)                                                   │
│  tables · rule editor (32 branches) · SCADA editor · traffic log   │
├────────────────────────────────────────────────────────────────────┤
│  crates/                                                           │
│  mb-proto    PDU encode/decode · MBAP · RTU+CRC · ASCII ':'+LRC     │
│  mb-format   .mbp/.mbw parser → lossless MbpDoc → semantic Table    │
│  mb-engine   multithreaded poller: TCP · RTU-serial · RtuOverTcp    │
│              · Ascii-serial · AsciiOverTcp, rules, alarms, trends  │
│  mb-sim      slave simulator (+ demo_server example)               │
└────────────────────────────────────────────────────────────────────┘
```

## 🗺 Roadmap

- ✅ Modbus/TCP, RTU, RTU-over-TCP transports with per-connection traffic log (v0.1)
- ✅ Import `.mbp`/`.mbw` by file and by folder (v0.1)
- ✅ 32-branch styled rules + dynamic row text — UI and engine (v0.2)
- ✅ Display formats through 64-bit: hex32 / ascii16 / u16‑swapped / u64 / i64 / f64 (v0.3)
- ✅ Byte-exact `.mbp` round-trip on a real corpus (v0.4)
- ✅ Modbus ASCII framing + ASCII serial & over-TCP transports (v0.4)
- ✅ Per-row sparklines + CSV value log (v0.4)
- ⬜ `.mbp` writer in the UI — «Save as Modbus Poll» (round-trip layer is already engine-side)
- ⬜ Per-row trend window (bigger than a sparkline)
- ⬜ macOS / Linux release builds

## ⁇ FAQ

**Is this a Modbus Poll rip-off?**
No. This is a **clean-room** implementation: the wire protocol is public spec, and the `.mbp` layout was learned from byte-level analysis of files produced by Modbus Poll 10.5 — no proprietary source code was touched.

**What "byte-exact round-trip" means for me?**
Your Modbus Poll files go in, everything else comes back identical. mbSCADA can edit aliases without regenerating the whole file, so unedited bytes stay exactly as Modbus Poll wrote them.

**Does it talk to my devices today?**
TCP / RTU / RTU-over-TCP are production paths with integration tests. ASCII framing and its serial / over-TCP transports are implemented and unit-tested; wire-side integration testing on real devices is on the roadmap.

**Why Rust?**
One executable, no runtime, memory-safe poller — and the whole engine is testable headlessly (see `cargo test`).

## 🤝 Contributing

Issues and PRs welcome. The engine is test-first: `crates/mb-engine`, `crates/mb-format` / `crates/mb-proto` all have unit suites covering Tauri-free paths — start there.

<div align="center">

**mbSCADA** — Modbus Poll for people who also want SCADA.

[Build it](#-getting-started) · [Roadmap](#-roadmap) · [License (MIT)](LICENSE)

</div>
