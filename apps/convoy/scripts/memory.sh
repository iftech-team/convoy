#!/usr/bin/env bash
# What the client costs, for comparison with the Electron preview's 745 MB
# across seven processes and the GTK client's 167 MB in one.
#
# Tauri is not one process: the web view runs its own network and rendering
# processes, as every browser engine does. They are all counted.
set -uo pipefail

cd "$(dirname "$0")/.."
binary=${BINARY:-src-tauri/target/debug/convoy-tauri}
display=${DISPLAY_NUMBER:-:80}
wait_for=${WAIT:-12}

root=$(mktemp -d)
export XDG_CONFIG_HOME="$root/config"
storage="$XDG_CONFIG_HOME/Convoy Desktop Preview"
mkdir -p "$storage" "$root/project"
(cd ../linux && cargo run -q -p convoy-core --example emit -- \
  "$storage/workspace.json" "$root/project" >/dev/null 2>&1)

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

main=$(pgrep -x convoy-tauri -U "$(id -u)" | head -1)
if [ -z "$main" ]; then
  echo "the app was not running long enough to measure"
  tail -10 "$root/app.log"
  kill ${vite:+"$vite"} "$xvfb" 2>/dev/null
  rm -rf "$root"
  exit 1
fi

# Walk the real descendant tree from /proc. `pstree` prints thread ids too,
# and counting those would inflate the total several times over.
python3 - "$main" <<'MEASURE'
import os, sys

root = int(sys.argv[1])
parents = {}
for entry in os.listdir("/proc"):
    if not entry.isdigit():
        continue
    try:
        with open(f"/proc/{entry}/stat") as handle:
            fields = handle.read().rsplit(") ", 1)[1].split()
        parents[int(entry)] = int(fields[1])
    except OSError:
        pass

tree = {root}
changed = True
while changed:
    changed = False
    for pid, parent in parents.items():
        if parent in tree and pid not in tree:
            tree.add(pid)
            changed = True

def field(path, key):
    try:
        with open(path) as handle:
            for line in handle:
                if line.startswith(key):
                    return int(line.split()[1])
    except OSError:
        pass
    return 0

def total(path, key):
    value = 0
    try:
        with open(path) as handle:
            for line in handle:
                if line.startswith(key):
                    value += int(line.split()[1])
    except OSError:
        pass
    return value

rss = sum(field(f"/proc/{pid}/status", "VmRSS:") for pid in tree)
pss = sum(total(f"/proc/{pid}/smaps_rollup", "Pss:") for pid in tree)
names = []
for pid in sorted(tree):
    try:
        names.append(open(f"/proc/{pid}/comm").read().strip())
    except OSError:
        pass

print(f"processes : {len(tree)}  ({', '.join(sorted(set(names)))})")
print(f"RSS       : {rss // 1024} MB")
if pss:
    print(f"PSS       : {pss // 1024} MB")
MEASURE
printf 'binary    : %s (%s build)\n' "$(du -h "$binary" | cut -f1)" \
  "$([ "${binary#*release}" != "$binary" ] && echo release || echo debug)"

kill "$appproc" 2>/dev/null
sleep 1
kill ${vite:+"$vite"} "$xvfb" 2>/dev/null
wait "$appproc" ${vite:+"$vite"} "$xvfb" 2>/dev/null
rm -rf "$root"
