# 09. Сборка и упаковка

## Зависимости сборки

```
rust >= 1.90        (на машине 1.98.1)
gtk4 >= 4.14        (4.22.5)
libadwaita >= 1.5   (1.9.4)   — нужен adw::Dialog, adw::AlertDialog
vte4 >= 0.76        (0.84.1)  — pacman -S vte4
gtksourceview5      (5.20.0)
blueprint-compiler  (если разметка на .blp)
meson или чистый cargo + build.rs для gresource
```

Крейты — версии проверить на старте M0, они идут общим release train
с gtk4-rs:

```toml
gtk4 = { version = "0.10", package = "gtk4", features = ["v4_14"] }
libadwaita = { version = "0.8", features = ["v1_5"] }
vte4 = "0.9"
sourceview5 = "0.10"
gio, glib, pango          — транзитивно из gtk4
serde, serde_json
pulldown-cmark            — markdown-превью
sha2                      — sha256 для имён файлов истории и профилей
uuid = { features = ["v4"] }
rustix или nix            — killpg
tempfile                  — тесты и discardHunk
regex
```

Версии `gtk4` ↔ `libadwaita` ↔ `vte4` ↔ `sourceview5` должны быть из одного
поколения биндингов, иначе типы не сойдутся. Это первое, что проверяется в M0.

## ⚠ Flatpak здесь — плохая идея

Сначала кажется очевидным выбором: GNOME runtime уже содержит GTK4,
libadwaita и VTE, пакет вышел бы в единицы мегабайт.

Но Convoy по своей природе:

1. **Запускает CLI с хоста** (`claude`, `codex`, `git`, `gh`) — внутри
   песочницы их нет. Потребуется `flatpak-spawn --host` для каждого запуска,
   что ломает `launchSpec()` и всю логику env.
2. **Читает и пишет произвольные каталоги с кодом** — нужен
   `--filesystem=host`, то есть песочница отключается.
3. **Пишет hooks для Claude**, где `command` — путь к бинарнику. Claude
   запускается на хосте и не увидит путь внутри песочницы. Пришлось бы
   ставить хостовый шим.
4. **Читает `~/.claude` и `~/.codex`** — снова хост.

Итог: Flatpak с `--filesystem=host --talk-name=org.freedesktop.Flatpak`
формально возможен, но песочница становится декоративной, а сложность
интеграции растёт втрое. **Не делаем.**

## Что делаем вместо

### 1. AUR (основной способ для себя)

```
convoy-linux-git     PKGBUILD из git, для разработки
convoy-linux         из тега, когда стабилизируется
```

Зависимости: `gtk4 libadwaita vte4 gtksourceview5`.
Makedepends: `rust blueprint-compiler`.
Ставит бинарник в `/usr/bin/convoy`, `.desktop` в
`/usr/share/applications/com.iftech.convoy.linux.desktop`, иконки в
`/usr/share/icons/hicolor/`, metainfo в `/usr/share/metainfo/`.

Это основной путь: машина на Arch, накладных расходов ноль.

### 2. `.deb` и `.rpm` (для раздачи)

`cargo-deb` и `cargo-generate-rpm` — обе умеют собирать из метаданных Cargo.
Зависимости указываются на системные библиотеки, бинарник динамически
линкуется. Размер пакета — единицы мегабайт.

Риск: версии GTK4/libadwaita/VTE в Debian stable и Ubuntu LTS могут быть
старше требуемых. Проверить до обещаний: `adw::Dialog` требует libadwaita 1.5
(GNOME 46, весна 2024). Ubuntu 24.04 — libadwaita 1.5, подходит.
Debian 12 — 1.2, **не подходит**; для Debian нужен либо бэкпорт, либо
AppImage.

### 3. AppImage (запасной вариант)

Собирается через `linuxdeploy` + `linuxdeploy-plugin-gtk`. Тянет внутрь
GTK4, libadwaita, VTE, gdk-pixbuf, GSettings-схемы и иконки — получится
40–70 МБ. Всё равно вдвое меньше текущих 122 МБ, но заметно больше
нативного пакета.

Делать только если понадобится раздавать людям на старых дистрибутивах.

## Обязательные файлы

**`.desktop`** — нужен не только для меню, но и для доставки уведомлений:
GNotification требует, чтобы application id совпадал с именем desktop-файла.

```ini
[Desktop Entry]
Name=Convoy
Comment=Workspace for Claude Code and Codex sessions
Exec=convoy
Icon=com.iftech.convoy.linux
Terminal=false
Type=Application
Categories=Development;IDE;
StartupNotify=true
StartupWMClass=com.iftech.convoy.linux
```

**Application id.** Electron-версия использует `com.iftech.convoy.desktop`.
Новый порт должен взять **другой** id — `com.iftech.convoy.linux` — иначе
D-Bus single-instance сочтёт их одним приложением, и запуск порта будет
поднимать окно Electron-версии. Это нужно именно потому, что обе версии
какое-то время сосуществуют.

**AppStream metainfo** — нужен, если пакет пойдёт в репозитории;
для личного использования можно отложить.

**GSettings-схема** — не нужна, настройки живут в `workspace.json`.

## Сборка

```sh
cd apps/linux
cargo build --release
./target/release/convoy
```

Ресурсы (Blueprint → `.ui` → `gresource`) собираются в `build.rs`:

```rust
// build.rs
blueprint_compiler::compile("src/ui/*.blp", "target/ui/");
glib_build_tools::compile_resources(&["target/ui"], "convoy.gresource.xml", "convoy.gresource");
```

Альтернатива — meson, как принято в GNOME-проектах. Для одного разработчика
чистый cargo проще; meson добавлять только если пойдёт в дистрибутивы.

## CI

Новый workflow `.github/workflows/linux.yml`, фильтр по `apps/linux/**`.
Существующий `desktop.yml` (Windows + Ubuntu для Electron) **не трогаем** —
он продолжает собирать Windows-превью.

```yaml
runs-on: ubuntu-24.04     # libadwaita 1.5, gtk4 4.14
steps:
  - apt install libgtk-4-dev libadwaita-1-dev libvte-2.91-gtk4-dev
                libgtksourceview-5-dev blueprint-compiler
  - cargo fmt --check
  - cargo clippy -- -D warnings
  - cargo test -p convoy-core             # без дисплея
  - xvfb-run -a cargo test -p convoy-gtk  # smoke
  - cargo build --release
  - cargo deb                             # артефакт
```

Ubuntu 24.04 — минимальная версия раннера с подходящим libadwaita.
Сборка `.rpm` и AppImage — отдельными джобами, когда понадобится.

## Размер результата

Ожидания:

```
бинарник (release, strip)        8–15 МБ
.deb / AUR пакет                 3–6 МБ
AppImage (если понадобится)      40–70 МБ
                        против   122 МБ сейчас
```

Бинарник крупнее «типичного Rust-приложения» из-за биндингов и встроенных
ресурсов, но всё это динамически линкуется с системными GTK/VTE, которые
на машине уже есть.
