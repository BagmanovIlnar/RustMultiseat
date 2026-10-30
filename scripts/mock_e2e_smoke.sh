#!/usr/bin/env bash
# One-command mock E2E: build → daemon → ctl smoke [--with-agent] → teardown.
# Uses an ephemeral TCP port so it works on macOS/Linux/Windows (Git Bash / MSYS).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WITH_AGENT=0
KEEP_LOGS=0
for arg in "$@"; do
  case "$arg" in
    --with-agent) WITH_AGENT=1 ;;
    --keep-logs) KEEP_LOGS=1 ;;
    -h|--help)
      echo "Usage: $0 [--with-agent] [--keep-logs]"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      exit 2
      ;;
  esac
done

# Free TCP port via Python (available on CI + macOS).
PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
ADDR="127.0.0.1:${PORT}"
DATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/multiseat-smoke.XXXXXX")"
LOG_DIR="${DATA_DIR}/logs"
mkdir -p "$LOG_DIR"
DAEMON_LOG="${LOG_DIR}/daemon.log"

cleanup() {
  if [[ -n "${DAEMON_PID:-}" ]] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ "$KEEP_LOGS" -eq 1 ]]; then
    echo "logs kept under ${DATA_DIR}"
  else
    rm -rf "$DATA_DIR"
  fi
}
trap cleanup EXIT

echo "==> build multiseat-daemon / multiseat-ctl"
cargo build -q -p multiseat-daemon --bins

DAEMON_BIN="${CARGO_TARGET_DIR:-target}/debug/multiseat-daemon"
CTL_BIN="${CARGO_TARGET_DIR:-target}/debug/multiseat-ctl"
# Windows cargo may emit .exe
if [[ ! -x "$DAEMON_BIN" && -x "${DAEMON_BIN}.exe" ]]; then
  DAEMON_BIN="${DAEMON_BIN}.exe"
  CTL_BIN="${CTL_BIN}.exe"
fi

echo "==> start daemon --bind ${ADDR} --data-dir ${DATA_DIR}"
"$DAEMON_BIN" --bind "$ADDR" --data-dir "$DATA_DIR" >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

echo "==> wait for daemon"
READY=0
for _ in $(seq 1 100); do
  if "$CTL_BIN" --addr "$ADDR" hello >/dev/null 2>&1; then
    READY=1
    break
  fi
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "daemon exited early; log:" >&2
    cat "$DAEMON_LOG" >&2 || true
    exit 1
  fi
  sleep 0.05
done
if [[ "$READY" -ne 1 ]]; then
  echo "daemon did not become ready at ${ADDR}" >&2
  cat "$DAEMON_LOG" >&2 || true
  exit 1
fi

SMOKE_ARGS=(--addr "$ADDR" smoke)
if [[ "$WITH_AGENT" -eq 1 ]]; then
  SMOKE_ARGS+=(--with-agent)
fi

echo "==> ctl ${SMOKE_ARGS[*]}"
"$CTL_BIN" "${SMOKE_ARGS[@]}"
echo "==> mock E2E smoke OK"
