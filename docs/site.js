/* mbSCADA site — i18n + small interactions */
"use strict";

var DICT = {
  en: {
    "doc.title": "mbSCADA — Modbus SCADA in pure Rust",

    "nav.features": "Features",
    "nav.screens": "Screenshots",
    "nav.arch": "Architecture",
    "nav.download": "Download",
    "nav.start": "Build",

    "hero.live": "All local — no cloud, no telemetry",
    "hero.title": "Modbus Poll, reimagined — in pure Rust.",
    "hero.sub": "A clean-room Modbus SCADA: poll TCP & RTU devices into live tables, import real Modbus Poll .mbp/.mbw files byte-exactly, style every row with a 32-branch rule tree, build operator mimics and emulate slave devices — in a single ~6 MB executable.",
    "hero.cta.dl": "⬇ Download v1.4.1",
    "hero.cta.gh": "★ View on GitHub",

    "stat1": "single executable",
    "stat2": "platforms per release",
    "stat3": "tests passing",
    "stat4": "cloud calls. Ever.",

    "sec.f.title": "Everything a poller forgot to be",
    "sec.f.lead": "mbSCADA starts where classic Modbus pollers stop: real alarm handling, an expression engine, a historian and a floating watch window — while staying a small, fast, local tool.",

    "f1.t": "Live polling",
    "f1.d": "Functions 01/02/03/04 and writes 05/06/0F/10 over Modbus TCP, RTU, ASCII and ASCII-over-TCP. Per-poll scaling, engineering units, word order (u16_swapped) and a live traffic log with per-response turnaround.",
    "f2.t": "Byte-exact .mbp import",
    "f2.d": "Import real Modbus Poll .mbp/.mbw files by folder — and the round-trip layer proves write(parse(bytes)) == bytes on real v10.5 files. Your configs survive, your colleagues keep their tooling.",
    "f3.t": "32-branch rule tree",
    "f3.d": "Every row gets a first-match rule tree — not two color settings. Per-branch row colors and dynamic row text: “0 → CLOSED, 1 → OPEN”, fault codes spelled out in words, right in the table.",
    "f4.t": "Alarm side panel",
    "f4.d": "hi / hi-hi / lo / lo-lo alarms surface as cards with timestamps: acknowledge one or all, browse history with durations, enable sound, click to jump to the exact table row.",
    "f5.t": "SCADA mimic widgets",
    "f5.d": "Lay tags on a grid: a semicircular SVG gauge whose arc is painted by alarm levels, a toggle-write button that writes on/off values to the device, and min/max bars. Configure everything with a double-click.",
    "f6.t": "Parameter watch window <span class=\"tag\">v1.2</span>",
    "f6.d": "Pin the registers you actually watch — or computed tags — into a floating window: big live values painted by your alarm rules, quality, session min/max to catch spikes, custom captions, jump-to-row.",
    "f7.t": "Computed tags <span class=\"tag\">v1.1</span>",
    "f7.d": "Arithmetic on your tags evaluated every scan: {T2 jacket} − {T1 reactor}, min/max/pow/avg… A safe built-in expression DSL — no eval, no reflection, syntax errors caught in the editor.",
    "f8.t": "Journal, archive, diagnostics <span class=\"tag\">v1.1</span>",
    "f8.d": "Engine events (RX/WR/ERR/SYS) append to a log file; a CSV historian records values change-only or on a fixed interval and resumes on restart; diagnostics show ok/err, timeouts, reconnects and last-success age.",
    "f9.t": "RU / EN out of the box",
    "f9.d": "One click in the toolbar flips the entire UI between Russian and English. Documentation ships in both languages, and this site speaks both too.",
    "f10.t": "Slave simulator <span class=\"tag\">v1.3</span>",
    "f10.d": "A virtual Modbus device inside the app: TCP & RTU-over-TCP on any port, an editable register map you type values into, mixed data types (real / int / uint / hex) side by side, sine / saw / noise autogeneration, right-click dec → hex → bin per cell — and the map lives in the project file.",
    "f11.t": "Serial RTU slave <span class=\"tag\">v1.4</span>",
    "f11.d": "The same virtual device, but on a real wire: pick a COM / ttyUSB port behind a USB-RS485 adapter, set baud / parity / stop bits and let any hardware master poll it — MBAP, RTU-over-TCP and now physical RTU, with automatic resync when the line picks up garbage.",

    "sec.s.title": "A 30-second look",
    "sec.s.lead": "Real screenshots from the demo project — tables, mimic and the watch window, exactly as they ship.",
    "s1.t": "Live tables",
    "s1.d": "colored alarm rows, per-row sparklines and the alarm side panel",
    "s2.t": "SCADA mimic",
    "s2.d": "gauge painted by alarm levels, toggle-write button, min/max bars",
    "s3.t": "Watch window",
    "s3.d": "pinned registers and computed tags with session min/max",
    "s4.t": "Slave simulator",
    "s4.d": "a virtual device with an editable register map — master and slave on one screen",

    "sec.a.title": "Four crates, no monolith",
    "sec.a.lead": "The workspace splits cleanly: protocol, file formats, engine and the test simulator — the Tauri app is a thin shell on top.",
    "a.proto": "Modbus frames: TCP (MBAP), RTU with CRC-16, ASCII / ASCII-over-TCP, protocol errors and timeouts.",
    "a.format": "Parsing and byte-exact generation of Modbus Poll .mbp/.mbw plus the internal project format.",
    "a.engine": "One thread per connection, scan scheduling, alarm rules, computed-tag evaluator, diagnostics and the CSV historian.",
    "a.sim": "Virtual Modbus slaves that hang, corrupt CRC and disconnect — 72 tests pass without a single physical device.",
    "a.app": "The application: Tauri 2 + vanilla JS — no frameworks, no bundler, instant start, ~6 MB binary.",

    "sec.d.title": "Download v1.4.1",
    "sec.d.lead": "Prebuilt installers for every platform — no Rust toolchain needed. Built automatically by GitHub Actions from a v* tag: five runners, one commit, all platforms.",
    "dl.win.t": "Windows 10/11 x64",
    "dl.win.d": "NSIS installer .exe · .msi — WebView2 is preinstalled on Win10/11.",
    "dl.linux.t": "Linux x86_64 & aarch64",
    "dl.linux.d": ".AppImage (runs anywhere, bundles its own WebKit) · .deb",
    "dl.mac.t": "macOS",
    "dl.mac.d": ".dmg for Apple Silicon and Intel.",
    "dl.note": "Every release is reproducible from a tag — check the Actions tab to watch a build live.",
    "dl.all": "⬇ All platforms on Releases",

    "sec.q.title": "Build from source",
    "sec.q.lead": "One command, standard toolchain — the workspace builds with stable Rust 1.85+.",
    "q.copy": "Copy",
    "q.note": "Or grab a prebuilt installer from Releases if you don’t want a toolchain — the app is self-contained.",
    "q.guide": "📖 Slave simulator guide (RU)",
    "q.guide2": "— a full walkthrough: run the slave, connect masters, type values from both sides.",

    "sec.r.title": "Roadmap",
    "sec.r.lead": "Shipped so far and what’s next — open an issue if something matters to you.",
    "r0": "v1.0–v1.4: live tables, rule tree, alarm panel, mimics, computed tags, CSV archive, watch window, slave simulator with typed values, serial RTU slave on COM/RS-485",
    "r1b": "Alarm history export with filters",
    "r2": "Trend plots for computed tags",
    "r3": "More mimic widgets and archive scheduling",
    "r4": "Your feature here — PRs welcome",

    "ft.tag": "mbSCADA — Modbus Poll, reimagined. MIT License.",
    "ft.repo": "Repository",
    "ft.rel": "Releases",
    "ft.iss": "Issues",
    "ft.made": "Made with Rust, Tauri 2 and one stubborn mascot."
  },

  ru: {
    "doc.title": "mbSCADA — Modbus SCADA на чистом Rust",

    "nav.features": "Возможности",
    "nav.screens": "Скриншоты",
    "nav.arch": "Архитектура",
    "nav.download": "Скачать",
    "nav.start": "Сборка",

    "hero.live": "Всё локально — без облака и телеметрии",
    "hero.title": "Modbus Poll, переосмысленный — на чистом Rust.",
    "hero.sub": "Clean-room SCADA для Modbus: опрос устройств по TCP и RTU в живые таблицы, byte-exact импорт настоящих файлов Modbus Poll (.mbp/.mbw), дерево правил на 32 ветки, операторская мимика и встроенный симулятор slave-устройств — в одном бинарнике ~6 МБ.",
    "hero.cta.dl": "⬇ Скачать v1.4.1",
    "hero.cta.gh": "★ Смотреть на GitHub",

    "stat1": "исполняемый файл",
    "stat2": "платформ в релизе",
    "stat3": "проходящих тестов",
    "stat4": "облачных вызовов. Никогда.",

    "sec.f.title": "Всё то, о чём забыл классический опросчик",
    "sec.f.lead": "mbSCADA начинается там, где обычные Modbus-опросчики заканчиваются: настоящие аварии с квитированием, движок выражений, историк значений и плавающее окно параметров — и при этом маленький, быстрый, локальный инструмент.",

    "f1.t": "Живой опрос",
    "f1.d": "Функции 01/02/03/04 и записи 05/06/0F/10 через Modbus TCP, RTU, ASCII и ASCII-over-TCP. Скейлинг и единицы по опросу, порядок слов (u16_swapped), живой журнал обмена с временем ответа.",
    "f2.t": "Byte-exact импорт .mbp",
    "f2.d": "Импорт настоящих файлов Modbus Poll (.mbp/.mbw) папкой — слой раундтрипа доказывает write(parse(bytes)) == bytes на реальных файлах v10.5. Конфигурации выживают, коллеги остаются при своём инструменте.",
    "f3.t": "Дерево правил на 32 ветки",
    "f3.d": "Каждая строка — ветвящееся дерево «первый совпавший», а не две цветовые настройки. Цвета и динамический текст строки: «0 → ЗАКРЫТ, 1 → ОТКРЫТ», расшифровка кодов ошибок прямо в таблице.",
    "f4.t": "Панель аварий",
    "f4.d": "Уровни hi / hi-hi / lo / lo-lo выносятся карточками со временем: квитирование по одной или всем, история с длительностями просадок, звук, клик — переход к точной строке таблицы.",
    "f5.t": "SCADA-мимика",
    "f5.d": "Тэги на сетке: полукруглый SVG-гейдж, дуга которого красится аварийными уровнями, кнопка-тумблер с записью on/off в устройство, бары min/max. Всё настраивается двойным кликом.",
    "f6.t": "Окно параметров <span class=\"tag\">v1.2</span>",
    "f6.d": "Закрепите регистры, которые реально смотрите, — или вычисляемые тэги — в плавающем окне: крупные живые значения в цветах ваших правил, качество, min/max за сессию для ловли иголок, свои подписи, переход к строке.",
    "f7.t": "Вычисляемые тэги <span class=\"tag\">v1.1</span>",
    "f7.d": "Арифметика над тэгами на каждом скане: {T2 рубашка} − {T1 реактор}, min/max/pow/avg… Безопасный встроенный DSL — ни eval, ни рефлексии, ошибки синтаксиса ловятся в редакторе.",
    "f8.t": "Журнал, архив, диагностика <span class=\"tag\">v1.1</span>",
    "f8.d": "События движка (RX/WR/ERR/SYS) дописываются в лог-файл; CSV-историк пишет значения по изменениям или по периоду и возобновляется после перезапуска; диагностика — ok/err, таймауты, реконнекты, давность успеха.",
    "f9.t": "RU / EN из коробки",
    "f9.d": "Один клик в тулбаре переключает весь интерфейс между русским и английским. Документация на обоих языках — и этот сайт тоже говорит на обоих.",
    "f10.t": "Симулятор slave <span class=\"tag\">v1.3</span>",
    "f10.d": "Виртуальное Modbus-устройство внутри приложения: TCP и RTU-over-TCP на любом порту, редактируемая карта регистров с вводом значений, разнородные типы (real / int / uint / hex) вперемешку, автогенерация синус / пила / шум, ПКМ по ячейке — dec → hex → bin, а карта сохраняется в файле проекта.",
    "f11.t": "Serial RTU-слейв <span class=\"tag\">v1.4</span>",
    "f11.d": "Тот же виртуальный прибор, но на настоящем проводе: выберите COM / ttyUSB порт за USB-RS485 адаптером, задайте скорость / чётность / стоп-биты — и опрашивайте его любым «железным» мастером. MBAP, RTU-over-TCP и теперь физический RTU с автопросыпанием после мусора в линии.",

    "sec.s.title": "Полминуты взгляда",
    "sec.s.lead": "Настоящие скриншоты из демо-проекта — таблицы, мимика и окно параметров ровно в том виде, в каком поставляются.",
    "s1.t": "Живые таблицы",
    "s1.d": "цветные строки аварий, спарклайны по строкам и боковая панель аварий",
    "s2.t": "SCADA-мимика",
    "s2.d": "гейдж в цветах аварий, кнопка-тумблер, бары min/max",
    "s3.t": "Окно параметров",
    "s3.d": "закреплённые регистры и вычисляемые тэги с min/max за сессию",
    "s4.t": "Симулятор slave",
    "s4.d": "виртуальное устройство с редактируемой картой регистров — мастер и слейв на одном экране",

    "sec.a.title": "Четыре крейта, никаких монолитов",
    "sec.a.lead": "Рабочая область делится чисто: протокол, форматы файлов, движок и тестовый симулятор — приложение Tauri лишь тонкая оболочка сверху.",
    "a.proto": "Кадры Modbus: TCP (MBAP), RTU с CRC-16, ASCII / ASCII-over-TCP, ошибки протокола и таймауты.",
    "a.format": "Разбор и byte-exact генерация файлов Modbus Poll .mbp/.mbw плюс внутренний формат проекта.",
    "a.engine": "Поток на соединение, планировщик сканов, правила аварий, вычислитель выражений, диагностика и CSV-историк.",
    "a.sim": "Виртуальные слейвы, которые зависают, портят CRC и отваливаются — 72 теста проходят без единого физического устройства.",
    "a.app": "Приложение: Tauri 2 + ванильный JS — без фреймворков и бандлеров, мгновенный старт, бинарник ~6 МБ.",

    "sec.d.title": "Скачать v1.4.1",
    "sec.d.lead": "Готовые установщики под все платформы — тулчейн не нужен. Каждый релиз собирается автоматически GitHub Actions из тега v*: пять раннеров, один коммит, все платформы.",
    "dl.win.t": "Windows 10/11 x64",
    "dl.win.d": "Установщик NSIS .exe · .msi — WebView2 уже предустановлен в Win10/11.",
    "dl.linux.t": "Linux x86_64 и aarch64",
    "dl.linux.d": ".AppImage (запускается везде, несёт свой WebKit) · .deb",
    "dl.mac.t": "macOS",
    "dl.mac.d": ".dmg для Apple Silicon и Intel.",
    "dl.note": "Каждый релиз воспроизводим из тега — загляните во вкладку Actions, чтобы посмотреть сборку вживую.",
    "dl.all": "⬇ Все платформы на странице релизов",

    "sec.q.title": "Сборка из исходников",
    "sec.q.lead": "Одна команда, стандартный тулчейн — workspace собирается стабильным Rust 1.85+.",
    "q.copy": "Копировать",
    "q.note": "Либо возьмите готовый установщик из релизов — приложение полностью автономно.",
    "q.guide": "📖 Гайд по slave-симулятору",
    "q.guide2": "— полный разбор: запуск, подключение мастеров, ввод значений с обеих сторон.",

    "sec.r.title": "Планы",
    "sec.r.lead": "Что уже сделано и что дальше — если что-то из этого важно вам, откройте issue.",
    "r0": "v1.0–v1.4: живые таблицы, дерево правил, панель аварий, мимика, вычисляемые тэги, CSV-архив, окно параметров, симулятор slave с типизированными значениями, serial RTU-слейв на COM/RS-485",
    "r1b": "Экспорт истории аварий с фильтрами",
    "r2": "Графики трендов по вычисляемым тэгам",
    "r3": "Больше виджетов мимики и расписания архивирования",
    "r4": "Ваша фича здесь — PR приветствуются",

    "ft.tag": "mbSCADA — Modbus Poll, переосмысленный. Лицензия MIT.",
    "ft.repo": "Репозиторий",
    "ft.rel": "Релизы",
    "ft.iss": "Issues",
    "ft.made": "Сделано на Rust, Tauri 2 и одном упрямом маскоте."
  }
};

(function () {
  function apply(lang) {
    var d = DICT[lang] || DICT.en;
    document.title = d["doc.title"];
    document.documentElement.lang = lang;
    var nodes = document.querySelectorAll("[data-i18n]");
    for (var i = 0; i < nodes.length; i++) {
      var el = nodes[i];
      var v = d[el.getAttribute("data-i18n")];
      if (v != null) el.innerHTML = v;
    }
    var btns = document.querySelectorAll(".lang button");
    for (var j = 0; j < btns.length; j++) {
      btns[j].classList.toggle("on", btns[j].getAttribute("data-lang") === lang);
    }
    try { localStorage.setItem("mb-site-lang", lang); } catch (e) {}
  }

  var saved = null;
  try { saved = localStorage.getItem("mb-site-lang"); } catch (e) {}
  var start = saved || ((navigator.language || "en").toLowerCase().indexOf("ru") === 0 ? "ru" : "en");
  apply(start);

  var btns = document.querySelectorAll(".lang button");
  for (var k = 0; k < btns.length; k++) {
    btns[k].addEventListener("click", function () { apply(this.getAttribute("data-lang")); });
  }

  var copy = document.getElementById("copy");
  if (copy) {
    copy.addEventListener("click", function () {
      var text = document.getElementById("code").innerText;
      var done = function () {
        var old = copy.textContent;
        copy.textContent = "✓";
        setTimeout(function () { copy.textContent = DICT[localStorage.getItem("mb-site-lang") || "en"]["q.copy"] || old; }, 1400);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, done);
      } else {
        var ta = document.createElement("textarea");
        ta.value = text;
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand("copy"); } catch (e) {}
        document.body.removeChild(ta);
        done();
      }
    });
  }
})();
