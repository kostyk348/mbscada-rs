# mbSCADA — Modbus Poll + SCADA

[![CI](https://github.com/kostyk348/mbscada-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/kostyk348/mbscada-rs/actions/workflows/ci.yml)
![tests](https://img.shields.io/badge/tests-51%20passing-brightgreen)
![license](https://img.shields.io/github/license/kostyk348/mbscada-rs)
![lang](https://img.shields.io/badge/rust-1.85%2B-orange)

Чистая (clean-room) реализация **Modbus Poll** на **Rust**: опрос Modbus-устройств, живые таблицы значений, аварии, журнал обмена — плюс **SCADA-режим** (мимик-схемы) и расширенные стилевые правила, которых в оригинале нет.

> Clean-room: формат файлов изучен по байтам реальных `.mbp/.mbw` и документации Modbus Poll; исходники Modbus Poll не использовались.

**🇷🇺 Русский** · [🇬🇧 English (below)](#-english) · [Оглавление: Features](#возможности) / [Features](#features)

![Таблицы, живые значения](docs/img/app.png)

## Возможности

**Опрос (Modbus master, как в Modbus Poll)**
- Транспорты: Modbus/TCP, RTU (COM), RTU-over-TCP; заготовки ASCII/ASCII-over-TCP.
- Функции: 01/02/03/04 (чтение), 05/06/0F/10 (запись), каждый опрос со своим slave/адресом/периодом.
- Каждый опрос — свой «взгляд» на адреса: количество, база 0/1, шкалирование (`×mult ÷div +offset`).

**Форматы отображения (Modbus Poll + расширения)**
- `u16 / i16 / hex16 / bin16 / ascii16 / u16 (swapped)`
- `u32 / i32 / hex32` (слова ABCD), `float32` в 4 вариантах порядка слов: **ABCD / CDAB / BADC / DCBA**
- 64-битные: `u64 / i64 / f64` (ABCD)
- Шкалирование и единицы измерения на опрос.

**Аварии и правила (superset Modbus Poll)**
- Пороговые аварии как в оригинале: hi / hi-hi / lo / lo-lo с подписями.
- Стилевые правила: цвет/мигание, до **32 веток на строку** (в оригинале — 2 цветовых правила), операторы `> ≥ < ≤ = ≠`, в диапазоне / вне диапазона, бит.
- **Динамический текст строки**: ветка подменяет подпись строки по значению (например «0 → Закрыт, 1 → Открыт»).
- Приоритет первой сработавшей ветки (обобщение «Rule 1 старше Rule 2»).

**SCADA (мимик-схемы)** — редактор схем: фигуры, подписи, привязка к живым регистрам (Modbus Poll не имеет SCADA-режима — это наше расширение). Плюс журнал обмена со временем отклика каждого ответа.

![SCADA-редактор и журнал обмена](docs/img/scada.png)

**Совместимость с Modbus Poll**
- Импорт `.mbp` (конфигурация окна) и `.mbw` (workspace, список окон), в том числе по папке.
- Разбор побайтовый: маркер `FF FE FF` + UTF-16LE строки, per-row записи `[alias][PAD][6×color][6×0]`, хвостовые поля (период/slave и пр.) — проверен на реальных файлах Modbus Poll 10.5.
- Формат, множитель, адрес, цвета переносятся в опросы.

**Инженерное**
- Журнал обмена (TX/RX/ошибки/тайминги) и статистика соединений (online, сотни опросов/сек).
- Симулятор ведомого (`mb-sim`) для тестов без железа; интеграционные тесты движка по TCP и RTU-over-TCP.

## Сборка и запуск

```bash
# тесты
cargo test

# приложение (Tauri 2, Windows)
cd apps/mbscada/src-tauri
cargo build --release --target x86_64-pc-windows-gnu
# готовый exe: target/x86_64-pc-windows-gnu/release/mbscada.exe

# демо-ведомый (для проверки без железа)
cargo run -p mb-sim --example demo_server -- 502
```

## Структура

```
crates/
  mb-proto/    PDU/RTU/MBAP: кодирование, CRC, исключения
  mb-format/   парсер/писатель .mbp/.mbw
  mb-engine/   многопоточный движок опроса, форматы, правила, аварии
  mb-sim/      симулятор ведомого + сетевые обёртки
apps/
  mbscada/     Tauri 2 UI (таблицы, схемы SCADA, правила, журнал)
```

## Дорожная карта

- [ ] ASCII-транспорт (заготовки уже в enum)
- [ ] CSV-логирование значений по времени
- [ ] Графики трендов per-row
- [x] Byte-exact `.mbp` round-trip: `mbp_doc` хранит файл сегментами (head / строки / closer) и собирает бит-в-бит; правка alias не портит осталь­ные байты

## Лицензия

MIT

---

## 🇬🇧 English

A clean-room reimplementation of **Modbus Poll** in **Rust**: Modbus polling, live value tables, alarms, traffic log — plus a **SCADA mode** (mimic diagrams) and extended styling rules that the original does not have.

> Clean-room: the file format was studied from real `.mbp/.mbw` bytes and Modbus Poll documentation; no Modbus Poll sources were used.

### Features

**Polling (Modbus master, like Modbus Poll)**
- Transports: Modbus/TCP, RTU (COM), RTU-over-TCP; ASCII / ASCII-over-TCP stubs ready.
- Functions: 01/02/03/04 (read), 05/06/0F/10 (write); each poll has its own slave/address/period.
- Every poll is an independent view of the address space: count, 0/1 base, scaling (`×mult ÷div +offset`).

**Display formats (Modbus Poll + extensions)**
- `u16 / i16 / hex16 / bin16 / ascii16 / u16 (swapped)`
- `u32 / i32 / hex32` (ABCD words), `float32` in 4 word orders: **ABCD / CDAB / BADC / DCBA**
- 64-bit: `u64 / i64 / f64` (ABCD)
- Per-poll scaling and engineering units.

**Alarms & rules (superset of Modbus Poll)**
- Threshold alarms like the original: hi / hi-hi / lo / lo-lo with labels.
- Styling rules: color/blink, up to **32 branches per row** (the original has only 2 color rules); operators `> ≥ < ≤ = ≠`, in-range / out-of-range, bit test.
- **Dynamic row text**: a branch can replace the row label by value (e.g. «0 → Closed, 1 → Open»).
- First matching branch wins (generalizing “Rule 1 before Rule 2”).

**SCADA (mimic diagrams)** — a diagram editor: shapes, labels, bindings to live registers (Modbus Poll has no SCADA mode — that's our extension). Plus a traffic log with per-response turnaround time.

**Modbus Poll compatibility**
- Import of `.mbp` (single window config) and `.mbw` (workspace with window list), including whole-folder import.
- Byte-accurate parsing: `FF FE FF` markers + UTF-16LE strings, per-row records `[alias][PAD][6×color][6×0]`, trailing fields (period/slave etc.) — verified against real Modbus Poll 10.5 files.
- Format, multiplier, address and colors are carried over into polls.

**Engineering**
- Traffic log (TX/RX/errors/timings) and per-connection stats (online state, polls per second).
- Slave simulator (`mb-sim`) for testing without hardware; TCP and RTU-over-TCP integration tests.

### Build & run

```bash
# tests
cargo test

# app (Tauri 2, Windows)
cd apps/mbscada/src-tauri
cargo build --release --target x86_64-pc-windows-gnu
# executable: target/x86_64-pc-windows-gnu/release/mbscada.exe

# demo slave (no hardware needed)
cargo run -p mb-sim --example demo_server -- 502
```

### Structure

```
crates/
  mb-proto/    PDU/RTU/MBAP: encoding, CRC, exceptions
  mb-format/   .mbp/.mbw parser/writer
  mb-engine/   multithreaded polling engine, formats, rules, alarms
  mb-sim/      slave simulator + network wrappers
apps/
  mbscada/     Tauri 2 UI (tables, SCADA diagrams, rules, log)
```

### Roadmap

- [x] Byte-exact `.mbp` round-trip: `mbp_doc` keeps the file as segments (head / rows / closer) and reassembles it bit-for-bit; editing an alias leaves all other bytes untouched
- [ ] ASCII transport (enum stubs already in place)
- [ ] Time-series CSV logging
- [ ] Per-row trend charts
- [ ] Byte-exact `.mbp` writer (currently canonical semantic round-trip)

### License

MIT
