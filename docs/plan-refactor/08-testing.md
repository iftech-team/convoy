# 08. Тестирование

## Что есть сейчас

36 тестов в `apps/desktop/test/`, из них 29 логических и 7 smoke.

| Файл | Тестов | Портируется в |
|---|---:|---|
| `core.test.cjs` | 10 | `convoy-core` юнит-тесты |
| `files.test.cjs` | 7 | `convoy-core` интеграционные (реальный git) |
| `features.test.cjs` | 7 | `convoy-core` юнит + интеграционные |
| `planning.test.cjs` | 7 | `convoy-core` юнит-тесты |
| `provider.test.cjs` | 5 | `convoy-core` юнит-тесты (минус 3 Windows-теста) |
| `ui-smoke.cjs` | — | GTK smoke |
| `app-smoke.cjs` | — | GTK smoke |
| `pty-smoke.cjs` | — | VTE smoke |

## Что переносится один в один

Эти 26 тестов проверяют чистую логику и переезжают в `cargo test` без
изменения смысла:

**Хранилище** (`core.test.cjs`)
- projects and sessions survive relaunch; duplicate folders are ignored
- corrupt and future workspace files are never overwritten
- invalid mutations leave both memory and disk untouched
- failed writes do not commit mutations in memory
- schema 1 migrates without losing provider IDs or unknown data
- settings and scoped commands persist; invalid changes are atomic
- running sessions cannot be archived or rebound, but notes and pinning can change
- review handoff preserves folder, branch, source link and uses a new provider identity
- snapshots remain bounded plain text, and IDs cannot escape the history directory
- feedback paste strips control sequences and does not press Enter

**Планирование** (`planning.test.cjs`) — все 7
- spec approval gates task preparation and edits invalidate completed work
- unchanged specs retain approval and task edits preserve previous session history
- running tasks reject edits, and relaunch marks them interrupted without restarting
- markdown export includes acceptance, revision, tasks and findings
- account profiles use separate homes, reject provider mismatches, and keep resumed homes stable
- activity stays bounded and persisted without storing credentials
- publication and review choices are explicit and changing them invalidates the old brief

**Провайдеры** (`provider.test.cjs`) — 2 из 5
- resume uses exact provider identity and never repeats the initial prompt
- agent environment removes parent conversation markers without losing login configuration

**Файлы и git** (`files.test.cjs`) — все 7, через `tempfile` + реальный `git init`
- discovery stops at project boundaries and skips dependencies and symlinks
- preview rejects traversal, external symlinks, binary and oversized files
- Git panel handles unborn repositories, spaced paths, stage, commit, diffs and rename records
- discard hunk rejects stale diffs and preserves other hunks
- staging a path treats Git wildcard characters literally
- shared-file setup never follows destination symlinks or overwrites existing files
- discard and hunk discard respect Git CRLF checkout settings

**Фичи** (`features.test.cjs`) — 4 из 7
- real worktrees isolate changes, reject invalid branches, and refuse dirty removal
- telemetry stores only status and validated quota windows
- Codex usage performs only initialization and rate-limit read
- provider history matches folders and excludes Codex subagents

## Что выбрасывается

3 Windows-теста:
- Windows launch encodes fixed provider arguments
- Windows rejects prompt argv until npm shim escaping is supported
- Windows PowerShell forwards provider flags
- Windows npm resolver launches scripts directly with literal argument vectors

Остаётся 1 POSIX-тест, который сейчас помечен `skip` на Windows и станет
безусловным:
- POSIX launch preserves shell metacharacters as literal arguments

## Чего в тестах не хватает

Дыры, которые видно по коду и которые стоит закрыть при портировании —
дешевле написать сразу, чем ловить в проде:

1. **`start()` целиком не покрыт.** Самая сложная функция в проекте
   (`main.cjs:251–330`) тестируется только через smoke. Проверки брифа задачи,
   отсутствующего каталога аккаунта, отката при сбое записи — не покрыты.
   В Rust это тестируемо: `ProcessRunner` подменяется заглушкой.

2. **Логика очереди (`runNext` / `finishTask`) не покрыта юнит-тестами.**
   Есть только smoke с двумя задачами. Правила перехода статусов
   (`changes` / `review` / `failed`) стоит вынести в чистую функцию и покрыть
   таблично.

3. **`monitor` не покрыт.** Переходы состояний агента, гибернация,
   игнорирование устаревших значений — чистая логика, поддающаяся тестам,
   если отделить её от таймера.

4. **Валидация шорткатов** покрыта только косвенно. Формат + уникальность +
   конвертер в GTK-accel — хороший кандидат на property-тест.

5. **`rateWindows()`** — граничные значения (0, 100, 101, отрицательные,
   истёкший `resetsAt`) стоит покрыть таблично.

## Smoke-тесты в GTK

Три существующих smoke-теста требуют адаптации:

**`pty-smoke.cjs` → `tests/vte_smoke.rs`**
Запустить VTE с безобидным `/bin/echo`, дождаться `child-exited`, проверить
код и содержимое буфера. Требует дисплей.

**`ui-smoke.cjs` → `tests/ui_smoke.rs`**
Собрать главное окно, проверить, что виджеты создаются и основные действия
зарегистрированы. `gtk::test_init()` или обычный `Application` в headless.

**`app-smoke.cjs` → `tests/app_smoke.rs`**
Полный цикл: создать временный workspace, открыть окно, создать сессию,
запустить безобидную фикстуру вместо реального агента, остановить,
проверить состояние на диске.

**Headless-окружение.** Сейчас CI использует `xvfb-run -a`. Для GTK4
варианты:
- `xvfb-run -a` — работает, GTK4 умеет X11;
- `weston --backend=headless-backend.so` + `WAYLAND_DISPLAY` — ближе к
  реальному окружению;
- `GDK_BACKEND=broadway` — не подходит, VTE там ведёт себя иначе.

Рекомендация: `xvfb-run` для CI (проще, уже отлажено), Wayland — для
локальной проверки.

## Организация

```
crates/convoy-core/
  src/…                          #[cfg(test)] mod tests — юнит-тесты рядом с кодом
  tests/
    git_integration.rs           реальные репозитории в tempdir
    workspace_persistence.rs     атомарность, миграции, отказы
    worktree.rs                  shared paths, setup, удаление
    provider.rs                  launchSpec, env, rateWindows
    transcripts.rs               фикстуры .jsonl

crates/convoy-gtk/
  tests/
    vte_smoke.rs
    ui_smoke.rs
    app_smoke.rs
```

## Правила, которые нельзя нарушать

Из текущего README desktop: **тесты никогда не отправляют модельные запросы
и не публикуют ветки или PR.** Все агенты подменяются безобидными шелл-фикстурами.
Тесты не трогают настоящий `~/.config/Convoy Desktop Preview/` — только tempdir.

Перенести дословно в новый CI.
