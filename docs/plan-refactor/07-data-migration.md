# 07. Данные и совместимость

## Решение: формат не меняется

`workspace.json` схемы 3 остаётся байт-в-байт совместимым с Electron-версией.
Причины:

1. Можно поставить порт рядом со старой версией и **сразу работать на реальных
   данных**, а не на тестовых.
2. Откат бесплатный: если порт окажется хуже, запускается старый AppImage.
3. Существующие тесты миграции переносятся без переписывания фикстур.

**Ограничение:** нельзя запускать обе версии одновременно на одном файле.
То же предупреждение, что сейчас в README desktop про Swift и Electron.

## Расположение

```
${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/
├── workspace.json              состояние
├── TerminalHistory/            <sha256(session.id)>.txt — выжимки вывода
├── telemetry/                  <sha256(session.id)>.settings.json
│                               <sha256(session.id)>.status
│                               <sha256(session.id)>.usage
├── accounts/<sha256(profile.id)>/   изолированные CLAUDE_CONFIG_DIR / CODEX_HOME
└── worktrees/<uuid>/           worktree'ы, созданные приложением
```

В Electron это `app.getPath('appData')` + имя. В Rust —
`glib::user_config_dir()` или `directories::ProjectDirs`. **Важно:** имя каталога
должно остаться ровно `Convoy Desktop Preview`, иначе совместимость теряется.

`glib::user_config_dir()` возвращает `$XDG_CONFIG_HOME` или `~/.config` — это то
же, что даёт Electron на Linux. Совпадение проверить на первом же запуске.

## Модель данных

Полный набор типов для `core/workspace/model.rs`. Помечено `?` — опциональное.

```rust
struct State {
    schema_version: u8,               // всегда 3 при записи
    projects: Vec<Project>,
    sessions: Vec<Session>,
    settings: Settings,
    quick_commands: Vec<QuickCommand>,
    specs: Vec<Spec>,
    tasks: Vec<Task>,
    profiles: Vec<Profile>,
    activity: Vec<ActivityEvent>,
    #[serde(flatten)]
    unknown: serde_json::Map<String, Value>,   // ← критично, см. ниже
}

struct Project {
    id: String, title: String, path: PathBuf,
    group: Option<String>, color: Option<String>, icon: Option<String>,
    setup_command: Option<String>, shared_paths: Option<String>,
    review_template: Option<String>,
}

struct Session {
    id: String, project_id: String,
    agent: Agent,                     // claude | codex
    title: String, prompt: String, provider_id: String,
    started: bool, model: Option<String>,
    notes: Option<String>, branch: Option<String>,
    archived: Option<bool>, pinned: Option<bool>,
    working_directory: Option<PathBuf>,
    agent_home: Option<PathBuf>,
    owns_worktree: Option<bool>, worktree_removed: Option<bool>,
    review_of: Option<String>, task_id: Option<String>,
    profile_id: Option<String>,
}

struct Spec { id, project_id, title, problem, requirements, acceptance,
              constraints, plan, revision: u64, approved_revision: Option<u64> }

struct Task { id, project_id, title, details, findings, agent: Agent,
              status: TaskStatus, mode: PublishMode, auto_review: bool,
              spec_id: Option<String>, session_id: Option<String>,
              spec_revision: Option<u64>, last_error: Option<String> }

struct Profile { id, label, agent }
struct QuickCommand { id, title, text, submit: bool, project_id: Option<String> }
struct ActivityEvent { id, at: String, kind: ActivityKind, session_id, title, detail }

enum TaskStatus { Queued, Building, Review, Changes, Done, Failed }
enum PublishMode { None, Pr, Push }
enum ActivityKind { Started, Resumed, Exited, Done, Waiting, Hibernated, Worktree }
```

### ⚠ Сохранение неизвестных полей

Инвариант 4 требует, чтобы миграция была аддитивной и не теряла неизвестные
поля. В JS это получается само (`{ ...state }`). В serde — **нет**: по умолчанию
неизвестные поля молча отбрасываются.

Решение: `#[serde(flatten)] unknown: Map<String, Value>` на каждой структуре,
где это важно (`State`, `Project`, `Session`, `Task`, `Spec`). Иначе старая
Electron-версия потеряет данные после того, как файл перепишет порт.

Это **не** декоративная деталь: пока обе версии существуют параллельно, они
будут читать и писать один формат по очереди.

## Валидация

`validate()` + `validateSettings()` + `validatePlanning()` — 80 строк JS,
которые надо перенести полностью, а не «на глазок». Проверки:

**Общие**
- `schemaVersion ∈ {1,2,3}`, `projects` и `sessions` — массивы;
- уникальность всех id в объединённом пространстве
  projects + sessions + specs + tasks + profiles + activity;
- `project.path` — абсолютный.

**Сессии**
- `agent ∈ {claude, codex}`, `providerID` — строка, `prompt` — строка,
  `started` — bool, `projectID` существует;
- `providerID` непустой → должен быть UUID v4-подобным
  (`^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$`, регистронезависимо);
- Claude → `providerID` обязателен;
- `notes` / `branch` — строки, `archived` / `pinned` — bool;
- `workingDirectory` / `agentHome` — абсолютные пути;
- `reviewOf` указывает на другую сессию того же проекта.

**Настройки**
- `fontSize ∈ [10,24]`, `scrollback ∈ [1000,50000]` — целые;
- `theme ∈ {dark,light,system}`, `defaultAgent ∈ {claude,codex}`;
- `keepAwake ∈ {off,always,sessions}`, `hibernateMinutes ∈ [0,1440]`;
- `shortcuts` — только 7 разрешённых ключей, формат
  `^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$`, **значения уникальны**.

**Быстрые команды**
- уникальные id, непустые title (≤200) и text (≤32000), `submit` — bool,
  `projectID` (если есть) существует.

**Планирование**
- спека: 6 текстовых полей, title ≤200 и непустой, остальные ≤16000;
  `revision ≥ 1`; `approvedRevision`, если задан, равен `revision`;
- задача: `status` из списка, `agent` валиден, `specID` того же проекта,
  `sessionID` указывает на сессию с обратной ссылкой `taskID`;
- профиль: label ≤100 непустой, agent валиден;
- сессия с `profileID` → профиль существует и того же агента;
- activity: ≤200 записей, `kind` из списка, `at` — парсимая дата,
  `sessionID` существует.

Все сообщения об ошибках сохранить дословно: они уже выверены и попадают
пользователю на глаза.

## Тестовые фикстуры

Существующие тесты работают с синтетическими состояниями. Портировать вместе
с логикой:

- схема 1 с неизвестными полями → после загрузки они на месте;
- битый JSON → файл не тронут;
- `schemaVersion: 99` → отказ, файл не тронут;
- задача в `building` → после загрузки `failed` + `lastError`;
- невалидная мутация → ни память, ни диск не изменились;
- сбой записи → память не изменилась.

Последний случай в Rust проверяется подменой каталога на недоступный для
записи или через трейт-обёртку над записью.
