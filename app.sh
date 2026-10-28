#!/usr/bin/env bash
# Convenience wrapper: HID proxy daemon + the Wine vendor app.
#
#   ./app.sh            start daemon and app
#   ./app.sh daemon     start only the daemon
#   ./app.sh app        start only the app
#   ./app.sh stop       stop app and daemon
#   ./app.sh status     show what is running
#   ./app.sh log        follow the capture log
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
LOG="$ROOT/bridge/hidrawd.log"
OUT="$ROOT/bridge/daemon.out"
PIDFILE="$ROOT/bridge/hidrawd.pid"
DAEMON="$ROOT/bridge/hidrawd.py"
APP_DIR="${PHANTOM_APP_DIR:-$ROOT/program_files_x86}"
# the vendor app is not committed; fall back to the sibling checkout
[[ -d "$APP_DIR" ]] || APP_DIR="$ROOT/../program_files_x86"
PREFIX="${WINEPREFIX:-$ROOT/wine_prefix}"
PORT=47820

find_node() {
    PYTHONPATH="$ROOT${PYTHONPATH:+:$PYTHONPATH}" \
        python3 -c 'import phantom; print(phantom.find_device() or "")' 2>/dev/null || true
}

daemon_pid() {
    if [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
        cat "$PIDFILE"; return 0
    fi
    pgrep -f '^python3? -u .*hidrawd\.py' | head -1
}

daemon_running() {
    [[ -n "$(daemon_pid)" ]]
}

daemon_kill_stale() {
    if [[ -n "$(daemon_pid)" ]]; then
        local pid; pid="$(daemon_pid)"
        kill "$pid" 2>/dev/null || true
        for _ in 1 2 3 4 5; do kill -0 "$pid" 2>/dev/null || break; sleep 0.2; done
    fi
    rm -f "$PIDFILE"
}

daemon_start() {
    if daemon_running; then
        echo "daemon already running (pid $(daemon_pid))"
        return
    fi
    daemon_kill_stale
    local dev
    dev="$(find_node)"
    if [[ -z "$dev" ]]; then
        echo "keyboard not found (VID 379a PID 0920, interface 1)" >&2
        echo "check: lsusb | grep 379a" >&2
        exit 1
    fi
    : > "$LOG"
    nohup python3 -u "$DAEMON" "$dev" "$PORT" "$LOG" > "$OUT" 2>&1 &
    echo $! > "$PIDFILE"
    sleep 0.6
    if daemon_running; then
        echo "daemon: $dev on 127.0.0.1:$PORT (pid $(daemon_pid))"
        echo "log:    $LOG"
    else
        echo "daemon failed to start:" >&2
        cat "$OUT" >&2
        exit 1
    fi
}

app_start() {
    if [[ ! -f "$APP_DIR/hidapi_real.dll" ]]; then
        echo "warning: $APP_DIR/hidapi_real.dll missing." >&2
        echo "         the app needs the proxy as hidapi.dll; original should be hidapi_real.dll" >&2
    fi
    if ! daemon_running; then
        echo "warning: daemon not running; the app will not see the device" >&2
    fi
    echo "launching app with WINEPREFIX=$PREFIX"
    ( cd "$APP_DIR" && WINEPREFIX="$PREFIX" nohup wine "PhantomS Software.exe" \
        > /tmp/phantom-app.out 2>&1 & )
    sleep 2
    pgrep -f 'PhantomS Software.exe' >/dev/null && echo "app running" || echo "app did not start (see /tmp/phantom-app.out)"
}

stop_all() {
    if daemon_running; then
        kill "$(daemon_pid)" 2>/dev/null || true
        rm -f "$PIDFILE"
        echo "daemon stopped"
    else
        echo "daemon not running"
    fi
    # wineserver must be told which prefix, otherwise it kills the default one
    WINEPREFIX="$PREFIX" wineserver -k 2>/dev/null || true
    for p in $(pgrep -f '^Z:.*PhantomS Software\.exe'); do kill "$p" 2>/dev/null || true; done
    sleep 1
    for p in $(pgrep -f '^Z:.*PhantomS Software\.exe'); do kill -9 "$p" 2>/dev/null || true; done
    echo "wine stopped"
}

status() {
    local dev
    dev="$(find_node)"
    echo "device : ${dev:-not found}"
    if daemon_running; then
        echo "daemon : running (pid $(daemon_pid))"
    else
        echo "daemon : stopped"
    fi
    if pgrep -f 'PhantomS Software.exe' >/dev/null; then
        echo "app    : running"
    else
        echo "app    : stopped"
    fi
    if [[ -f "$LOG" ]]; then
        echo "log    : $(wc -l < "$LOG") lines, $(stat -c%s "$LOG") bytes"
    fi
}

case "${1:-all}" in
    daemon) daemon_start ;;
    app)    app_start ;;
    all)    daemon_start; app_start ;;
    stop)   stop_all ;;
    status) status ;;
    log)    tail -f "$LOG" ;;
    *)      echo "usage: $0 [all|daemon|app|stop|status|log]" >&2; exit 2 ;;
esac
