# 06. Интеграции

## Телеметрия Claude через hooks

Сейчас работает так:

1. `telemetry.configuration()` пишет `<userData>/telemetry/<sha256(id)>.settings.json`
   с восемью hooks (`SessionStart`, `UserPromptSubmit`, `PreToolUse`,
   `PostToolUse`, `PermissionRequest`, `Notification`, `Stop`, `SessionEnd`),
   каждый — exec-form команда с таймаутом 5 с.
2. Команда хука — сам Electron в режиме Node:
   `{ command: process.execPath, args: [telemetry.cjs, output], timeout: 5 }`
   плюс `ELECTRON_RUN_AS_NODE=1`.
3. Хелпер читает JSON из stdin, `sanitize()` его, пишет `<output>.status`
   или `<output>.usage` атомарно.
4. Приложение раз в 2 секунды читает эти файлы (`monitor` в `main.cjs`).

**В Rust становится чище.** Никаких `ELECTRON_RUN_AS_NODE` и `asarUnpack`:
хелпером работает сам бинарник.

```json
{ "type": "command", "command": "/usr/bin/convoy", "args": ["--hook", "<output>"], "timeout": 5 }
```

`main.rs` перехватывает `--hook` **до** `gtk::init()` и `adw::Application::new()`:

```rust
fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--hook") {
        return convoy_core::telemetry::hook::run(&args[pos + 1]);
    }
    // обычный запуск GUI
}
```

Путь к бинарнику брать из `std::env::current_exe()`.

**Status line** (`settings.claudeUsage`) — тот же бинарник, но команда
строкой, а не exec-form, потому что Claude исполняет её через шелл:

```
'/usr/bin/convoy' --hook '<output>'
```

Квотирование — тот же `quote()` из `launch.cjs` (одинарные кавычки,
`'` → `'"'"'`). Windows-ветка с `psQuote` и `EncodedCommand` удаляется.

**Инвариант 27 сохраняется целиком:** `sanitize()` валидирует
`used_percentage ∈ [0,100]`, `resets_at` в будущем, а `configuration()`
удаляет старые `.status` и `.usage` перед каждым запуском.

`sanitize()` портируется один в один вместе со своей таблицей состояний:

```
SessionStart      → idle
UserPromptSubmit  → working
PreToolUse        → working, но AskUserQuestion → waiting
PostToolUse       → working
PermissionRequest → waiting
Notification      → idle_prompt ? done : waiting
Stop              → done
SessionEnd        → ended
```

## Опрос телеметрии

`setInterval(..., 2000)` → `glib::timeout_add_seconds_local(2, ...)`.
Логика `monitor` (`main.cjs:401–428`) переносится как есть:

- читать `.status` для каждого живого терминала;
- игнорировать значения старше 30 минут и не новее последнего;
- смена на `done` / `waiting` → запись в activity + уведомление;
- `working` + задача в `review` → вернуть задачу в `building`;
- `done` + задача в `building` → `review` + `finishTask()`;
- гибернация: `done` + простой больше `hibernateMinutes` → `stop()`.

Опрос файлов можно заменить на `gio::FileMonitor`, но в первой версии
оставить таймер: он проще, покрыт существующим поведением и стоит копейки.

## Codex usage

`codexLimits()` — единственная асинхронная многошаговая интеграция.
Запускает `codex app-server --listen stdio://`, обменивается JSON-RPC:

```
→ {"id":1,"method":"initialize","params":{"clientInfo":{"name":"convoy","version":"0.1.0"}}}
← {"id":1,...}
→ {"method":"initialized"}
→ {"id":2,"method":"account/rateLimits/read"}
← {"id":2,"result":{...}}
```

Ограничения: таймаут 25 с, лимит ответа 2 МБ, stderr игнорируется,
процесс убивается в любом исходе.

В Rust — `gio::Subprocess` с `stdin_pipe` / `stdout_pipe`, построчное чтение
через `gio::DataInputStream::read_line_async`, таймаут через
`glib::timeout_add_seconds_local_once`.

`rateWindows()` портируется один в один, включая отбрасывание окон с
истёкшим `resetsAt` и значениями вне [0, 100].

**Инвариант 28:** никаких других методов, никаких модельных запросов.

## Уведомления

`Notification` из Electron → `gio::Notification` + `Application::send_notification()`.

```rust
let n = gio::Notification::new("Agent finished a turn");
n.set_body(Some(&session.title));
n.set_default_action_and_target_value("app.focus-session", Some(&session.id.to_variant()));
app.send_notification(Some(&session.id), &n);
```

Клик по уведомлению активирует `app.focus-session` — прямой аналог
`notification.on('click')` → `send('session:focus', id)`.

Условия показа не меняются: только если `settings.notifications`, окно не в
фокусе, и событие не является результатом явной остановки пользователем.

Для доставки нужен `.desktop`-файл с тем же application id
(`com.iftech.convoy.linux`) — иначе уведомления не покажутся. См.
[09-packaging.md](09-packaging.md).

## Keep awake

`powerSaveBlocker.start('prevent-app-suspension')` → `gtk::Application::inhibit()`:

```rust
let cookie = app.inhibit(
    Some(&window),
    gtk::ApplicationInhibitFlags::IDLE | gtk::ApplicationInhibitFlags::SUSPEND,
    Some("Agent sessions are running"),
);
// снятие: app.uninhibit(cookie)
```

Логика `updateWake()` не меняется: `always` — всегда; `sessions` — пока есть
хотя бы один живой терминал; `off` — никогда.

Работает через XDG portal или напрямую через session manager, в зависимости
от окружения. В README при релизе сохранить оговорку: предотвращается
простой, а не явное усыпление или закрытие крышки.

## Диалоги выбора файлов

`dialog.showOpenDialog({ properties: ['openDirectory'] })` →
`gtk::FileDialog::select_folder_future()`.
`dialog.showSaveDialog(...)` (экспорт спеки) → `gtk::FileDialog::save_future()`
с фильтром `*.md`.

## Корзина

`shell.trashItem(path)` → `gio::File::trash_future()`.
Проверки из инварианта 24 остаются в core, вызов — в UI.

## Открыть папку в файловом менеджере

Сейчас этого в Electron-превью нет (есть в нативной версии: «Show in Finder»).
Добавить через `gtk::FileLauncher::open_containing_folder` — дешёвый паритет
с macOS-версией.

## Single instance

`app.requestSingleInstanceLock()` + `second-instance` →
`gio::ApplicationFlags::default()` и сигнал `activate`: GApplication делает это
из коробки через D-Bus. Второй запуск просто поднимает существующее окно.

## Провайдерские транскрипты

`transcripts.cjs:scan()` портируется как есть:

- Claude: `<home>/projects/<путь-с-заменой-не-буквенно-цифровых-на-дефис>/*.jsonl`,
  id — имя файла;
- Codex: `<home>/sessions/**/*.jsonl`, id из `session_meta.payload.id`,
  с проверкой `cwd` и отбрасыванием субагентов (`source.subagent`,
  `parent_thread_id`).

Ограничения обхода (3000 узлов, глубина 5, 3000 файлов, первые 96 000 байт
файла, 100 результатов) сохраняются.

→ тест «provider history matches folders and excludes Codex subagents»

## Генерация сообщения коммита

`repository-tools.cjs:generateMessage()` — вызывает
`claude --print --tools '' -- '<промпт с диффом>'`, таймаут 60 с,
лимит диффа 100 000 символов, ответ обрезается до 10 000.

Портируется как есть, минус `ELECTRON_RUN_AS_NODE`. Запускается в
`gio::spawn_blocking`. Сохранить проверку из `files-ui.js`: не затирать
поле, если пользователь успел в него напечатать.

## Создание PR

`gh pr create --fill` с `GH_PROMPT_DISABLED=1`, `GIT_TERMINAL_PROMPT=0`,
таймаут 60 с. Без изменений.
