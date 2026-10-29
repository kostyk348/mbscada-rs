# mbSCADA promo kit (v1.4.3)

Готовый набор для постинга. Всё в тёмном техно-стиле приложения; маскот — официальный.

## Что где постить

| Материал | Файл | Куда |
|---|---|---|
| GIF-демо 17.6 с (EN UI): заставка → соединения/опросы → live-значения → запись FC06 → slave по RS-485 → импорт .mbp | `demo.gif` | Telegram (анимацией), VK, X (reply/quote), Reddit (комментарий), Habr (в текст) |
| GitHub OG / соц-превью 1280×640 (EN) | `banner_github_og_1280x640_en.png` | Settings → Social preview репозитория, Open Graph, Reddit |
| X/Twitter карточка 1200×675 (EN) | `banner_x_1200x675_en.png` | X тред, LinkedIn |
| Habr обложка 1200×600 (RU) | `banner_habr_1200x600_ru.png` | Habr |
| Квадрат 1080×1080 (RU) | `banner_square_1080x1080_ru.png` | Telegram/VK пост |

## Тексты

- `posts_RU.md` — Telegram/VK (короткий) + Habr (развёрнутый анонс).
- `posts_EN.md` — X-тред (4 твита) + Reddit r/PLC + короткий блёрб для GitHub Discussions.

## Ссылки (проверены)

- Релиз v1.4.3: https://github.com/kostyk348/mbscada-rs/releases/tag/v1.4.3
- Последний релиз: https://github.com/kostyk348/mbscada-rs/releases/latest
- Сайт и гайд: https://kostyk348.github.io/mbscada-rs/
- Репозиторий: https://github.com/kostyk348/mbscada-rs

## Как пересобрать (скрипты)

- `scripts/demo/shoot.py` — кадры + GIF (Playwright + ffmpeg, детерминированный таймлайн `demo.js`).
- `scripts/demo/shoot_banners.py` — 4 баннера из `banner.html`.
