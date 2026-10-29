# Promo posts EN — mbSCADA v1.4.3

Links to include:
- Release: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.3
- Latest release: https://github.com/kostyk348/mbscada-rs/releases/latest
- Site & guide: https://kostyk348.github.io/mbscada-rs/
- Repo: https://github.com/kostyk348/mbscada-rs

Attach:
- X/Twitter — `banner_x_1200x675_en.png` on tweet 1; `demo.gif` works as a reply or quote.
- Reddit — `banner_github_og_1280x640_en.png` as link preview or upload; GIF in comments.
- GitHub Discussions/README — `banner_github_og_1280x640_en.png` (also fits the repo social preview at 2:1).

---

## X / Twitter thread (4 tweets)

**Tweet 1 (with banner_x_1200x675_en.png or demo.gif):**

Modbus Poll, reimagined — in pure Rust. 🦀

mbSCADA v1.4.3: a clean-room Modbus Poll with a SCADA mode on top — live tables, mimic diagrams, rule-based alarms and a built-in slave simulator. One ~6 MB executable, MIT.

⬇ github.com/kostyk348/mbscada-rs/releases/latest

**Tweet 2:**

Five transports: Modbus TCP (MBAP), RTU over TCP (real CRC16), Serial RTU over a physical COM/RS-485 line (2-wire echo handled), Serial ASCII, ASCII over TCP.

Polls from 20 ms, formats uint16 → float64 with word/byte swaps, scaling, per-cell dec/hex/bin.

**Tweet 3:**

It also answers as a Modbus device: the built-in slave simulator serves registers over TCP or a real serial line, with an editable grid + auto-generation (sine/ramp/noise). Test your master with zero hardware.

And it imports real Modbus Poll .mbp/.mbw files byte-exactly.

**Tweet 4:**

SCADA extras: a 32-branch rule tree per row (colors, alarms, severity), computed tags with expressions, drag&drop mimic diagrams, CSV history + a RX/WR/ERR journal.

Windows / Linux / macOS · MIT · 68+ tests in CI

Releases: github.com/kostyk348/mbscada-rs/releases/latest
Site & guide: kostyk348.github.io/mbscada-rs

---

## Reddit — r/PLC (also fits r/embedded, r/rust)

**Title:**
I built a clean-room Modbus Poll alternative in Rust — master + slave simulator + SCADA visuals in one ~6 MB exe (MIT)

**Body:**

After years of juggling Modbus Poll licenses, random simulator tools and a separate HMI builder, I wrote mbSCADA — a single Tauri/Rust executable that covers the whole loop:

- **Master:** Modbus TCP (MBAP), RTU over TCP (CRC16), Serial RTU on a physical COM/RS-485 line (handles 2-wire echo), Serial ASCII, ASCII over TCP. Poll periods from 20 ms, formats uint16→float64 with byte-order swaps, scaling, and per-cell dec/hex/bin display.
- **Slave simulator:** the app answers as a device — over TCP or a real serial line (silence-based resync, 300 ms). The register grid is editable and has auto-generation, so you can test masters against a "live" device with no hardware.
- **.mbp/.mbw import:** it reads real Modbus Poll files byte-exactly (tables, row aliases, periods, addresses). Imported polls follow the connection's Slave ID, so switching device address re-anchors everything automatically.
- **SCADA layer:** 32-branch rule tree per row for colors/alarms, computed tags with expressions like {T2 jacket} - {T1 reactor}, drag&drop mimic diagrams (lamps, bars, gauges, write buttons), sparkline trends, CSV history + RX/WR/ERR journal.

Everything is MIT-licensed, 68+ tests run in CI, builds are produced for Windows (exe/msi), Linux (deb/rpm/AppImage, x64+arm64) and macOS.

No Modbus Poll sources were read — the .mbp format was reverse-engineered from files.

- Release (v1.4.3): https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.3
- Site & guide: https://kostyk348.github.io/mbscada-rs/
- Source: https://github.com/kostyk348/mbscada-rs

Feedback welcome — especially serial/RS-485 edge cases from people with real converters (the half-duplex echo handling was fun).

---

## GitHub Discussions / short blurb

mbSCADA v1.4.3 is out: imported .mbp/.mbw polls now follow the connection's Slave ID (change it once — every poll follows), slave-simulator start errors show right next to the Start button, and the panel got a proper 1-2-3 start guide (Transport: Modbus TCP / RTU over TCP / Serial RTU). Grab it here: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.3 — full guide: https://kostyk348.github.io/mbscada-rs/
