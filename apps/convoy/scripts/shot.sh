#!/usr/bin/env bash
# Runs the app headlessly against a seeded workspace and photographs it, so
# the design can be looked at without a display attached.
set -uo pipefail

cd "$(dirname "$0")/.."
binary=${BINARY:-src-tauri/target/debug/convoy-tauri}
out=${1:-/tmp/convoy-shot.png}
wait_for=${WAIT:-7}
display=${DISPLAY_NUMBER:-:77}

root=$(mktemp -d)
export XDG_CONFIG_HOME="$root/config"
storage="$XDG_CONFIG_HOME/Convoy Desktop Preview"
mkdir -p "$storage" "$root/project"
(cd ../linux && cargo run -q -p convoy-core --example emit -- \
  "$storage/workspace.json" "$root/project" >/dev/null 2>&1)

# Xvfb has no GPU; WebKit's accelerated compositing has nothing to talk to.
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export GDK_BACKEND=x11

# A debug build points at the dev server; a release build embeds the front end.
vite=""
if [ "${binary#*release}" = "$binary" ]; then
  npm run dev >"$root/vite.log" 2>&1 &
  vite=$!
  for _ in $(seq 1 60); do
    curl -sf http://localhost:1420 >/dev/null 2>&1 && break
    sleep 0.5
  done
fi

Xvfb "$display" -screen 0 1440x900x24 >/dev/null 2>&1 &
xvfb=$!
sleep 1

DISPLAY=$display "$binary" >"$root/app.log" 2>&1 &
appproc=$!
sleep "$wait_for"

DISPLAY=$display import -window root "$out" 2>/dev/null

# Signalling the recorded pids only. A pattern kill here would also match this
# script's own command line and take the shell with it.
kill "$appproc" 2>/dev/null
sleep 0.5
kill ${vite:+"$vite"} "$xvfb" 2>/dev/null
wait "$appproc" ${vite:+"$vite"} "$xvfb" 2>/dev/null

if [ -s "$out" ] && [ "$(stat -c %s "$out")" -gt 5000 ]; then
  echo "shot: $out"
else
  echo "no usable screenshot; app log:"
  tail -25 "$root/app.log"
  [ -f "$root/vite.log" ] && { echo "--- vite ---"; tail -8 "$root/vite.log"; }
fi
rm -rf "$root"
