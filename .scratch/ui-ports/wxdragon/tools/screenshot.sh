#!/usr/bin/env bash
# Captures a screenshot of the Question Desk window, and only that window.
#
# Usage: tools/screenshot.sh [-o OUT.png] [DEMO_ROUND...]
#
#   -o OUT.png    where to write the PNG (default: /tmp/question-desk-<time>.png)
#   QD_APPEARANCE=light|dark   start the app in that appearance (default: the system's)
#   DEMO_ROUND    arguments for `demo-client`, e.g. `all` or `grilling emoji`;
#                 without them, the window shows whatever is open (or the empty state)
#
# If the app is not running, the script builds and starts it, and stops it again
# afterwards. A running app from this checkout is reused and left running.
#
# wxDragon port: uses port 7461 and XDG_CONFIG_HOME=<repo>/.xdg, and finds the window by
# the app's process id, since other checkouts run binaries with the same name.
#
# The window id comes from CoreGraphics, not from System Events: the nono sandbox
# blocks Apple Events. The capture is `screencapture -l <window id>`; if no Question Desk
# window is found, the script fails rather than capture the screen. Where window capture
# is not permitted at all (screencapture: "could not create image from window"), an app
# the script started itself renders its own window instead (the `snapshot` feature).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT=7461
URL="http://127.0.0.1:$PORT/mcp"
APP="${SCREENSHOT_APP:-$ROOT/target/release/question-desk}"
# This copy keeps its config (port 7461) and window state in its own XDG dir.
export XDG_CONFIG_HOME="$ROOT/.xdg"
OUT="/tmp/question-desk-$(date +%Y%m%d-%H%M%S).png"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
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
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
let pid = Int(CommandLine.arguments[1]) ?? -1
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

APP_PID="$(pgrep -f "^$APP" | head -1 || true)"
if [[ -z "$APP_PID" ]]; then
  if server_up; then
    echo "screenshot: port $PORT is taken by something other than $APP" >&2
    exit 1
  fi
  (cd "$ROOT" && cargo build --release --bins --features snapshot --quiet)
  mkdir -p "$WORK/snap"
  QD_SNAPSHOT_DIR="$WORK/snap" "$APP" >"$WORK/app.log" 2>&1 &
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
if ! screencapture -x -o -l"$ID" "$OUT" 2>"$WORK/capture.log"; then
  if [[ ! -d "$WORK/snap" ]]; then
    grep -v sandbox_extension "$WORK/capture.log" >&2 || true
    echo "screenshot: screencapture failed; quit the running app so the script can start one that renders itself" >&2
    exit 1
  fi
  # Window capture is not permitted here: let the app render its own window.
  rm -f "$OUT"
  echo "$OUT" >"$WORK/snap/request"
  for _ in $(seq 1 100); do [[ -s "$OUT" ]] && break; sleep 0.1; done
fi
[[ -s "$OUT" ]] || { echo "screenshot: capture failed" >&2; exit 1; }
echo "$OUT"
