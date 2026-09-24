# 01. Целевая архитектура

## Размещение в репозитории

```
apps/
  desktop/          Electron-превью — НЕ ТРОГАЕМ. Остаётся для Windows.
  linux/            новое
    Cargo.toml      workspace
    crates/
      convoy-core/  вся логика, без единой зависимости от GTK
      convoy-gtk/   UI + бинарник
    data/           .desktop, иконки, AppStream metainfo
    packaging/      PKGBUILD, debian/, rpm spec
```

`apps/desktop/**` завязан на CI (`.github/workflows/desktop.yml` фильтрует по
этому пути), поэтому новый код селится рядом и получает свой workflow.

## Почему два крейта

`convoy-core` не зависит от `gtk4`, `libadwaita` и `vte4`. Причины:

1. **Тестируемость.** 29 из 36 существующих тестов — чисто логические. Они
   должны гонятся через `cargo test` без дисплея и без GTK.
2. **Перспектива.** Сейчас логика продублирована между Swift (`Sources/`,
   9112 строк) и Node (1100 строк). Если ядро не завязано на тулкит, SwiftUI-версия
   со временем сможет ходить в него через C-ABI вместо третьей реализации.
   Это не задача текущего порта, но архитектурный выбор делается сейчас.
3. **Дисциплина.** Граница крейтов не даёт логике расползтись по обработчикам
   сигналов — главная болезнь GTK-приложений.

## Модули `convoy-core`

```
convoy-core/src/
  lib.rs
  workspace/
    mod.rs          Workspace: загрузка, атомарная запись, update()
    model.rs        Project, Session, Spec, Task, Profile, ActivityEvent, Settings
    validate.rs     полный порт validate() + validateSettings() + validatePlanning()
    migrate.rs      схемы 1/2 → 3, аддитивно
  git/
    mod.rs          git(), gitStatus(), createWorktree()
    status.rs       parseStatus() — разбор --porcelain=v1 -z
    diff.rs         hunks(), digest()
    mutate.rs       stage/unstage/commit/discard/fetch/pull/push/switch/revert/reset
  files/
    mod.rs          discover(), folderFiles(), snapshot()
    preview.rs      preview(), fileContent() — bounded, realpath-проверки
    paths.rs        relative() — защита от traversal
  provider/
    mod.rs          providerSpec(), sessionSpec(), agentArgs()
    launch.rs       launchSpec(), agentEnvironment()
    codex.rs        codexLimits() — app-server JSON-RPC по stdio
    transcripts.rs  scan() — чтение .jsonl Claude/Codex
  planning/
    mod.rs          saveSpec/approveSpec/saveTask/setTaskStatus/prepareTask
    markdown.rs     markdown() — экспорт спеки
  accounts.rs       accountEnvironment()
  history.rs        History — bounded хранение вывода, reviewBrief()
  telemetry/
    mod.rs          configuration(), read()
    hook.rs         sanitize() + точка входа хук-хелпера
  worktree.rs       setup() — shared paths + setup command
  process.rs        трейт ProcessRunner (см. ниже)
  error.rs          ConvoyError
```

## Модули `convoy-gtk`

```
convoy-gtk/src/
  main.rs           точка входа; ветка --hook перехватывается ДО gtk::init
  app.rs            adw::Application, actions, accels, single-instance
  state.rs          AppState: Rc<RefCell<...>> + gio::ListStore модели
  objects/          GObject-обёртки для ListView
    project.rs      ProjectObject
    session.rs      SessionObject
    task.rs         TaskObject
    spec.rs         SpecObject
    activity.rs     ActivityObject
  window/
    mod.rs          главное окно, NavigationSplitView
    sidebar.rs      дерево проектов + поиск
    sessions.rs     список сессий, тулбар сессии
    terminals.rs    VTE-панели, split
  terminal/
    mod.rs          TerminalManager: запуск, остановка, реестр
    session.rs      TerminalSession: VTE + pty + child pid + tail
  dialogs/
    session.rs      новая сессия / review handoff
    edit.rs         редактирование сессии
    settings.rs     AdwPreferencesDialog
    project.rs      настройки проекта
    files.rs        Files & Changes
    planning.rs     Specs & tasks
    accounts.rs     профили аккаунтов
    commands.rs     быстрые команды
    palette.rs      командная палитра
    shortcuts.rs    редактор шорткатов
    activity.rs     журнал
    history.rs      сохранённый вывод
    usage.rs        лимиты
    transcripts.rs  история провайдера
    text.rs         универсальный текстовый диалог (feedback / saved message)
  runner.rs         GioRunner — реализация ProcessRunner
  notify.rs         gio::Notification
  power.rs          gtk::Application::inhibit
  monitor.rs        периодический опрос телеметрии (аналог setInterval 2000мс)
```

## Конкурентность: без второго рантайма

GTK крутит свой main loop на `glib::MainContext`. Тащить туда tokio — лишний
рантайм и лишний источник багов.

**Решение:** `convoy-core` синхронный. Все внешние вызовы идут через трейт:

```rust
pub trait ProcessRunner: Send + Sync {
    fn run(&self, spec: &ProcessSpec) -> Result<Output, ConvoyError>;
}
```

- `StdRunner` — `std::process::Command`, используется в тестах и в хук-хелпере.
- `GioRunner` — то же, но вызывается из `gio::spawn_blocking`, результат
  возвращается в UI через `async_channel` + `glib::spawn_future_local`.

Типовой вызов из UI:

```rust
let (tx, rx) = async_channel::bounded(1);
gio::spawn_blocking(move || { let _ = tx.send_blocking(core.git_snapshot(&dir)); });
glib::spawn_future_local(clone!(@weak window => async move {
    match rx.recv().await { Ok(Ok(snapshot)) => window.show(snapshot),
                            Ok(Err(e)) => window.error(e), Err(_) => {} }
}));
```

Git-операции короткие; `fetch`/`pull`/`push` долгие, но блокирующий поток их
переживает и UI не встаёт. Это минимум движущихся частей.

**Исключение:** `codexLimits()` — долгоживущий дочерний процесс с двусторонним
JSON-RPC по stdio и таймаутом 25 секунд. Его логичнее сделать на `gio::Subprocess`
с асинхронным чтением, а не в blocking-потоке. Он изолирован в `provider/codex.rs`
за тем же трейтом.

## Модель состояния

Текущий renderer перерисовывает весь DOM на каждое изменение (`render()` в
`renderer.js:83`). В GTK так нельзя — виджеты имеют состояние (фокус, скролл,
выделение), и тотальная пересборка его теряет.

**Схема:**

```
                  Workspace (convoy-core)
                  единственный источник истины, на диске
                            │
                            │ загрузка / update()
                            ▼
              AppState { workspace: Workspace,
                         projects: gio::ListStore<ProjectObject>,
                         sessions: gio::ListStore<SessionObject>,
                         specs, tasks, activity: ListStore<...>,
                         terminals: HashMap<SessionId, TerminalSession>,
                         queues: HashSet<ProjectId>,
                         selection: Selection }
                            │
                 ┌──────────┴──────────┐
                 ▼                     ▼
          GObject properties      сигналы
          → ListView/ColumnView   → точечное обновление
             через bindings          тулбара и заголовков
```

Правила:

1. После каждой мутации `Workspace` вызывается `AppState::sync()`, который
   **выравнивает** `ListStore` с состоянием (добавить/удалить/обновить по id),
   а не пересоздаёт его. `gio::ListStore` + `gtk::SignalListItemFactory`
   перерисуют только изменившиеся строки.
2. Скалярные части UI (заголовок, путь, доступность кнопок, git-инфо) обновляются
   явной функцией `refresh_selection()` — прямой аналог второй половины `render()`.
3. Списки в сайдбаре — `gtk::TreeListModel` поверх `ListStore`: группы проектов
   становятся настоящими раскрывающимися узлами. Это закрывает пункт
   «group labels rather than nested sidebar trees» из `docs/port/status.md`.

## Состояния, которых нет в `workspace.json`

Живут только в памяти, как и сейчас:

| Что | Где сейчас | Где будет |
|---|---|---|
| Запущенные терминалы | `terminals: Map` в `main.cjs:27` | `AppState.terminals` |
| Активные очереди | `queues: Set` | `AppState.queues` |
| Блокировки worktree | `busy: Set` | `AppState.busy` |
| Блокировки репозитория | `repositories: Map` | `AppState.repo_locks` |
| Выбор в списке транскриптов | `transcriptChoices: Map` | в самом диалоге |
| Состояние агента (idle/working/…) | `entry.agentState` | `TerminalSession.agent_state` |
| Хвост вывода | `entry.tail` (48000 симв.) | снимается из VTE по требованию |

## Что сознательно НЕ переносится

- `preload.cjs` — нет границы процессов.
- Проверка `event.senderFrame.url !== page` в `handle()` — защищала от
  скомпрометированного рендерера. Нет рендерера — нет угрозы.
- CSP, `setPermissionRequestHandler`, `setWindowOpenHandler`,
  `will-navigate` — специфика веб-движка.
- `render-process-gone` хендлер.
- `app.requestSingleInstanceLock()` → `gio::ApplicationFlags` даёт это из коробки.

**Что переносится обязательно, хоть и выглядит как «электронная» защита:**
валидация путей (`relative()`, realpath-проверки в `preview()`, `worktree-setup.cjs`),
лимиты размеров, очистка env в `git()`. Это защита от вредоносного содержимого
`workspace.json` и от симлинков в чужом репозитории, а не от рендерера.
См. [03-invariants.md](03-invariants.md).
