# 02. Маппинг модулей

Построчная сверка: что откуда куда. Колонка «Сложность» — субъективная оценка
трудозатрат при портировании.

## Сводка

| Файл | Строк | Куда | Сложность |
|---|---:|---|---|
| `src/main.cjs` | 444 | распадается на 6 мест (см. ниже) | средняя |
| `src/renderer.js` | 393 | `window/` + `dialogs/` | **высокая** |
| `src/workspace.cjs` | 152 | `core/workspace/` | низкая |
| `src/files.cjs` | 147 | `core/files/` + `core/git/` | средняя |
| `src/planning.cjs` | 125 | `core/planning/` | низкая |
| `src/style.css` | 112 | ~20 строк CSS для GTK + libadwaita по умолчанию | низкая |
| `src/files-ui.js` | 100 | `dialogs/files.rs` | **высокая** |
| `src/provider.cjs` | 67 | `core/provider/` | средняя |
| `src/index.html` | 62 | `window/` + `dialogs/` (UI-файлы или код) | **высокая** |
| `src/preload.cjs` | 54 | **удаляется** | — |
| `src/telemetry.cjs` | 50 | `core/telemetry/` + `main.rs --hook` | средняя |
| `src/launch.cjs` | 41 | `core/provider/launch.rs` | низкая (минус Windows) |
| `src/git.cjs` | 38 | `core/git/mod.rs` | низкая |
| `src/worktree-setup.cjs` | 37 | `core/worktree.rs` | средняя |
| `src/navigation.js` | 36 | `app.rs` (actions/accels) + `dialogs/palette.rs` | средняя |
| `src/transcripts.cjs` | 34 | `core/provider/transcripts.rs` | средняя |
| `src/history.cjs` | 29 | `core/history.rs` | низкая |
| `src/repository-tools.cjs` | 19 | `core/git/mutate.rs` | низкая |
| `src/accounts.cjs` | 17 | `core/accounts.rs` | низкая |

---

## `main.cjs` — разбор 444 строк

| Строки | Что | Куда |
|---|---|---|
| 1–23 | импорты, `setPath('userData')`, single-instance | `app.rs`; путь → `glib::user_config_dir()` |
| 25–51 | `boot()`: Workspace/History, BrowserWindow, security-хендлеры | `app.rs` + `window/mod.rs`; security-хендлеры **удаляются** |
| 53–57 | `handle()` — валидация отправителя IPC | **удаляется целиком** |
| 58–89 | обработчики workspace / session / settings / spec / task / profile / command | прямые вызовы core из обработчиков сигналов |
| 90–110 | `output()`, `insert()`, review brief, quick commands | `terminal/session.rs` |
| 111–148 | `files:snapshot` / `files:read` / `files:mutate` + подтверждения | `dialogs/files.rs` + `core/files` |
| 149–178 | `project:discover` / `edit` / `reconnect` / `remove`, `profile:remove` | `dialogs/project.rs` |
| 179–196 | `transcripts:scan` / `transcripts:import` | `dialogs/transcripts.rs` |
| 197–204 | `session:recover`, `git:status` | `window/sessions.rs` |
| 205–250 | `makeWorktree()`, `worktree:remove` | `core/worktree.rs` + `dialogs/` |
| **251–330** | **`start(id)`** — самая важная функция | `terminal/mod.rs::start()` |
| 284–300 | `child.onData` — рассылка, tail, детект `codex resume` | `contents-changed` с дебаунсом |
| 301–330 | `child.onExit` — история, статус задачи, activity, уведомление | сигнал `child-exited` |
| 331–366 | `runNext()`, `finishTask()`, `queue:toggle` | `core/planning/queue.rs` + `dialogs/planning.rs` |
| 367–374 | `terminal:write` / `terminal:resize` | **исчезают** — VTE владеет PTY |
| 375–390 | `stop(id)` — killpg SIGHUP → SIGKILL | `terminal/mod.rs::stop()` |
| 391–400 | `usage:read` | `dialogs/usage.rs` + `core/provider/codex.rs` |
| 401–428 | `monitor` — `setInterval(2000)` опрос телеметрии | `monitor.rs`, `glib::timeout_add_seconds_local(2, …)` |
| 429–444 | `win.on('close')`, `render-process-gone`, `window-all-closed` | `window/mod.rs::close_request` |

### `start(id)` — детально (main.cjs:251–330)

Функция, которую нельзя испортить. Последовательность:

1. Уже запущена → выход.
2. Сессия существует, не archived, не worktreeRemoved, не busy.
3. Директория = `session.workingDirectory || project.path`, проверка что каталог.
4. Если `session.taskID` — проверить, что бриф не устарел:
   `task.sessionID === id` и `spec.approvedRevision === spec.revision === task.specRevision`.
5. `accountEnvironment()` → home + env. Если профиль: при resume home должен
   существовать, иначе ошибка; создать каталог.
6. Claude → `telemetry.configuration()` пишет `<hash>.settings.json` с hooks
   и, опционально, statusLine; удаляет старые `.status`/`.usage`.
7. `launchSpec(session, session.started, platform, env, bindings)` → file + args.
8. `pty.spawn(...)` **до** записи в workspace.
9. `workspace.update()`: `started = true`, `agentHome`, задача → `building`.
   Если запись упала — убить процесс и пробросить ошибку.
10. Регистрация в `terminals`, `updateWake()`, запись в activity.
11. Подписки `onData` / `onExit`.

**В Rust:** шаги 1–7 идентичны. Шаг 8 → `vte::Terminal::spawn_async()`, который
асинхронный: pid приходит в колбэк. Значит шаг 9 переезжает **в колбэк**, а
ошибка записи приводит к `killpg` только что созданного процесса.
Это единственное место, где структура кода меняется не механически.

### `onData` (main.cjs:284–300) — три задачи

1. `send('terminal:data')` → **не нужно**, VTE рисует сама.
2. `entry.tail` + сохранение в History раз в 2 секунды → переезжает на
   `contents-changed` с дебаунсом (см. [05-terminal-vte.md](05-terminal-vte.md)).
3. Детект `codex resume <uuid>` в выводе → регулярка по тексту VTE.

### `onExit` (main.cjs:301–330)

Порт один в один на сигнал `child-exited`:
сохранить историю → снять из реестра → обновить статус задачи
(`changes` / `review` / `failed` по правилам) → записать в activity →
`finishTask()` при коде 0 → уведомление, если окно не в фокусе.

---

## `renderer.js` — разбор 393 строк

| Строки | Что | Куда |
|---|---|---|
| 1–31 | импорты, `showError`, `perform()` | `window/mod.rs`: `adw::Toast` вместо `#error` |
| 32–54 | `update()`, `button()`, `theme()`, `applySettings()` | `state.rs` + `adw::StyleManager` |
| 55–73 | `terminal(id)`, `fitActive()` | `terminal/session.rs`; **`fitActive` не нужен** — VTE сама |
| 74–82 | `focusPane`, `openSession` | `window/sessions.rs` |
| **83–129** | **`render()`** — тотальная перерисовка | `sync()` + `refresh_selection()` (см. 01) |
| 130–137 | search, show-archived, open-folder | `gtk::SearchEntry` + `gtk::FilterListModel` |
| 138–154 | новая сессия, форма сессии | `dialogs/session.rs` |
| 155–174 | start / stop / `showText()` | `window/sessions.rs` + `dialogs/text.rs` |
| **175–218** | **`session-menu`** — 13 действий | `gio::SimpleAction` + `gtk::MenuButton` |
| 219–244 | edit / worktree / settings формы | соответствующие диалоги |
| 245–271 | quick commands | `dialogs/commands.rs` |
| 272–281 | подписки `onData` / `onExit` / `onChange` / ResizeObserver | **не нужны** |
| 282–301 | профили аккаунтов, журнал активности | `dialogs/accounts.rs`, `dialogs/activity.rs` |
| 302–370 | `renderPlanning()`, формы спеки и задачи, prepare | `dialogs/planning.rs` |
| 371–393 | Files, настройки проекта, статус агента, очередь, история провайдера | соответствующие диалоги |

**Фильтрация списков.** Сейчас `visible(s)` (renderer.js:88) фильтрует по
запросу и флагу archived на каждом `render()`. В GTK это
`gtk::FilterListModel` + `gtk::CustomFilter`, который перевызывается только при
изменении запроса. Сортировка по pinned — `gtk::SortListModel` + `gtk::CustomSorter`.

---

## `files-ui.js` → `dialogs/files.rs`

Самый плотный UI-файл. Ключевые части:

| Что | Сейчас | Станет |
|---|---|---|
| `generation` — счётчик для отмены устаревших чтений | `let generation = 0` | `Cell<u64>` + проверка после await |
| Вкладки changes/files/log/branches | `data-git-tab` атрибуты | `adw::ViewSwitcher` + `gtk::Stack` |
| Список изменений | `div.file-row` с кнопками | `gtk::ListView` + `adw::ActionRow` с suffix-кнопками |
| Превью текста | `<pre>` | `GtkSourceView` (read-only) + `GtkSourceBuffer` |
| Подсветка диффа | ручной парсинг в split-таблицу | `GtkSourceLanguage "diff"` — **бесплатно** |
| Split diff с номерами строк | `<table class="split-diff">` | два `GtkSourceView` в `gtk::Paned`, `show-line-numbers` |
| Markdown-рендер | ручной парсинг 5000 строк | `pulldown-cmark` → Pango markup в `GtkTextView` |
| Картинки | base64 data URI в `<img>` | `gtk::Picture::for_file` — **проще** |
| Кнопки «Discard hunk N» | генерируются из `@@`-матчей | те же, из `hunks()` в core |
| Черновики сообщений коммита | `drafts: Map` | `HashMap<PathBuf, String>` в диалоге |

**Важно сохранить:** `generation++` при закрытии диалога и при смене вкладки —
это фикс из `docs/port/status.md` («Clearing a file/diff selection invalidates
outstanding asynchronous reads»). В Rust тот же паттерн: токен сравнивается
после `.await`, устаревший результат отбрасывается.

**Важно сохранить:** проверка `$('git-message').value === original` после
генерации сообщения (files-ui.js) — не затирать то, что пользователь напечатал
пока работал агент.

---

## `navigation.js` → `app.rs` + `dialogs/palette.rs`

Шорткаты сейчас — глобальный `keydown` с ручным разбором аккорда.
В GTK это `gio::SimpleAction` + `Application::set_accels_for_action()`.

Нужен конвертер формата: `"mod+shift+p"` → `"<Primary><Shift>p"`.

```
mod   → <Primary>     alt → <Alt>      shift → <Shift>
arrowright → Right    arrowleft → Left     ","  → comma
```

Дефолты сохраняются как есть:

| Действие | Аккорд | GTK action |
|---|---|---|
| palette | `mod+shift+p` | `app.palette` |
| newSession | `mod+n` | `app.new-session` |
| files | `mod+shift+g` | `app.files` |
| next | `mod+alt+arrowright` | `app.next-session` |
| previous | `mod+alt+arrowleft` | `app.previous-session` |
| settings | `mod+,` | `app.settings` |
| search | `mod+k` | `app.search` |

Валидация в `workspace.cjs:validateSettings` требует формат
`^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$` и уникальность значений — **формат хранения
не меняем**, конвертация только на входе в GTK. Это сохраняет совместимость
`workspace.json` с Electron-версией.

---

## Что удаляется полностью

| Код | Строк | Почему |
|---|---:|---|
| `preload.cjs` | 54 | нет границы процессов |
| `handle()` + валидация sender | ~10 | то же |
| CSP / permission / navigate хендлеры | ~6 | нет веб-движка |
| `windowsExecutable()` | 15 | Windows не в скоупе |
| PowerShell-ветка `launchSpec` | 12 | то же |
| `psQuote` + `EncodedCommand` в telemetry | 4 | то же |
| windows-ветка `worktree-setup` | 3 | то же |
| case-insensitive сравнение путей | 1 | то же |
| `ELECTRON_RUN_AS_NODE` (4 места) | 4 | нет Electron |
| `terminal:write` / `terminal:resize` | 8 | VTE владеет PTY |
| `fitActive()` + FitAddon + ResizeObserver | 10 | VTE сама |
| `plain()` / `stripVTControlCharacters` | 3 | VTE отдаёт чистый текст |
| `scripts/build.cjs` (esbuild) | 5 | нет бандлинга |
| **Итого** | **~135** | |
