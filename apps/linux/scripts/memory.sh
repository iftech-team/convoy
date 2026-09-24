#!/usr/bin/env bash
# Measures what the client actually costs: one window and one session.
#
# Reports RSS and PSS. RSS double-counts pages shared with other GTK
# applications, so PSS is the fairer number on a desktop that already runs
# GNOME; both are printed.
set -euo pipefail

cd "$(dirname "$0")/.."
binary=target/release/convoy
[ -x "$binary" ] || { echo "build it first: cargo build --release --bin convoy"; exit 1; }

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
export XDG_CONFIG_HOME="$root/config"
storage="$XDG_CONFIG_HOME/Convoy Desktop Preview"
mkdir -p "$storage" "$root/project"
cargo run -q -p convoy-core --example emit -- "$storage/workspace.json" "$root/project" >/dev/null

# Xvfb has no GPU, so GTK falls back to a software Vulkan or GL stack that
# carries tens of megabytes of its own. `GSK_RENDERER` picks which one, and the
# difference between them is the renderer's cost, not the application's.
renderer=${GSK_RENDERER:-default}
export GSK_RENDERER=$renderer

# Long enough to open the window, build the tabs and settle.
CONVOY_EXIT_AFTER_MS=12000 xvfb-run -a "$binary" >/dev/null 2>&1 &
launcher=$!
sleep 6

# `-x convoy` matches the binary itself, not xvfb-run's wrapper shell.
pid=$(pgrep -x convoy -U "$(id -u)" | head -1 || true)
if [ -z "$pid" ]; then
  echo "the process was not running long enough to measure"
  wait "$launcher" || true
  exit 1
fi

processes=$(pgrep -x convoy -U "$(id -u)" | wc -l)
rss=$(awk '/^VmRSS:/ {print $2}' "/proc/$pid/status")
pss=$(awk '/^Pss:/ {total += $2} END {print total}' "/proc/$pid/smaps_rollup" 2>/dev/null || echo 0)

printf 'processes : %s\n' "$processes"
printf 'RSS       : %s MB\n' "$((rss / 1024))"
[ "$pss" -gt 0 ] && printf 'PSS       : %s MB\n' "$((pss / 1024))"
printf 'binary    : %s\n' "$(du -h "$binary" | cut -f1)"
printf 'renderer  : %s\n' "$renderer"

wait "$launcher" || true
