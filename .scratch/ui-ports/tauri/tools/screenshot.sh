#!/usr/bin/env bash
# Captures a screenshot of the Question Desk window, and only that window.
#
# Usage: tools/screenshot.sh [-o OUT.png] [DEMO_ROUND...]
#
#   -o OUT.png    where to write the PNG (default: /tmp/question-desk-<time>.png)
#   DEMO_ROUND    arguments for `demo-client`, e.g. `all` or `grilling emoji`;
#                 without them, the window shows whatever is open (or the empty state)
#
# If the app is not running, the script builds and starts it, and stops it again
# afterwards. A running app (e.g. the user's own) is reused and left running.
#
# This copy is the Tauri port: it uses port 7462 and the config in .xdg/ (via
# XDG_CONFIG_HOME), and only ever looks at the window of *this* copy's binary, found by
# PID, because other ports of the app run under the same process name.
# QUESTION_DESK_THEME=light|dark forces the appearance of a freshly started app.
# QUESTION_DESK_BIN=/path/to/question-desk runs another (pre-built) binary instead, e.g. the
# egui build for comparison.
#
# The window id comes from CoreGraphics, not from System Events: the nono sandbox
# blocks Apple Events. The capture is always `screencapture -l <window id>`; if no
# Question Desk window is found, the script fails rather than capture the screen.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT=7462
URL="http://127.0.0.1:$PORT/mcp"
BIN="${QUESTION_DESK_BIN:-$ROOT/target/release/question-desk}"
OUT="/tmp/question-desk-$(date +%Y%m%d-%H%M%S).png"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
fi
if [[ "${1:-}" == "-o" ]]; then
  OUT="$2"
  shift 2
fi

WORK="$(mktemp -d)"
PIDS=()
cleanup() {
  for pid in ${PIDS[@]+"${PIDS[@]}"}; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$WORK"
}
trap cleanup EXIT

cat >"$WORK/winid.swift" <<'EOF'
import CoreGraphics
let pid = Int(CommandLine.arguments[1]) ?? -1
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
if CommandLine.arguments.count > 2 {
    for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid { print(w) }
}
for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid
    && (w[kCGWindowLayer as String] as? Int) == 0 {
    print(w[kCGWindowNumber as String]!)
    break
}
EOF
swiftc -O -o "$WORK/winid" "$WORK/winid.swift" 2>"$WORK/swiftc.log" || {
  cat "$WORK/swiftc.log" >&2
  exit 1
}

server_up() { curl -s -o /dev/null -X POST "$URL"; }

APP_PID="$(pgrep -f "^$BIN\$" | head -1 || true)"
if [[ -z "$APP_PID" ]]; then
  if server_up; then
    echo "screenshot: port $PORT is taken by something other than $BIN" >&2
    exit 1
  fi
  [[ -n "${QUESTION_DESK_BIN:-}" ]] || (cd "$ROOT" && cargo build --release --bins --quiet)
  XDG_CONFIG_HOME="$ROOT/.xdg" "$BIN" >"$WORK/app.log" 2>&1 &
  APP_PID=$!
  PIDS+=($APP_PID)
  for _ in $(seq 1 100); do server_up && break; sleep 0.1; done
  server_up || { cat "$WORK/app.log" >&2; exit 1; }
fi

if [[ $# -gt 0 ]]; then
  (cd "$ROOT" && cargo build --release --bin demo-client --quiet)
  # The demo client blocks until its rounds are answered; it is stopped on exit.
  "$ROOT/target/release/demo-client" --url "$URL" "$@" >"$WORK/demo.log" 2>&1 &
  PIDS+=($!)
fi

ID=""
for _ in $(seq 1 50); do
  ID="$("$WORK/winid" "$APP_PID")"
  [[ -n "$ID" ]] && break
  sleep 0.2
done
if [[ -z "$ID" ]]; then
  echo "screenshot: no on-screen Question Desk window found" >&2
  exit 1
fi

# Give new rounds time to arrive and render.
sleep 2
# screencapture warns "sandbox_extension_issue_file failed" inside nono; that is harmless.
screencapture -x -o -l"$ID" "$OUT" 2>"$WORK/capture.log" || true
[[ -s "$OUT" ]] || { echo "screenshot: capture failed" >&2; "$WORK/winid" "$APP_PID" debug >&2; grep -v sandbox_extension "$WORK/capture.log" >&2; exit 1; }
echo "$OUT"
