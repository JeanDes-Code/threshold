#!/bin/bash
# Headless QA harness for Threshold. No window on the real desktop, no
# Hyprland output, no clipboard, no real input.
#   QA_OUT=<folder outside the repository> scripts/qa/headless.sh start
#   QA_OUT=<same folder> scripts/qa/headless.sh stop    (stops only what start started)
# Then drive the app with scripts/qa/wd.py (WebDriver on 127.0.0.1:4455):
# `wd.py new` launches target/debug/threshold inside the Xvfb display.
set -u
REPO=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${QA_OUT:?set QA_OUT to a run folder outside the repository}
case "$OUT" in "$REPO"/*) echo "QA_OUT must be outside the repository"; exit 1;; esac
DISP=:97
PORT=4455
mkdir -p "$OUT"

start() {
  avail=$(free -g | awk '/^Mem:/ {print $7}')
  [ "$avail" -lt 10 ] && { echo "only ${avail} GB of RAM available; not starting"; exit 1; }
  [ -x "$REPO/target/debug/threshold" ] || { echo "build the app first (cargo build), then start again"; exit 1; }

  Xvfb $DISP -screen 0 1600x1000x24 -nolisten tcp >"$OUT/xvfb.log" 2>&1 &
  echo $! >"$OUT/xvfb.pid"
  sleep 1

  (cd "$REPO" && npm --prefix ui run dev >"$OUT/vite.log" 2>&1 & echo $! >"$OUT/vite.pid")

  # The driver starts the app, and the app inherits this environment:
  # X11 only (no Wayland socket), software rendering inside Xvfb.
  env -u WAYLAND_DISPLAY DISPLAY=$DISP GDK_BACKEND=x11 \
      WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
      LIBGL_ALWAYS_SOFTWARE=1 \
      TAURI_WEBVIEW_AUTOMATION=true TAURI_AUTOMATION=true \
      WebKitWebDriver --port=$PORT --host=127.0.0.1 >"$OUT/wdriver.log" 2>&1 &
  echo $! >"$OUT/wdriver.pid"
  sleep 2
  echo "started: Xvfb $DISP, vite, WebKitWebDriver :$PORT, files in $OUT"
}

stop() {
  for f in wdriver vite xvfb; do
    [ -f "$OUT/$f.pid" ] && { pkill -P "$(cat "$OUT/$f.pid")" 2>/dev/null; kill "$(cat "$OUT/$f.pid")" 2>/dev/null; rm -f "$OUT/$f.pid"; }
  done
  echo "stopped"
}

case "${1:-}" in start) start;; stop) stop;; *) sed -n 2,8p "$0";; esac
