# 10. Дорожная карта

Оценки — в рабочих днях одного человека. Итого **35–45 дней ≈ 7–9 недель**
при полном объёме. M1+M2 (≈2 недели) дают приложение, которым уже можно
пользоваться каждый день.

Порядок не случаен: сначала вертикальный срез, проверяющий рискованные
допущения, потом горизонтальное расширение.

**Статус на 24.09.2026.** M0 и M1 закрыты. `convoy-core` — 61 тест зелёный,
`cargo clippy --all-targets -- -D warnings` чист по всему workspace.
VTE проверен на живых агентах: 12 из 12 автоматических сценариев зелёные,
Claude Code 2.1.273 и Codex 0.156.0 рисуются полностью. Две находки, менявшие
план, записаны в [05](05-terminal-vte.md): снимать текст надо через
`text_format`, и `watch_child` нужен запасной путь при мгновенном выходе.
Ближайший шаг — M2.

---

## M0 — Разведка и скелет · 1–2 дня

Цель: убедиться, что фундамент держит, до того как в него вложено время.

- [x] `pacman -S vte4`
- [x] Подобрать совместимые версии крейтов: `gtk4` ↔ `libadwaita` ↔ `vte4` ↔
      `sourceview5` из одного поколения. Собрать пустое окно.
- [x] **Прототип VTE на 50 строк** (см. [05](05-terminal-vte.md), «Что проверить»):
      запуск `bash -ilc`, `child-exited` с кодом, `killpg`, снятие текста,
      прямая запись в `pty().fd()`.
- [x] **Запустить в нём настоящий Claude Code и Codex.** Проверить alt-screen,
      цвета, ширину emoji, мышь, bracketed paste, ресайз.
      *Сделано автоматически (`convoy-vte-selftest`, 12/12). Мышь и
      перерисовка под нагрузкой остались на ручную проверку в `convoy-vte-probe`.*
- [x] Скелет workspace: два крейта, `cargo build`, пустое `adw::ApplicationWindow`.
- [x] Проверить, что `glib::user_config_dir()` даёт тот же путь, что Electron.

**Выход достигнут.** Прототип (`convoy-vte-probe`) и автоматический набор
проверок (`convoy-vte-selftest`) работают. Claude Code и Codex в VTE выглядят
правильно, ширина emoji и CJK верная. Две ошибки в самом плане найдены и
исправлены до того, как на них было потрачено время.

---

## M1 — Ядро · 5–7 дней

Цель: вся логика на Rust, покрытая тестами, без единой строчки UI.

- [x] `workspace/model.rs` — все типы + `#[serde(flatten)] unknown`
- [x] `workspace/validate.rs` — полный порт трёх функций валидации
- [x] `workspace/mod.rs` — загрузка, `update()`, атомарная запись, миграция
- [x] `git/` — `git()`, `parseStatus()`, `snapshot()`, `hunks()`, `mutate()`
- [x] `files/` — `discover()`, `preview()`, `fileContent()`, `relative()`
- [x] `provider/launch.rs` — `launchSpec()`, `agentArgs()`, `agentEnvironment()`
      (минус Windows)
- [x] `provider/mod.rs` — `providerSpec()`, `sessionSpec()`
- [x] `planning/` — spec/task/queue-логика, `markdown()`
- [x] `accounts.rs`, `history.rs`, `worktree.rs`
- [x] `telemetry/` — `sanitize()`, `configuration()`, `read()`, `hook::run()`
- [x] `process.rs` — трейт `ProcessRunner` + `StdRunner`
- [x] **Порт 26 тестов** (см. [08](08-testing.md))
- [x] Сверх плана: `storage.rs` (пути), `transcripts.rs`, `codex.rs`,
      `repository.rs`, `ansi.rs`, `time.rs`, `json.rs` (семантика JS-строк)

**Выход достигнут:** `cargo test -p convoy-core` — 61 тест, зелёный.
Из 36 тестов Electron перенесены все, кроме четырёх Windows-специфичных;
добавлено 8 новых на валидацию (шорткаты, границы настроек, ссылочная
целостность) и 8 юнит-тестов на модули, которых в JS не было отдельно.

---

## M2 — Вертикальный срез · 5–7 дней

Цель: приложение, которым можно работать. Не полное, но настоящее.

- [ ] `adw::ApplicationWindow` + `NavigationSplitView`
- [ ] Сайдбар: дерево проектов из `TreeListModel`, поиск
- [ ] Open folder (`gtk::FileDialog`)
- [ ] `adw::TabView` с сессиями проекта
- [ ] Диалог новой сессии
- [ ] **VTE-терминал: Start / Stop / Resume**
- [ ] `AppState::sync()` + `refresh_selection()`
- [ ] Toast-ошибки
- [ ] Тема (dark/light/system) и настройки шрифта/скроллбека
- [ ] Сохранение вывода в History по `contents-changed`

**Выход:** можно открыть проект, создать сессию, запустить Claude Code,
поработать, остановить, вернуться. Это уже замена Electron-версии на 40%
сценариев. **С этого момента порт используется как основной инструмент** —
остальные этапы дорабатываются на живом использовании.

---

## M3 — Сессии целиком · 4–5 дней

- [ ] Меню сессии: 13 действий
- [ ] Review handoff: бриф, выбор агента, связь builder ↔ review
- [ ] Send feedback to builder (вставка без submit)
- [ ] Fresh recovery session
- [ ] Pin / archive / edit
- [ ] Worktrees: создание, setup-команда с подтверждением, удаление
- [ ] Split-панели (`gtk::Paned`, две панели)
- [ ] Saved output, git-статус в панели
- [ ] Quick commands

**Выход:** паритет по работе с сессиями.

---

## M4 — Files & Changes · 5–7 дней

Самый объёмный UI-кусок.

- [ ] `adw::NavigationPage` + `ViewSwitcher` на 4 вкладки
- [ ] Changes: список, stage/unstage/discard/trash, подтверждения
- [ ] Превью: `GtkSourceView` с языком `diff`
- [ ] Split diff с номерами строк
- [ ] Markdown через `pulldown-cmark`, картинки через `gtk::Picture`
- [ ] Discard hunk с проверкой sha256
- [ ] Коммит, amend, черновики сообщений
- [ ] Generate with Claude (с защитой от затирания)
- [ ] Fetch / pull / push / PR
- [ ] Log с diff коммита, revert, reset soft/mixed
- [ ] Branches: переключение, создание
- [ ] Отмена устаревших асинхронных чтений (`generation`)

**Выход:** паритет по Git-панели, причём лучше текущего — подсветка
и номера строк бесплатно.

---

## M5 — Specs & tasks · 4–5 дней

- [ ] `adw::NavigationView` со списком спек и задач
- [ ] Формы спеки и задачи, ревизии, одобрение
- [ ] Экспорт Markdown (`gtk::FileDialog::save`)
- [ ] Prepare session, выбор аккаунта
- [ ] Очередь: запуск, пауза, подтверждение со списком
- [ ] `runNext()` / `finishTask()` + автоматический review handoff
- [ ] Правила переходов статусов + новые юнит-тесты на них

**Выход:** паритет по SDD-части.

---

## M6 — Интеграции · 4–5 дней

- [ ] `--hook` режим бинарника, генерация settings.json
- [ ] Status line (опционально, по настройке)
- [ ] `monitor` на `glib::timeout` — состояния агента, гибернация
- [ ] Уведомления `gio::Notification` + `app.focus-session`
- [ ] Keep awake через `Application::inhibit`
- [ ] Codex usage через `gio::Subprocess`
- [ ] Профили аккаунтов: создание, удаление, изоляция
- [ ] История провайдера: скан и импорт транскриптов

**Выход:** паритет по интеграциям. Самая «невидимая» работа — почти нет UI,
но много поведения.

---

## M7 — Навигация и настройки · 3–4 дня

- [ ] `gio::SimpleAction` для всех действий + accels
- [ ] Конвертер `mod+shift+p` ↔ `<Primary><Shift>p`
- [ ] Командная палитра
- [ ] Редактор шорткатов с валидацией
- [ ] `adw::PreferencesDialog`: Appearance, Agents, Session, Shortcuts, Accounts
- [ ] Настройки проекта
- [ ] Журнал активности
- [ ] Контекстные меню в сайдбаре
- [ ] Show in Files, Copy path (паритет с macOS-версией)

**Выход:** полный паритет функций.

---

## M8 — Упаковка и релиз · 3–5 дней

- [ ] `.desktop`, иконки, application id `com.iftech.convoy.linux`
- [ ] PKGBUILD для AUR
- [ ] `cargo-deb`
- [ ] CI workflow `linux.yml`
- [ ] GTK smoke-тесты под `xvfb-run`
- [ ] Прогон полного набора инвариантов вручную по чек-листу из [03](03-invariants.md)
- [ ] Замер памяти и сравнение с 745 МБ
- [ ] README для `apps/linux/`
- [ ] Обновить `docs/port/status.md`: что проверено, что нет

**Выход:** устанавливаемый пакет.

---

## Definition of done для каждого этапа

1. `cargo clippy -- -D warnings` чистый.
2. Новый код покрыт тестами там, где это чистая логика.
3. Затронутые инварианты из [03](03-invariants.md) перепроверены.
4. Этап отработан на настоящем проекте, а не на фикстурах.

## Чего НЕ делаем в этом порте

Явно вне скоупа, чтобы не расползалось:

- Windows и macOS — остаются на своих реализациях.
- Переиспользование `convoy-core` из Swift — архитектурно подготовлено,
  но не реализуется.
- Переживание PTY после перезапуска приложения (этого нет и в Electron).
- Повторяющиеся автономные циклы fix/review (этого нет и в macOS-версии).
- Более двух терминальных панелей.
- Полноценный Markdown-рендер — остаётся базовым, как сейчас.
- WSL-бэкенд.
