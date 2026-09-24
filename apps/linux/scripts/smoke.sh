#!/usr/bin/env bash
# Headless start-up check: builds the app, gives it a seeded workspace in a
# throwaway config directory, and requires a clean start and a clean exit.
# Nothing here launches an agent or touches the real workspace.
set -euo pipefail

cd "$(dirname "$0")/.."
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

export XDG_CONFIG_HOME="$root/config"
storage="$XDG_CONFIG_HOME/Convoy Desktop Preview"
mkdir -p "$storage" "$root/project"

cargo build --bin convoy
cargo run -q -p convoy-core --example emit -- "$storage/workspace.json" "$root/project" >/dev/null

log="$root/stderr.log"
CONVOY_EXIT_AFTER_MS=4000 xvfb-run -a ./target/debug/convoy 2>"$log"
status=$?

# DRI3 and session-bus complaints are the display server's, not ours.
problems=$(grep -E 'CRITICAL|WARNING \*\*: .*(convoy|Convoy)|panicked|assertion' "$log" \
  | grep -vE 'Unable to acquire session bus|gtk-application-prefer-dark-theme' || true)

if [ -n "$problems" ]; then
  echo "smoke: the app complained on startup:"
  echo "$problems"
  exit 1
fi
if [ "$status" -ne 0 ]; then
  echo "smoke: exited with status $status"
  sed -n '1,40p' "$log"
  exit 1
fi
echo "smoke: started, presented a window and exited cleanly"
