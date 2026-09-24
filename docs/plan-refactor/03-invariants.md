# 03. Инварианты — контракт поведения

Это главный документ плана. Всё остальное — способ его реализовать.

Список собран из кода и из 36 существующих тестов. Каждый пункт — поведение,
за которое кто-то уже заплатил отладкой. Порт обязан сохранить их все.
Формат: **инвариант** → где живёт сейчас → чем проверяется.

## Хранилище

**1. Запись атомарна.**
Временный файл `<file>.<uuid>.tmp` с `mode 0o600`, затем `rename()`. Временный
файл удаляется в `finally` при любом исходе.
→ `workspace.cjs:update()`
→ тест «invalid mutations leave both memory and disk untouched»

**2. Невалидное состояние не коммитится ни на диск, ни в память.**
`update()` делает `structuredClone`, применяет изменение, валидирует клон, и
только потом пишет и присваивает `this.state`. Если валидация или запись упали —
`this.state` остаётся прежним.
→ `workspace.cjs:update()`
→ тесты «invalid mutations…», «failed writes do not commit mutations in memory»

**3. Повреждённый или будущий файл никогда не перезаписывается.**
Неизвестная `schemaVersion` или битый JSON → исключение на старте, файл не тронут.
→ `workspace.cjs:validate()`
→ тест «corrupt and future workspace files are never overwritten»

**4. Миграция схем 1 и 2 аддитивна.**
Все ID, `providerID` и **неизвестные поля** сохраняются (`{ ...this.state, ... }`).
→ `workspace.cjs` конструктор
→ тест «schema 1 migrates without losing provider IDs or unknown data»

**5. Крэш-рекавери задач.**
При загрузке каждая задача в статусе `building` → `failed` с пояснением
«App closed while the session was running». Никакого автоматического рестарта.
→ `workspace.cjs` конструктор
→ тест «running tasks reject edits, and relaunch marks them interrupted without restarting»

## Сессии и провайдеры

**6. Resume использует точную личность разговора и никогда не переигрывает
первое сообщение.**
`claude --resume <uuid>` против `claude --session-id <uuid>`;
`codex resume <uuid>`. Prompt добавляется через `--` только при `resume === false`.
→ `launch.cjs:agentArgs()`
→ тесты «resume uses exact provider identity and never repeats the initial prompt»,
«enhanced launch passes model/settings before a literal prompt and never replays on resume»

**7. Claude-сессия обязана иметь UUID, Codex — нет.**
`if (session.agent === 'claude' && !session.providerID) throw`.
Codex получает ID позже, из вывода терминала.
→ `workspace.cjs:validate()`

**8. Флаги вставляются перед позиционным разделителем `--`.**
`--model` и `--settings` идут до `--`, иначе они станут частью prompt.
→ `provider.cjs:sessionSpec()`

**9. Env агента чистится от маркеров родительского разговора.**
Удаляются все `CLAUDE*` кроме `CLAUDE_CONFIG_DIR`, и `CODEX_THREAD_ID`.
Ставятся `TERM=xterm-256color`, `COLORTERM=truecolor`,
`CLAUDE_CODE_FORCE_SESSION_PERSISTENCE=1`.
→ `launch.cjs:agentEnvironment()`
→ тест «agent environment removes parent conversation markers without losing login configuration»

**10. POSIX-запуск сохраняет метасимволы как литералы.**
`bash -ilc 'exec ...'` с ручным квотированием `'` → `'"'"'`.
Логин-шелл именно `/bin/bash`, а не `$SHELL` — fish не исполнит POSIX-синтаксис.
→ `launch.cjs:launchSpec()`
→ тест «POSIX launch preserves shell metacharacters as literal arguments»

**11. Профиль аккаунта изолирует конфиг и вычищает API-ключи.**
`CLAUDE_CONFIG_DIR` / `CODEX_HOME` = `<root>/<sha256(profile.id)>`.
При использовании профиля из env удаляются `ANTHROPIC_API_KEY`,
`ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY`, `CODEX_API_KEY`.
`session.agentHome` фиксируется при первом запуске и больше не меняется.
→ `accounts.cjs`
→ тест «account profiles use separate homes, reject provider mismatches, and keep resumed homes stable»

**12. Запущенную сессию нельзя архивировать или перепривязать.**
`providerID` и `archived` заблокированы пока процесс жив. Notes и pinned — можно.
→ `workspace.cjs:editSession()`
→ тест «running sessions cannot be archived or rebound, but notes and pinning can change»

## Задачи, спеки, очередь

**13. Нулевой код выхода означает `review`, а не `done`.**
Принятие задачи всегда явное, никогда автоматическое.
→ `main.cjs:301–330` (`onExit`), `finishTask()`
→ README.md desktop: «Claude Stop events or a successful CLI exit move tasks to review, **not done**»

**14. Правка спеки инкрементирует ревизию, снимает одобрение и помечает
выполненные задачи как `changes`.**
Неизменённая спека сохраняет одобрение (сравнение всех 6 полей до записи).
→ `planning.cjs:saveSpec()`
→ тесты «spec approval gates task preparation…», «unchanged specs retain approval…»

**15. Запуск задачи требует одобренной текущей ревизии.**
Проверка дважды: в `prepareTask()` и повторно в `start()` —
`spec.approvedRevision === spec.revision === task.specRevision`.
→ `planning.cjs:prepareTask()`, `main.cjs:start()`

**16. Падение задачи ставит очередь на паузу; рестарт никогда не возобновляет её молча.**
`queues.delete(projectID)` при ненулевом выходе или ошибке. Очередь живёт
только в памяти и не переживает перезапуск.
→ `main.cjs:onExit`, `runNext()`

**17. Бриф задачи ограничен 32 000 символов и режектится до создания сессии.**
→ `planning.cjs:prepareTask()`

**18. Смена режима публикации или автоматического ревью инвалидирует
подготовленный бриф.**
Изменение `title`/`details`/`agent`/`mode`/`autoReview` сбрасывает статус в
`queued` и отвязывает сессию.
→ `planning.cjs:saveTask()`
→ тест «publication and review choices are explicit and changing them invalidates the old brief»

## Git

**19. Все вызовы git обеззараживают окружение.**
`GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, и удаляются `GIT_DIR`,
`GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY`,
`GIT_ALTERNATE_OBJECT_DIRECTORIES` — иначе переменные родительского агента
перенаправят операцию в чужой репозиторий.
Всегда `--literal-pathspecs`: имя файла со звёздочкой не должно стать шаблоном.
Таймаут 60 с, `maxBuffer` 2 МБ.
→ `git.cjs:git()`
→ тест «staging a path treats Git wildcard characters literally»

**20. Discard hunk сверяет sha256 диффа и отказывает на устаревшем.**
Патч применяется через `git apply --check --reverse` во временном каталоге,
и только потом по-настоящему. Файлы с `new file`/`deleted file`/`rename` —
нельзя по одному ханку.
→ `files.cjs:mutate() case 'discardHunk'`
→ тесты «discard hunk rejects stale diffs and preserves other hunks»,
«discard and hunk discard respect Git CRLF checkout settings»

**21. Worktree удаляется только чистым, без `--force`, ветка сохраняется.**
Все связанные сессии архивируются и получают `worktreeRemoved: true`.
Проверка запущенных сессий делается **дважды**: до диалога и после него.
Удалять можно только worktree, созданный самим приложением
(`ownsWorktree && dirname(dir) === worktreeRoot`).
→ `main.cjs:worktree:remove`
→ тест «real worktrees isolate changes, reject invalid branches, and refuse dirty removal»

**22. Shared paths никогда не перезаписывают существующие файлы и не следуют
симлинкам.**
`fs.cp(..., { force: false, errorOnExist: true, dereference: false })`.
Каждый компонент пути назначения проверяется на `isDirectory() && !isSymbolicLink()`.
`.git` шарить запрещено. Setup-команда с таймаутом 120 с; при провале worktree
сохраняется.
→ `worktree-setup.cjs`
→ тест «shared-file setup never follows destination symlinks or overwrites existing files»

## Файлы и превью

**23. Чтение ограничено и проверено.**
- `relative()` отбивает абсолютные пути, `..`, `\0`.
- `realpath(root)` + `realpath(target)`, затем проверка префикса `base + sep`.
- Максимум 1 МБ, читается `size + 1` байт — файл, выросший после `stat`, не
  проскочит.
- Нулевой байт в содержимом → «binary, preview unavailable».
- Обход дерева ограничен: 3000 узлов, глубина 10, 10 000 файлов.
- Discovery останавливается на границе проекта, пропускает скрытые каталоги,
  зависимости и не идёт по симлинкам.
→ `files.cjs`
→ тесты «discovery stops at project boundaries…», «preview rejects traversal, external symlinks, binary and oversized files»

**24. В корзину можно только неотслеживаемые файлы.**
Проверяется по свежему `snapshot()`, родитель проверяется через `realpath`
(а не сам файл — чтобы не пойти по его симлинку). Подтверждение обязательно.
→ `main.cjs:files:mutate case 'trash'`

## Терминал, история, телеметрия

**25. Вставка текста никогда не нажимает Enter без явного согласия.**
Bracketed paste `\x1b[200~ … \x1b[201~`, `\r` добавляется только при
`submit === true`. `\r` из самого текста вырезается. Управляющие символы
вычищаются. Лимит 32 000 символов.
→ `history.cjs:paste()`
→ тест «feedback paste strips control sequences and does not press Enter»

**26. Сохранённый вывод ограничен и не покидает свой каталог.**
Имя файла — `sha256(session.id)`, хвост 48 000 символов, `mode 0o600`,
запись через `.tmp` + rename. Это выжимка, а не полный транскрипт.
→ `history.cjs:History`
→ тест «snapshots remain bounded plain text, and IDs cannot escape the history directory»

**27. Телеметрия хранит только валидированные окна квот и никогда не угадывает ноль.**
`used_percentage` должен быть конечным числом в [0, 100]; `resets_at` — либо
отсутствует, либо в будущем. Отсутствующая квота — «unavailable», не «0%».
Старые `.status`/`.usage` удаляются перед каждым запуском, чтобы прошлый Stop
не пометил свежий запуск завершённым.
→ `telemetry.cjs:sanitize()`, `configuration()`
→ тест «telemetry stores only status and validated quota windows»

**28. Codex usage не делает модельных запросов.**
Только `initialize` → `initialized` → `account/rateLimits/read`. Таймаут 25 с,
лимит ответа 2 МБ.
→ `provider.cjs:codexLimits()`
→ тест «Codex usage performs only initialization and rate-limit read»

**29. Гибернация срабатывает только после явного события завершения хода.**
`agentState === 'done'` плюс настроенный простой. Никаких догадок по тексту
терминала. Возобновление всегда явное.
→ `main.cjs:401–428` (`monitor`)

**30. Остановка сессии убивает группу процессов.**
`kill(-pid, SIGHUP)`, через 1.5 с `SIGKILL` если процесс ещё жив.
Иначе дочерние процессы агента остаются сиротами.
→ `main.cjs:375–390` (`stop`)

## Общие

**31. Деструктивные действия подтверждаются.**
`discard`, `discardHunk`, `revert`, `resetSoft`, `resetMixed`, `commit --amend`,
`trash`, удаление проекта, удаление worktree, запуск очереди, выход с
запущенными сессиями.
→ `main.cjs`

**32. Журнал активности ограничен 200 записями и не хранит учётные данные.**
→ `planning.cjs:record()`, `validatePlanning()`
→ тест «activity stays bounded and persisted without storing credentials»

**33. Одновременная работа над одним репозиторием блокируется.**
`repositories: Map` — один git-мутатор на каталог за раз.
`busy: Set` — сессии в процессе worktree-операции.
→ `main.cjs`

**34. Данные превью отделены от нативного приложения.**
`~/.config/Convoy Desktop Preview/`. Swift и порт не должны смотреть в один файл.
→ `main.cjs:app.setPath('userData')`, README.md desktop
