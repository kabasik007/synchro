> **Follow-up:** Synchro v0.4.2 mitigates multiple P1 findings below. See [the current security release](RELEASE-0.4.2.md). This original audit remains valid for its v0.4.1 baseline; Rust and production tests are still needed.

# Synchro v0.4.1 — незалежний аудит багів і безпеки

**Дата:** 2026-10-19  
**Перевірено:** головну гілку `main`, коміт `ad346b0162829294b4fc1cb20101e94105f5424c`  
**Компоненти:** `server/src/{api,main,setup,websocket}.rs`, `web/`, `deploy/`, aaPanel / Nginx, SQLite.  
**Тип перевірки:** ручний static code review + звірення окремих механізмів з офіційними документаціями Nginx/Axum та OWASP + локальний ізольований тест поведінки `cp -r` із symlink. **Не було:** реальної Rust-компіляції, penetration test у публічній мережі, запуску на aaPanel або end-to-end тесту APK на телефоні.

## Короткий висновок

Проєкт має добру базу для single-owner MVP: loopback-only Rust API, окремі 256-бітні токени пристроїв, хешування токенів у SQLite, авторизація recipient ID, UUID для серверних файлів, SHA-256 після складання блоків, WebSocket-токен у першому повідомленні (не в URL), приватні директорії та CSP без inline JavaScript.

**Статус: НЕ готовий для відкритого production deployment.** До запуску на публічному домені рекомендовано закрити всі P1, а потім повторно провести cargo test / integration tests.

## P1 — критичні / високі

### SEC-01 — Повторний root-bootstrap потенційно записує довільний файл через symlink [P1, підтверджено]

**Місце:** [deploy/bootstrap-aapanel.sh:24–35,68–74](https://github.com/kabasik007/synchro/blob/main/deploy/bootstrap-aapanel.sh#L24-L35).

Скрипт запускається від root. Він перевіряє лише `index.html`, а потім виконує `cp -r "$REPO/web/." "$WEBROOT/"`. Якщо зловмисник уже має можливість запису до папки вебсайту (компрометований користувач `www`, помилка прав, webshell), він може підмінити будь-який цільовий `.js/.css` файл символічним посиланням на інший системний файл. Root-run `cp -r` може перезаписати ціль symlink. **Поведінку GNU cp підтверджено ізольованим локальним тестом** (ціль symlink була перезаписана).

Це **локальна ескалація за наявності попереднього доступу на запис до webroot**, а не віддалена атака на чистий сервер без авторизації.

**Виправлення:** ніколи не копіювати від root напряму у writable webroot без суворої перевірки/ізоляції. Готувати release у новій root-owned staging-директорії, перевірити `lstat` усіх цілей, копіювати під контрольованим користувачем, атомарно міняти реліз і забороняти symlink-перехід. Забезпечити rollback і захист від перезапису чужого сайту.

### SEC-02 — Гонка `put_chunk` ↔ `finish_transfer` [P1, підтверджена логікою]

**Місце:** [server/src/api.rs:257–308](https://github.com/kabasik007/synchro/blob/main/server/src/api.rs#L257-L308), [311–395](https://github.com/kabasik007/synchro/blob/main/server/src/api.rs#L311-L395).

`put_chunk` зчитує статус `uploading` **до** асинхронного читання тіла HTTP, створення каталогу та запису. Тим часом `finish_transfer` може перевести запис у `finalizing` і почати читати файли. Отже, останній блок може бути записаний після початку складання або під час очищення chunk-директорії. Це призводить до 409, непередбачуваного retry або неправильного стану.

**Виправлення:** узгодження станів під одним per-transfer lock/lease; врахування inflight chunks; відмова від нових PUT після атомарного переходу; фіналізація лише після підтвердження всіх блоків. Тест паралельних PUT+finish.

### SEC-03 — Гонка `prune_expired` ↔ фіналізація й читання [P1, підтверджена логікою]

**Місце:** [server/src/main.rs:206–249](https://github.com/kabasik007/synchro/blob/main/server/src/main.rs#L206-L249), [server/src/api.rs:311–395](https://github.com/kabasik007/synchro/blob/main/server/src/api.rs#L311-L395).

Прибирання отримує список ID, відпускає DB lock і видаляє файли/директорії. Паралельно `finish_transfer` може створювати blob і переходити у `ready`; `prune` не повторно перевіряє чинний стан/час під час фактичного видалення. Файли можуть бути видалені між збіркою й публікацією, створюючи непогоджені стани або відповіді про успіх без файла.

**Виправлення:** lease з `expires_at` і `lease_until`, стан `deleting`, транзакція захоплення права на cleanup, відмова від видалення активної фіналізації, окрема фонова job із гарантією retry.

### SEC-04 — Недостатня протидія ресурсним DoS [P1, підтверджено конфігурацією]

**Місце:** [server/src/websocket.rs:20–80](https://github.com/kabasik007/synchro/blob/main/server/src/websocket.rs#L20-L80), [server/src/api.rs:311–397](https://github.com/kabasik007/synchro/blob/main/server/src/api.rs#L311-L397), [deploy/nginx-synchro.conf](https://github.com/kabasik007/synchro/blob/main/deploy/nginx-synchro.conf).

Є ліміт **трьох** паралельних 8 MiB `PUT` і загальний ліміт розміру HTTP body — це плюс. Але відсутні `limit_req`, `limit_conn` і жорсткий ліміт одночасних WebSocket-з'єднань/перевірок першого повідомлення. `finish_transfer` може одночасно збирати кілька великих файлів, а download — відкривати великі потоки. Малий VPS можна перевантажити запитами й I/O навіть без порушення size limit.

**Виправлення:** Nginx і Rust rate limiting, загальний semaphore на finalize/streams, ліміти WebSocket за IP та device, загальні deadlines, коректні 429/503, обмеження швидкості і тест навантаження.

### SEC-05 — Plaintext файли на VPS; відсутнє E2EE [P1 / архітектурний ризик конфіденційності]

**Місце:** [server/src/api.rs:367–395](https://github.com/kabasik007/synchro/blob/main/server/src/api.rs#L367-L395) та `deploy/`.

HTTPS захищає канал браузер↔Nginx, але сервер зберігає оригінальні байти. Адміністратор VPS, інший процес із доступом до даних або скомпрометований сервер може читати файли. Cloudflare (якщо HTTP proxy) також є TLS-термінатором. **Це не криптографічна вразливість реалізації TLS, а відсутня можливість продукту**; не називати систему end-to-end encrypted.

**Виправлення:** клієнтське AEAD-шифрування (наприклад XChaCha20-Poly1305), ключі пристроїв, human-confirmed pairing, integrity метаданих, key rotation; сервер зберігає лише ciphertext. До впровадження — лише не конфіденційні файли.

### SEC-06 — ACK не доводить фактичне завантаження; можливе видалення недоставленого файла [P1 / цілісність доставки]

**Місце:** [server/src/api.rs:488–522](https://github.com/kabasik007/synchro/blob/main/server/src/api.rs#L488-L522) і `web/app.js`.

`inbox` показує відправнику/одержувачу очікуваний SHA-256; endpoint `ack` порівнює присланий хеш зі **значенням зі сховища метаданих**, не перевіряє, чи одержувач фактично скачав/зберіг файл. Кнопка **«✓ Доставлено»** доступна одразу в кожному рядку, навіть до `downloadFile()`. Після ACK сервер змінює статус і одразу видаляє blob.

Отже, одержувач або клієнт із його скомпрометованим токеном може необоротно видалити недоставлений файл. **ACK тут є заявою клієнта, а не підтвердженням фактичного приймання.**

**Виправлення:** в UI дозволяти ACK лише після успішної контрольної суми й підтвердження збереження, а на сервері вести окрему delivery receipt/session. Залишити grace window/повторне завантаження до підтвердження або timeout; відрізняти `downloaded` від `acknowledged`.

## P2 — середні

### SEC-07 — Немає перевірки WebSocket Origin [P2]

**Місце:** [server/src/websocket.rs:20–27](https://github.com/kabasik007/synchro/blob/main/server/src/websocket.rs#L20-L27). WebSocket handshake не перевіряє `Origin`; це спрощує cross-site виклики й ресурсний abuse. **Однак токен передається в першому повідомленні, а не через cookie, тому сама по собі відсутність Origin не доводить викрадення чужої сесії.** Вимагати allowlist публічного HTTPS origin, rate limits; підтримати контролювану зміну домену.

### SEC-08 — Немає ідемпотентної фіналізації й відновлення після втрати HTTP response [P2]

**Місце:** `finish_transfer` у `server/src/api.rs:311–365`. Успішна збірка може тривати довше за таймаут Cloudflare/Nginx; клієнт повторить `/finish`, але статус уже `ready` і отримає 409. Перенести в background job із GET /transfers/{id} і можливістю повторити `/finish` без побічних ефектів.

### SEC-09 — Nginx download-buffering може збільшувати використання диска [P2]

**Місце:** [deploy/nginx-synchro.conf:11–25](https://github.com/kabasik007/synchro/blob/main/deploy/nginx-synchro.conf#L11-L25). Немає явної політики `proxy_buffering`. За стандартних налаштувань Nginx може буферизувати великі відповіді у тимчасові файли. На невеликому VPS це додатковий непорахований простір. Для authenticated downloads розглянути `proxy_buffering off` плюс ліміти паралельних потоків.

### SEC-10 — Відсутні версійні міграції SQLite [P2]

**Місце:** `server/src/main.rs:164–203`. Є лише `CREATE TABLE IF NOT EXISTS` та глобальне скидання `finalizing -> uploading` при старті. Нові колонки/індекси не мігрують стару схему. Використати `PRAGMA user_version`, транзакційні migrations, резервну копію перед оновленням, restore-test.

### SEC-11 — Немає гарантованого backup/restore [P2]

SQLite використовує WAL. Копіювання тільки `.sqlite3` під час роботи може втратити дані з WAL. Потрібен SQLite Online Backup/VACUUM INTO, узгоджений backup файлових blob/chunk метаданих, перевірка відновлення.

### SEC-12 — Передача браузером до 64 MiB повністю буферизується у пам'яті [P2]

**Місце:** `web/app.js` — `sha256(file.arrayBuffer())`, `response.arrayBuffer()` плюс Blob. На слабких телефонах може викликати OOM або закриття вкладки. Використати інкрементальний streaming SHA-256 та downloads в потік / нативний файл.

### SEC-13 — Orphan-папки та тимчасові blob після аварійних завершень [P2]

**Місце:** `server/src/api.rs:286–308,367–395`, `main.rs:206–249`. Для активних записів cleanup є, але `.partial` й `.tmp` після process kill залишаються до expiry, а для деяких станів після невдалої транзакції можуть потребувати додаткового sweep. Додати запуск reconciliation і метрики orphan bytes.

### SEC-14 — Публічна статична директорія не має явної заборони прихованих/резервних файлів [P2]

**Місце:** `deploy/nginx-synchro.conf:50–57`. `try_files $uri` обслуговує файли, що випадково опинилися у webroot. Внести deny для `.git`, `.env`, архівів backup, SQLite, `*.bak`, `*.sql` та `disable_symlinks` або окремі контрольовані правила, погоджені з aaPanel.

### SEC-15 — Коректність реального місця: TOCTOU між перевіркою й записом [P2]

`fs2::available_space` і quota перевіряються до створення transfer. Інші aaPanel сайти й системні логи можуть паралельно зайняти диск. Не достатньо одного preflight: потрібні жорсткі записи з ENOSPC handling, reservation ledger і monitor free space під час фіналізації.

### SEC-16 — Перший installer commit і втрата одноразового response [P2]

**Місце:** `server/src/setup.rs:163–197`. SQLite transaction генерує і записує device token hashes, але після commit при мережевому обриві raw tokens вже неможливо відновити, а `installed=1` блокує wizard. Відновлення через admin endpoint зі створенням замінних токенів треба явно показати на сторінці locked із інструкцією, але не відновлювати raw secrets з БД.

### SEC-17 — Імена файлів не нормалізовані від Unicode spoofing [P2]

`safe_filename` відкидає slash, backslash та control, але не Unicode bidi override, misleading extension/невидимі символи. Для APK/EXE в UI це особливо важливо. Нормалізувати ім'я, рендерити тип із реально визначеного MIME/signature, не довіряти одному розширенню.

## P3 — UX/операції

- `/health` віддає `ok` незалежно від деградації диска/SQLite вже після startup. Розділити `live` і `ready`.
- Немає автоматичного CAPTCHA/anti-brute (для 256-бітних токенів brute-force непрактичний, але rate limiting все одно потрібен проти resource DoS).
- Вебмайстер не автоматизує aaPanel site creation/HTTPS: це **навмисне** безпечне обмеження; bootstrap і конфіг треба встановити з shell, потім користуватись `/install/`.
- Немає повноцінної багатокористувацької ACL: зареєстрований пристрій може адресувати файли будь-якому іншому активному пристрою на цьому **single-owner** сервері. Перед public multi-user релізом обов'язкова модель власників/пар/дозволів.
- `web/app.js`: після завершення завантаження браузер може не підтвердити фактичне збереження файла на пристрої; наразі UI показує лише SHA-256 та просить користувача натиснути ACK.

## Рекомендований план

1. **0.4.2 Security Hotfix:** SEC-01, SEC-02, SEC-03, SEC-04, SEC-06. Без цього не відкривати публічне завантаження.
2. **0.4.3 Deployment & Reliability:** SEC-08…SEC-16; rate-limits, migrations, прунер, downloads, installer recovery, backup/restore, nginx -t.
3. **0.5.0 Secure Internet Mode:** E2EE, криптографічне pairing, ACL, ротація ключів, інкрементальні chunks/хеш, Bunny S3 адаптер.
4. Після стабілізації — підключити до Rust relay нативні Windows/Android без втрати LAN/Tailscale.

## Тести приймання перед VPS

| Сценарій | Критерій |
| --- | --- |
| Сайтовий користувач має symlink у webroot; root повторно встановлює | Жодного перезапису поза контрольованим release dir |
| PUT останнього chunk одночасно з /finish | Атомарне завершення: або очікування chunk, або контрольована помилка без втрати |
| /finish одночасно з expired prune | Готовий файл не оголошений доступним, якщо фактично видалений |
| 1000 анонімних WS connections | Клієнти обмежені й швидко відхиляються; RAM/FD стабільні |
| Клієнт натискає ACK до скачування | Система не видаляє єдину копію без явно завершеної операції |
| HTTP timeout після успішного /finish | Повторний запит і GET /transfers повертають існуючий результат |
| SQLite/WAL backup та restore після завантажень | Transfer metadata і blob-и узгоджені |
| Диск VPS переповнений стороннім сайтом | Коректна відмова 507/503, без crash та неврахованих файлів |
| Domain A -> Domain B | Перенесення DNS/TLS/Nginx без нового setup і втрати токенів |
| `cargo test`, `cargo clippy`, `cargo build --release` | Успішна компіляція до запуску |
| `nginx -t` на aaPanel vhost + 2 браузери з різних мереж | Працюють TLS, API, WS, передача та SHA-256 |

## Джерела

- Вихідний код: https://github.com/kabasik007/synchro/tree/main
- OWASP File Upload Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html
- Nginx proxy buffering / request buffering: https://nginx.org/en/docs/http/ngx_http_proxy_module.html
- Axum request body limits: https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html

**Застереження:** цей документ — audit findings, не набір уже реалізованих виправлень. Код перевірений статично. Немає доказів реального remote exploit або успішної Rust-компіляції в цьому сеансі.
