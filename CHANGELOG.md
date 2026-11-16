# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries are derived from `git log` between consecutive tags (merge commits excluded);
each bullet cites the commit it comes from. Versions and dates are the repository's tags
and commit dates — nothing is back-filled from memory.

## [Unreleased]

Nothing since v1.7.0 — `HEAD` is the v1.7.0 tag.

## [1.7.0] — 2026-11-15

### Added

- **In-app auto-update**: signed Tauri updater, so the app fetches and installs a new
  release itself (`e3d7a41`).

## [1.6.0] — 2026-11-14

### Added

- **OPC UA bridge** (`mb-opc`): the engine publishes its tags over OPC UA, and an OPC UA
  client can write a tag back into Modbus (`FC5`/`FC6`/`FC16`) (`fefb515`, `170c08b`).
- **Poll coalescing**: identical register requests are served from one round trip
  (`fefb515`).
- **Save as Modbus Poll (`.mbp`)** export (`f200bf7`).
- Signed `SHA256SUMS` on release artifacts — cosign keyless, optional GPG (`96d5570`).
- Issue forms for bug reports and feature requests (`6972577`, `5047d09`); questions
  routed to Discussions, security to private advisories (`5099a61`).
- Landing page: OPC UA bridge card, portable build in the download block, roadmap v1.6
  (`b0816ad`).

### Fixed

- OPC UA: writable tags need `user_access_level CURRENT_WRITE` (`ef42828`).
- OPC UA: `data_encoding` must be a null `QualifiedName` for e2e reads (`af2ad21`).
- OPC UA: the bridge namespace is not `ns=2` but the actual index (`ed75d40`).
- OPC UA: `default_endpoint` waits for the endpoint ID, not the URL (`9e3c49f`), and
  discovery failed without it (`538c089`).
- OPC UA: setter uses `parking_lot::Mutex` from `opcua::sync` (`078cf39`); namespace moved
  out of the block, setter unsize coercion (`a69e9ce`).
- OPC UA: imports of service types via `types::service_types`; `build_server_and_space` /
  `pump` restored after the lib+bin split (`6dc006d`, `fa2ee97`).
- OPC UA: API update for `opcua` 0.12 — `parking_lot::RwLock`, `DateTime::now()`,
  `BadCommunicationError`, `&DateTime`, serde dependency (`2a77300`).
- OPC UA: `BadDataUnavailable` for a computed tag that fails (`2df5c93`); ownership and
  ambiguity fixes (`3fa6fc8`, `4b15e35`, `dbd39a0`).

### Changed

- OPC UA e2e test marked `#[ignore]` — the handshake is flaky (`f3ca4c1`); opcua logs and
  `RUST_LOG=debug` in CI (`1b9a7b8`); p1/p2 share scale in the test (`a9259b7`).
- Application certificates hung the first read after connecting — reverted (`9f2d643`).
- Release CI: cosign switched to the new bundle format, old flags are rejected
  (`012f329`); Strawberry Perl and `OPENSSL_SRC_PERL` for vendored OpenSSL on Windows
  (`2b7c441`, `6c263d2`).

## [1.5.2] — 2026-11-06

### Added

- Production-grade **alarm panel** (`4e660e1`).
- NSIS hook so a stale shortcut icon from a previous install does not survive an upgrade
  (`4e660e1`).

## [1.5.1] — 2026-11-06

### Fixed

- **Gauge arc** wrapped the whole circle on values above half scale (`650c01a`).

### Changed

- New application icon: the author's final version (Photoroom), icon set regenerated;
  README and site updated (`eb4a972`, `b5745fb`, `a8fa1d2`).
- SCADA screenshots refreshed — gauge arc bug gone (`b5818c0`).

## [1.5.0] — 2026-11-06

### Added

- **Rules on the SCADA mimic**: lamps, gauges, bars and values coloured by the fired rule
  branch — colour, text and blink, exactly like table rows (`bac513c`).
- **Interactive tour** that highlights real UI elements and auto-advances once a step is
  done, plus a "Demo project" button that raises the slave simulator and polls the app
  itself (`bac513c`).
- i18n validator for the landing page — 83 keys PASS (`8cc3798`).

### Changed

- Landing page download buttons and headers to v1.4.3; promo kit for the v1.4.3 posting;
  social banner (`8cc3798`, `800dac4`, `b123d80`, `bf92686`).

## [1.4.3] — 2026-10-28

### Fixed

- Fixes from v1.4.2 field feedback (`a8d091c`).

### Changed

- Landing page download CTAs to v1.4.2 (`ebc3162`).

## [1.4.2] — 2026-10-28

### Fixed

- The serial RTU slave now actually starts (`cfec55b`).
- RS-485 half-duplex echo is handled (`cfec55b`).

### Changed

- Landing page CTAs to v1.4.1 (`a342415`).

## [1.4.1] — 2026-10-28

### Fixed

- Master writes survive link-down (`8e16935`).
- Polls follow the connection's Slave ID (`8e16935`).

### Changed

- Landing page to v1.4.0 — serial RTU slave card, roadmap shipped (`008227d`).

## [1.4.0] — 2026-10-28

### Added

- **Slave simulator on a physical serial line** — Modbus RTU over COM/RS-485, 9600 8E1 and
  friends; line garbage never wedges the slave (`fb77a49`).
- Full slave simulator guide in Russian, linked from the README (`15073ed`).

### Changed

- Landing page synced to v1.3.0 — slave simulator card, screenshot, guide link
  (`cc926e0`).

## [1.3.0] — 2026-10-28

### Added

- **In-app slave simulator** — the app answers as a Modbus device: editable register grid,
  sine/ramp/noise auto-generation, TCP and RTU-over-TCP (`53f4409`).
- **Typed values** — real/int/uint mixed, float32/64 with all four word orders, 32/64-bit
  ints, hex/bin, typed write modals, per-cell dec → hex → bin cycle (`53f4409`).
- UI performance work (`53f4409`).
- Bilingual GitHub Pages landing in `docs/` (`dfdd117`).

## [1.2.0] — 2026-10-27

### Added

- Floating **parameter watch window** — pin registers and computed tags into a window with
  big live values, alarm colours and session min/max (`9c4e895`).

## [1.1.0] — 2026-10-27

### Added

- **Computed tags** — arithmetic over tags (`{T2 jacket} - {T1 reactor}`, `min`, `pow`)
  evaluated every scan and shown in their own table (`3684522`).
- **Journal** — engine events (RX/WR/ERR/SYS) appended to a log file (`3684522`).
- **CSV historian** — change-only or interval mode, configurable (`3684522`).
- **Diagnostics panel** — per-connection and per-poll counters, reconnect counts
  (`3684522`).
- **Alarm side panel** and SCADA gauge/button widgets (`543ee9d`).
- **RU/EN in one click** — the whole UI flips language from the toolbar, docs screenshots
  refreshed (`e286130`).

### Fixed

- Gauge overflow (`3684522`).

## [1.0.0] — 2026-10-27

### Added

- **Clean-room Modbus Poll reimplementation**: 32 value branches per row with per-branch
  dynamic row text («0 → Закрыт, 1 → Открыт») versus two colour rules in the original
  (`08a18f0`); mbrs-style display formats (`e4c17ce`).
- **Byte-exact `.mbp` round-trip** (`MbpDoc`): `write(parse(bytes)) == bytes`, with a
  public synthetic corpus replacing the private real-file one (`081d416`, `a71b0d6`).
- **Modbus ASCII** framing (`:ADU`-as-hex + LRC + CRLF) and transports; sparkline trends;
  CSV value log (`daa8b55`, `109deb9`).
- **Multi-platform build pipeline and installers** — Windows NSIS `.exe` / `.msi`,
  portable zip; Linux `.AppImage`, `.deb` (`0a110d2`).
- MIT license file, bilingual README (RU/EN) with live-tables and SCADA screenshots,
  mascot icon (`081d416`, `2cea2c5`, `5708c32`).

### Fixed

- Imported polls bind to their own connections, per-connection Slave ID, editable rules
  and connections (`0df382b`).

### Changed

- CI installs `libudev-dev` on Linux (serialport needs `pkg-config` + `libudev`)
  (`df9da4c`).

[Unreleased]: https://github.com/kostyk348/mbscada-rs/compare/v1.7.0...HEAD
[1.7.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.7.0
[1.6.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.6.0
[1.5.2]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.5.2
[1.5.1]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.5.1
[1.5.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.5.0
[1.4.3]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.3
[1.4.2]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.2
[1.4.1]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.1
[1.4.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.0
[1.3.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.3.0
[1.2.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.2.0
[1.1.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.1.0
[1.0.0]: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.0.0
