#!/usr/bin/env bash
# Captures a screenshot of the Question Desk window, and only that window.
#
# Usage: tools/screenshot.sh [-o OUT.png] [-d DATA_DIR] [DEMO_ROUND...]
#
#   -o OUT.png    where to write the PNG (default: /tmp/question-desk-<time>.png)
#   -d DATA_DIR   XDG data home for a started app; its rounds are in
#                 DATA_DIR/question-desk/rounds/ (default: a fresh temp dir, so
#                 screenshots never touch the user's History)
#   DEMO_ROUND    arguments for `demo-client`, e.g. `all` or `grilling emoji`;
#                 without them, the window shows whatever is open (or the empty state)
#
# If the app is not running, the script builds and starts it, and stops it again
# afterwards. A running app (e.g. the user's own) is reused and left running.
#
# The window id comes from CoreGraphics, not from System Events: the nono sandbox
# blocks Apple Events. The capture is always `screencapture -l <window id>`; if no
# Question Desk window is found, the script fails rather than capture the screen.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
URL="http://127.0.0.1:7457/mcp"
OUT="/tmp/question-desk-$(date +%Y%m%d-%H%M%S).png"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
fi
DATA=""
while [[ "${1:-}" == "-o" || "${1:-}" == "-d" ]]; do
  case "$1" in
    -o) OUT="$2" ;;
    -d) DATA="$2" ;;
  esac
  shift 2
done

WORK="$(mktemp -d)"
DATA="${DATA:-$WORK/data}"
PIDS=()
cleanup() {
  for pid in ${PIDS[@]+"${PIDS[@]}"}; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$WORK"
}
trap cleanup EXIT

cat >"$WORK/winid.swift" <<'EOF'
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list where (w[kCGWindowOwnerName as String] as? String) == "question-desk"
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

if ! pgrep -f "target/release/question-desk" >/dev/null; then
  if server_up; then
    echo "screenshot: port 7457 is taken by something other than target/release/question-desk" >&2
    exit 1
  fi
  (cd "$ROOT" && cargo build --release --bins --quiet)
  # Fresh config and data dirs: the default port, and no rounds or `data_dir` of the user's.
  XDG_CONFIG_HOME="$WORK/config" XDG_DATA_HOME="$DATA" \
    "$ROOT/target/release/question-desk" >"$WORK/app.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 100); do server_up && break; sleep 0.1; done
  server_up || { cat "$WORK/app.log" >&2; exit 1; }
fi

if [[ $# -gt 0 ]]; then
  (cd "$ROOT" && cargo build --release --bin demo-client --quiet)
  # The demo client blocks until its rounds are answered; it is stopped on exit.
  "$ROOT/target/release/demo-client" "$@" >"$WORK/demo.log" 2>&1 &
  PIDS+=($!)
fi

ID=""
for _ in $(seq 1 50); do
  ID="$("$WORK/winid")"
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
screencapture -x -o -l"$ID" "$OUT" 2>/dev/null
[[ -s "$OUT" ]] || { echo "screenshot: capture failed" >&2; exit 1; }
echo "$OUT"
