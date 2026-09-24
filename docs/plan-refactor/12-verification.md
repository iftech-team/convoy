# 12. Проверка инвариантов

Каждый из 34 инвариантов из [03](03-invariants.md) — с тем, чем он проверен в
порте. «Автоматически» значит, что проверка падает, если поведение изменится.

Прогон: `cargo test -p convoy-core` (79), `xvfb-run -a cargo test -p convoy-gtk
--test ui` (70), `xvfb-run -a cargo run --bin convoy-vte-selftest` (12),
`./scripts/smoke.sh`.

## Хранилище

| # | Инвариант | Чем проверен |
|---|---|---|
| 1 | Запись атомарна | `workspace::update` пишет во временный файл и делает rename; `failed_writes_do_not_commit_mutations_in_memory` |
| 2 | Невалидное состояние не коммитится | `invalid_mutations_leave_both_memory_and_disk_untouched` |
| 3 | Повреждённый или будущий файл не перезаписывается | `corrupt_and_future_workspace_files_are_never_overwritten` (три варианта) |
| 4 | Миграция аддитивна | `schema_1_migrates_without_losing_provider_ids_or_unknown_data` + живой round-trip с Electron-сборкой |
| 5 | Крэш-рекавери задач | `running_tasks_reject_edits_and_relaunch_marks_them_interrupted_without_restarting` |

## Сессии и провайдеры

| # | Инвариант | Чем проверен |
|---|---|---|
| 6 | Resume точен и не переигрывает первое сообщение | `resume_uses_exact_provider_identity_and_never_repeats_the_initial_prompt` |
| 7 | Claude обязан иметь UUID, Codex — нет | `sessions_must_reference_a_project_and_carry_a_well_formed_provider_id` |
| 8 | Флаги идут перед `--` | `enhanced_launch_passes_model_and_settings_before_a_literal_prompt` |
| 9 | Env чистится от маркеров родителя | `agent_environment_removes_parent_conversation_markers_without_losing_login_configuration` |
| 10 | POSIX-запуск сохраняет метасимволы | `posix_launch_preserves_shell_metacharacters_as_literal_arguments` — реально гоняет `bash` и сверяет argv |
| 11 | Профиль изолирует конфиг и чистит API-ключи | `account_profiles_use_separate_homes_and_reject_provider_mismatches`; окно: «each profile gets its own provider home» |
| 12 | Запущенную сессию нельзя архивировать | `running_sessions_cannot_be_archived_or_rebound_but_notes_and_pinning_can_change` |

## Задачи, спеки, очередь

| # | Инвариант | Чем проверен |
|---|---|---|
| 13 | Код 0 → `review`, не `done` | `a_clean_exit_asks_for_review_and_never_marks_work_done`; окно: «a clean exit reaches review, not done» |
| 14 | Правка спеки инкрементирует ревизию и снимает одобрение | `spec_approval_gates_task_preparation_and_edits_invalidate_completed_work` |
| 15 | Запуск требует одобренной ревизии | там же; окно: «an unapproved specification blocks preparation» |
| 16 | Падение ставит очередь на паузу | `queue_ui::after_exit` останавливает очередь на любом не-чистом выходе; `the_queue_runs_one_task_at_a_time_and_stops_when_empty` |
| 17 | Бриф ограничен 32 000 символов | `prepare_task` проверяет до создания сессии (UTF-16) |
| 18 | Смена режима инвалидирует бриф | `publication_and_review_choices_are_explicit_and_changing_them_invalidates_the_old_brief` |

## Git

| # | Инвариант | Чем проверен |
|---|---|---|
| 19 | Вызовы git обеззараживают окружение | `Git::environment` удаляет шесть `GIT_*`; `git_panel_handles_unborn_repositories_spaced_paths_stage_commit_and_diffs` |
| 20 | Discard hunk сверяет sha256 | `discard_hunk_rejects_stale_diffs_and_preserves_other_hunks`; окно: «the diff is hashed for a later hunk discard» |
| 21 | Worktree удаляется только чистым, ветка сохраняется | `worktrees_belong_to_fresh_sessions_and_only_this_app_may_remove_them`, `real_worktrees_isolate_changes_reject_invalid_branches_and_refuse_dirty_removal` |
| 22 | Shared paths не перезаписывают и не идут по симлинкам | `shared_file_setup_never_follows_destination_symlinks_or_overwrites_existing_files` |

## Файлы и превью

| # | Инвариант | Чем проверен |
|---|---|---|
| 23 | Чтение ограничено и проверено | `preview_rejects_traversal_external_symlinks_binary_and_oversized_files`, `traversal_and_absolute_paths_are_refused` |
| 24 | В корзину только неотслеживаемые файлы | `files_ui::trash` сверяется со снимком; `trash_resolves_the_parent_not_the_file` |

## Терминал, история, телеметрия

| # | Инвариант | Чем проверен |
|---|---|---|
| 25 | Вставка не нажимает Enter без согласия | `feedback_paste_strips_control_sequences_and_does_not_press_enter`; VTE: «bracketed paste — 43 байта дословно, без Enter» |
| 26 | Сохранённый вывод ограничен и не покидает каталог | `snapshots_remain_bounded_plain_text_and_ids_cannot_escape_the_history_directory` |
| 27 | Телеметрия хранит только валидные окна квот | `telemetry_stores_only_status_and_validated_quota_windows`, `expired_and_missing_quotas_are_unavailable_never_zero` |
| 28 | Codex usage не делает модельных запросов | `performs_only_initialization_and_rate_limit_read` — сверяет ровно три метода |
| 29 | Гибернация только после явного завершения хода | `a_stale_or_repeated_report_is_ignored`; окно: «hibernation waits for a finished turn» |
| 30 | Остановка убивает группу процессов | VTE: «killpg — group stopped, grandchild reaped» |

## Общие

| # | Инвариант | Чем проверен |
|---|---|---|
| 31 | Деструктивные действия подтверждаются | `confirm_and_mutate` требует подтверждения для discard, discardHunk, revert, reset и amend; для trash и remove-worktree — отдельные диалоги. **Вручную** |
| 32 | Журнал ограничен 200 записями | `activity_stays_bounded_and_persisted`, `activity_is_capped_and_must_point_at_a_live_session` |
| 33 | Одновременная работа над репозиторием блокируется | `FilesView::busy` + `App::busy`; отпускается и на ошибке. **Вручную** |
| 34 | Данные превью отделены от нативного приложения | `layout_matches_the_electron_preview`, `config_root_follows_the_xdg_variable_only_when_absolute` |

## Что осталось на живую проверку

Главное: **приложением никто не пользовался.** Всё перечисленное выше —
автоматика без дисплея. Окно ни разу не открывали на настоящем экране, сессию
руками не запускали, на боевом `workspace.json` не гоняли.

- **31** и **33** — диалоги подтверждения и блокировка репозитория проверены
  чтением кода, автоматического теста нет.
- Репортинг мыши и перерисовка под нагрузкой в VTE — для этого есть
  `convoy-vte-probe`, но интерактивно он не запускался.
- Установка собранного пакета на чистой системе.
