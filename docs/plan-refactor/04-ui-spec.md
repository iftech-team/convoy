# 04. Спецификация UI

Главный принцип: **не переносить DOM один в один.** Текущая разметка — 20
`<dialog>` в одном `index.html`, потому что так дешевле всего в вебе. В
libadwaita половина из них должна стать не модалками, а страницами настроек и
навигационными видами. Это заодно закрывает претензии из `docs/port/status.md`
(«Git uses a dialog», «group labels rather than nested sidebar trees»).

Разметка пишется на **Blueprint** (`.blp`) там, где структура статична, и кодом —
там, где виджеты создаются динамически. Blueprint компилируется в `.ui` на этапе
сборки и встраивается через `gio::Resource`.

## Главное окно

```
adw::ApplicationWindow
└── adw::NavigationSplitView
    ├── sidebar: adw::NavigationPage "Convoy"
    │   └── adw::ToolbarView
    │       ├── top: adw::HeaderBar
    │       │        ├── start: MenuButton (☰)  → Open folder, Discover projects,
    │       │        │                             Accounts, Activity, Settings
    │       │        └── title: gtk::SearchEntry
    │       ├── content: gtk::ScrolledWindow
    │       │        └── gtk::ListView(TreeListModel<ProjectObject>)
    │       └── bottom: gtk::Label "N running"
    └── content: adw::NavigationPage
        └── adw::ToolbarView
            ├── top: adw::HeaderBar
            │        ├── title: adw::WindowTitle(project.title, path)
            │        └── end: History │ Project │ Files │ Specs │ ＋ New session
            ├── top: adw::TabBar  ← сессии проекта
            ├── content: gtk::Stack
            │        ├── "empty"  → adw::StatusPage
            │        └── "terminals" → gtk::Paned (split) │ одиночный VTE
            └── bottom: gtk::ActionBar
                     Start │ Stop │ Quick commands │ ⋯ │ git-инфо │ ссылки ревью
```

Ошибки — `adw::ToastOverlay` поверх content, вместо красной полосы `#error`.
Ошибки внутри диалогов — `adw::Banner` в шапке диалога (аналог `.modal-error`).

## Сайдбар: проекты

Текущий `#projects` — плоский список кнопок с текстом `group / icon title`.
Становится настоящим деревом:

- `gio::ListStore<ProjectObject>` → `gtk::TreeListModel` (группы как узлы) →
  `gtk::FilterListModel` (поиск) → `gtk::SingleSelection` → `gtk::ListView`.
- Строка проекта — `gtk::Box`: иконка (emoji из `project.icon`), заголовок,
  `gtk::Label` с числом сессий.
- Группы формируются из поля `project.group` — оно уже заполняется при
  `project:discover` (`main.cjs`: `p.group = path.basename(root)`).
- Контекстное меню строки (`gtk::GestureClick` + `gtk::PopoverMenu`):
  New session, Discover projects here, Show in Files, Copy path,
  Reconnect folder, Remove project.
- Поиск фильтрует и проекты, и сессии — как сейчас в `visible()`.

## Сессии

Сейчас — горизонтальный ряд кнопок `#sessions` плюс чекбокс «Show archived».
Становится `adw::TabBar` + `adw::TabView`, где каждая вкладка — сессия:

- Заголовок вкладки: `★` (pinned), `●` (running), имя, состояние агента
  (`idle` / `working` / `waiting` / `done`) — как в `renderer.js:97`.
- Индикатор `●` — `adw::TabPage::set_indicator_icon` / `set_loading`.
- Архивные вкладки не показываются; переключатель «Show archived» переезжает
  в меню `⋯` панели.
- `adw::TabView` бесплатно даёт перетаскивание, закрытие по средней кнопке,
  и меню вкладки — туда уходит часть действий из `#session-menu`.

**Меню сессии (`⋯`)** — `gtk::PopoverMenu`, 13 действий из
`renderer.js:175–218`:

| Действие | GTK action | Условие доступности |
|---|---|---|
| Fresh recovery session | `session.recover` | не запущена, не busy |
| Edit name, notes, provider ID | `session.edit` | providerID — только если не запущена |
| Pin / unpin | `session.pin` | всегда |
| Archive / restore | `session.archive` | не запущена |
| Start review… | `session.review` | всегда |
| Send feedback to builder… | `session.feedback` | только у сессии с `reviewOf` |
| Usage limits… | `session.usage` | всегда |
| Saved output… | `session.history` | всегда |
| Create worktree… | `session.worktree` | новая, не запущена, без worktree |
| Remove worktree… | `session.remove-worktree` | `ownsWorktree` |
| Refresh Git status | `session.git-status` | всегда |
| Open split terminal… | `session.split` | есть другая сессия |
| Close split view | `session.unsplit` | активен split |

## Терминалы и split

- Одиночный режим: `vte4::Terminal` прямо в `gtk::Stack`.
- Split: `gtk::Paned` с двумя VTE. Сейчас ровно две панели (`panes` — массив
  из двух элементов), оставляем то же ограничение в первой версии;
  `gtk::Paned` можно вкладывать позже без изменения модели.
- Метка панели (`pane-label`) → `gtk::Label` в маленьком `ActionBar` над VTE,
  видна только в split-режиме.
- Фокус панели (`focused-pane`) → CSS-класс на рамке + `terminal.grab_focus()`.
- `fitActive()`, `FitAddon`, `ResizeObserver` — **не нужны**, VTE пересчитывает
  геометрию сама при изменении аллокации.

## Диалоги

libadwaita 1.5+ даёт `adw::Dialog` — не отдельное окно, а наложение внутри
окна, адаптивное. Используем его везде вместо `<dialog>`.

| Сейчас (`index.html`) | Становится | Почему |
|---|---|---|
| `#session-dialog` | `adw::Dialog` + `adw::PreferencesGroup` | форма из 5 полей |
| `#edit-dialog` | `adw::Dialog` | форма из 3 полей |
| `#text-dialog` | `adw::Dialog` + `GtkSourceView` | длинный текст |
| `#history-dialog` | `adw::Dialog` + `GtkSourceView` (read-only, моно) | |
| `#worktree-dialog` | `adw::AlertDialog` + `adw::EntryRow` | одно поле |
| `#settings-dialog` | **`adw::PreferencesDialog`** | нативная страница настроек |
| `#commands-dialog` | `adw::Dialog` со списком `adw::ActionRow` | |
| `#split-dialog` | `gtk::Popover` от кнопки | просто выбор из списка |
| `#accounts-dialog` | **`adw::PreferencesDialog`** страница | |
| `#activity-dialog` | `adw::Dialog` + `gtk::ListView` | |
| `#planning-dialog` | **`adw::NavigationPage`** в основном окне | не модалка: это рабочая область |
| `#spec-dialog` | `adw::NavigationPage` внутри Specs | вложенная навигация вместо закрыть/открыть |
| `#task-dialog` | `adw::NavigationPage` внутри Specs | то же |
| `#prepare-dialog` | `adw::AlertDialog` | одно поле |
| `#provider-history-dialog` | `adw::Dialog` | |
| `#palette-dialog` | `adw::Dialog` + SearchEntry + ListView | |
| `#shortcuts-dialog` | **`adw::PreferencesDialog`** страница | |
| `#usage-dialog` | `adw::AlertDialog` | только текст |
| `#project-dialog` | **`adw::PreferencesDialog`** | 6 полей, часть многострочных |
| `#files-dialog` | **`adw::NavigationPage`** в основном окне | главная претензия к текущему UI |

Обратить внимание на текущий антипаттерн: `#planning-dialog` закрывается,
чтобы открыть `#spec-dialog`, а при сабмите — открывается заново
(`renderer.js:renderPlanning`, `editSpec`). С `adw::NavigationView` это
становится обычным push/pop без мигания.

## Files & Changes — детально

Становится полноценной страницей, а не модалкой:

```
adw::NavigationPage "Files & Changes · <branch>"
└── adw::ToolbarView
    ├── top: adw::HeaderBar
    │        ├── title: adw::ViewSwitcher [Changes│Files│Log│Branches]
    │        └── end: Refresh
    ├── top: gtk::SearchEntry (фильтр)
    ├── content: gtk::Paned (горизонтальный)
    │        ├── gtk::ListView       ← список записей вкладки
    │        └── gtk::Stack
    │             ├── "source"  → GtkSourceView (диффы, файлы)
    │             ├── "split"   → Paned(GtkSourceView, GtkSourceView)
    │             ├── "markdown"→ GtkTextView с Pango markup
    │             └── "image"   → gtk::Picture
    └── bottom: gtk::Stack по вкладке
             Changes  → поле коммита, Amend, Generate, Commit, Stage all,
                        Fetch, Pull, Push, Create PR
             Log      → Revert, Reset soft, Reset mixed
             Branches → поле новой ветки, Create and switch
```

Строка изменения — `adw::ActionRow`:
- префикс: две буквы статуса `XY` в моноширинном `gtk::Label` + иконка;
- заголовок: путь (+ `original` для переименований);
- суффиксы: Stage / Unstage / Staged diff / Discard / Trash — по тем же
  условиям, что в `files-ui.js:draw()`.

Подсветка диффа — `GtkSourceLanguage "diff"`, бесплатно. Номера строк в
split-режиме — `show-line-numbers`, тоже бесплатно. Ручной парсер
`split-diff` из `files-ui.js` удаляется.

Кнопки «Discard hunk N» — `gtk::Box` над превью, максимум 50 штук
(как сейчас), строятся из результата `hunks()` в core.

## Specs & tasks

```
adw::NavigationPage "Specs & tasks"
└── adw::NavigationView
    ├── "list" → adw::PreferencesPage
    │      ├── group "Specifications" → adw::ExpanderRow на спеку
    │      │      заголовок: title · r<N> · Approved/Draft
    │      │      действия: Approve revision, Edit, Export Markdown
    │      └── group "Tasks" → adw::ActionRow на задачу
    │             заголовок: title · status
    │             суффиксы: gtk::DropDown статуса, Edit,
    │                       Open session / Prepare session
    │             подзаголовок: task.lastError, если есть
    ├── "spec" → adw::PreferencesPage с 6 полями
    └── "task" → adw::PreferencesPage с полями задачи
```

Правила доступности из `renderer.js:renderPlanning()` сохраняются:
- `Approve` неактивна, если `approvedRevision === revision`;
- статусы `building` и `failed` нельзя выбрать вручную;
- задача в `building` не редактируется и не меняет статус;
- `Prepare session` показывается, только если сессии нет, ревизия устарела
  или сессия архивирована.

Кнопка «Run / pause queue» — в шапке страницы, с подтверждением
(`adw::AlertDialog`) со списком задач и предупреждением о публикации.

## Настройки

`adw::PreferencesDialog` со страницами:

**Appearance** — тема (`adw::ComboRow`: Dark / Light / System →
`adw::StyleManager::set_color_scheme`), размер шрифта терминала
(`adw::SpinRow` 10–24), скроллбек (`adw::SpinRow` 1000–50000).

**Agents** — агент по умолчанию, Claude usage status line (`adw::SwitchRow`
с подзаголовком «next launch; replaces the CLI status line»).

**Session behaviour** — keep awake (`adw::ComboRow`: Off / While agents run /
While Convoy is open), hibernate minutes (`adw::SpinRow` 0–1440),
уведомления (`adw::SwitchRow`).

**Shortcuts** — 7 `adw::EntryRow`. Валидация формата и уникальности
до сохранения, как в `validateSettings()`.

**Accounts** — список профилей `adw::ActionRow` + кнопка добавления.

## Тема и стили

`style.css` (112 строк) почти целиком не нужен: libadwaita даёт цвета,
отступы, фокус-кольца и тёмную тему. Остаётся ~20 строк на:
- моноширинный шрифт и фон для VTE-контейнера;
- рамку активной панели в split-режиме (`.focused-pane`);
- цвета строк диффа, если GtkSourceView-схемы окажется мало;
- статусные буквы `XY` в списке изменений.

Цвета терминала берутся из текущего кода (`renderer.js:theme()`):
- тёмная: bg `#111318`, fg `#e4e7ee`, cursor `#88a5ff`;
- светлая: bg `#fafbfe`, fg `#202637`, cursor `#3854a4`.

Режим `system` — подписка на `adw::StyleManager::dark` notify вместо
`matchMedia('(prefers-color-scheme: light)')`.

## Доступность

Текущая разметка аккуратна с ARIA (`aria-label`, `aria-pressed`, `role="alert"`).
В GTK это `accessible-role` и `gtk::Accessible::update_property`. Для
динамических строк — задавать подписи явно, не полагаться на текст кнопок.
VTE доступна для скринридеров из коробки, в отличие от canvas-based xterm.js.
