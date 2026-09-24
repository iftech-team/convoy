#!/usr/bin/env bash
# Starts a real agent in the app's terminal and photographs the result.
#
# The session is bound to a throwaway account profile, so Claude Code writes
# its state into the temporary storage and the real one is left alone.
set -uo pipefail

cd "$(dirname "$0")/.."
binary=${BINARY:-src-tauri/target/debug/convoy-tauri}
out=${1:-/tmp/convoy-terminal.png}
display=${DISPLAY_NUMBER:-:79}

root=$(mktemp -d)
export XDG_CONFIG_HOME="$root/config"
storage="$XDG_CONFIG_HOME/Convoy Desktop Preview"
mkdir -p "$storage" "$root/project"
(cd ../linux && cargo run -q -p convoy-core --example emit -- \
  "$storage/workspace.json" "$root/project" >/dev/null 2>&1)

# Bind the first session to its own profile. `account_environment` then points
# CLAUDE_CONFIG_DIR at a hash under the temporary storage rather than ~/.claude.
python3 - "$storage/workspace.json" <<'PY'
import json, sys, uuid
path = sys.argv[1]
state = json.load(open(path))
profile = {"id": str(uuid.uuid4()), "label": "Probe", "agent": "claude"}
state.setdefault("profiles", []).append(profile)
for session in state["sessions"]:
    if session["agent"] == "claude":
        session["profileID"] = profile["id"]
        break
json.dump(state, open(path, "w"), indent=2)
PY

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
sleep 8

# The Start button of the first row. Coordinates rather than a selector: this
# is a photograph of the real window, not a DOM test.
DISPLAY=$display xdotool mousemove 1170 239 click 1
sleep 12

DISPLAY=$display import -window root "$out" 2>/dev/null

kill "$appproc" 2>/dev/null
sleep 1
kill ${vite:+"$vite"} "$xvfb" 2>/dev/null
wait "$appproc" ${vite:+"$vite"} "$xvfb" 2>/dev/null

if [ -s "$out" ]; then
  echo "shot: $out"
else
  echo "no screenshot; app log:"
  tail -20 "$root/app.log"
fi
rm -rf "$root"
