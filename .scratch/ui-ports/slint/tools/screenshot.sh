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
# Without Screen Recording permission (the nono sandbox), `screencapture` fails; then an app
# started by this script renders its own window (Window::take_snapshot, see
# src/ui/snapshot.rs). That image has no native title bar or menu bar.
#
# Slint port: the app runs with XDG_CONFIG_HOME=<repo>/.xdg (port 7463). Environment:
#   QUESTION_DESK_BIN         app binary (default <repo>/target/release/question-desk), e.g.
#                             the Qt build in <repo>/target-qt/release/question-desk
#   QUESTION_DESK_APPEARANCE  light | dark | system, passed on to the app
#   QUESTION_DESK_SHOT_DIALOG review|settings|cancel|submit|close|about: snapshot mode only;
#                             opens that page or dialog first and also writes OUT-dialog.png
#   QUESTION_DESK_SHOT_DELAY  seconds to wait for rounds before capturing (default 2)
#
# The window id comes from CoreGraphics, not from System Events: the nono sandbox
# blocks Apple Events. The capture is always `screencapture -l <window id>`; if no
# Question Desk window is found, the script fails rather than capture the screen.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
URL="http://127.0.0.1:7463/mcp"
BIN="${QUESTION_DESK_BIN:-$ROOT/target/release/question-desk}"
OUT="/tmp/question-desk-$(date +%Y%m%d-%H%M%S).png"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
fi
if [[ "${1:-}" == "-o" ]]; then
  OUT="$2"
  shift 2
fi

WORK="$(mktemp -d)"
SNAP="$WORK/snapshot.png"
STARTED=""
PIDS=()
cleanup() {
  for pid in ${PIDS[@]+"${PIDS[@]}"}; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$WORK"
}
trap cleanup EXIT

cat >"$WORK/winid.swift" <<'EOF'
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
// Only windows of the given process: other copies of the app may be running.
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

APP_PID="$(pgrep -f "^$BIN" | head -1 || true)"
if [[ -z "$APP_PID" ]]; then
  if server_up; then
    echo "screenshot: port 7463 is taken by something other than $BIN" >&2
    exit 1
  fi
  if [[ -z "${QUESTION_DESK_BIN:-}" ]]; then
    (cd "$ROOT" && cargo build --release --bins --quiet)
  fi
  STARTED=1
  XDG_CONFIG_HOME="$ROOT/.xdg" QUESTION_DESK_SNAPSHOT="$SNAP" "$BIN" >"$WORK/app.log" 2>&1 &
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
sleep "${QUESTION_DESK_SHOT_DELAY:-2}"
# screencapture warns "sandbox_extension_issue_file failed" inside nono; that is harmless.
rm -f "$OUT" "${OUT%.png}-dialog.png"
screencapture -x -o -l"$ID" "$OUT" 2>"$WORK/capture.log" || true
if [[ ! -s "$OUT" && -n "$STARTED" ]]; then
  # No Screen Recording permission (e.g. inside nono): let the app render its own window.
  printf '%s' "${QUESTION_DESK_SHOT_DIALOG:-}" >"$SNAP.request"
  for _ in $(seq 1 80); do [[ -s "$SNAP" ]] && break; sleep 0.1; done
  if [[ -s "$SNAP" ]]; then
    mv "$SNAP" "$OUT"
    [[ -s "$WORK/snapshot-dialog.png" ]] && mv "$WORK/snapshot-dialog.png" "${OUT%.png}-dialog.png"
  fi
fi
[[ -s "$OUT" ]] || {
  echo "screenshot: capture failed" >&2
  cat "$WORK/capture.log" >&2
  [[ -n "$STARTED" ]] && { grep -A4 "panicked" "$WORK/app.log" || tail -5 "$WORK/app.log"; } >&2
  exit 1
}
echo "$OUT"
