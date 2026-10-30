#!/usr/bin/env bash
# Stage 0 lab checklist: optional mock daemon + ctl lab-checklist artifact.
# On Windows lab machines this captures WTS/display/input probe evidence.
# On CI / macOS it is expected to write a NO-GO report (tooling smoke).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LOCAL_ONLY=0
REQUIRE_GO=0
OUT=""
for arg in "$@"; do
  case "$arg" in
    --local-only) LOCAL_ONLY=1 ;;
    --require-go) REQUIRE_GO=1 ;;
    --out=*) OUT="${arg#--out=}" ;;
    -h|--help)
      echo "Usage: $0 [--local-only] [--require-go] [--out=DIR]"
      echo "  Also runs soft input E2E (MULTISEAT_INPUT_FILTER=1) → soft_input_e2e.json"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$OUT" ]]; then
  OUT="${ROOT}/artifacts/stage0-latest"
fi
mkdir -p "$OUT"

echo "==> verify input-filter packaging (INF ↔ vcxproj)"
bash "${ROOT}/scripts/verify_input_filter_package.sh"

echo "==> build multiseat-daemon / multiseat-ctl"
cargo build -q -p multiseat-daemon --bins

DAEMON_BIN="${CARGO_TARGET_DIR:-target}/debug/multiseat-daemon"
CTL_BIN="${CARGO_TARGET_DIR:-target}/debug/multiseat-ctl"
if [[ ! -x "$DAEMON_BIN" && -x "${DAEMON_BIN}.exe" ]]; then
  DAEMON_BIN="${DAEMON_BIN}.exe"
  CTL_BIN="${CTL_BIN}.exe"
fi

DAEMON_PID=""
DATA_DIR=""
cleanup() {
  if [[ -n "${DAEMON_PID}" ]] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ -n "${DATA_DIR}" && -d "${DATA_DIR}" ]]; then
    rm -rf "$DATA_DIR"
  fi
}
trap cleanup EXIT

CTL_ARGS=(lab-checklist --out "$OUT")
if [[ "$REQUIRE_GO" -eq 1 ]]; then
  CTL_ARGS+=(--require-go)
fi

if [[ "$LOCAL_ONLY" -eq 1 ]]; then
  CTL_ARGS+=(--local-only)
  echo "==> ctl ${CTL_ARGS[*]} (no daemon)"
  "$CTL_BIN" "${CTL_ARGS[@]}"
else
  PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
  ADDR="127.0.0.1:${PORT}"
  DATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/multiseat-stage0.XXXXXX")"
  echo "==> start daemon --bind ${ADDR}"
  "$DAEMON_BIN" --bind "$ADDR" --data-dir "$DATA_DIR" >"${OUT}/daemon.log" 2>&1 &
  DAEMON_PID=$!

  READY=0
  for _ in $(seq 1 100); do
    if "$CTL_BIN" --addr "$ADDR" hello >/dev/null 2>&1; then
      READY=1
      break
    fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
      echo "daemon exited early" >&2
      cat "${OUT}/daemon.log" >&2 || true
      exit 1
    fi
    sleep 0.05
  done
  if [[ "$READY" -ne 1 ]]; then
    echo "daemon not ready" >&2
    exit 1
  fi

  echo "==> ctl --addr ${ADDR} ${CTL_ARGS[*]}"
  "$CTL_BIN" --addr "$ADDR" "${CTL_ARGS[@]}"
fi

echo "==> install-plan into Stage 0 artifact dir"
"$CTL_BIN" install-plan --out "$OUT"

echo "==> soft input E2E (MULTISEAT_INPUT_FILTER=1) → soft_input_e2e.json"
MULTISEAT_INPUT_FILTER=1 "$CTL_BIN" input-soft-e2e --out "$OUT"

echo "==> Stage 0 artifact: ${OUT}/stage0.json + REPORT.md + native_poc.json + install-plan.* + soft_input_e2e.json"
# Tooling smoke: artifact must exist and declare NO-GO until lab PoC passes.
python3 - <<PY
import json, os, sys
path = "${OUT}/stage0.json"
with open(path) as f:
    art = json.load(f)
go = art["report"]["go"]
summary = art["report"]["summary"]
print("go=", go)
print("summary=", summary)
if not art["report"]["checks"]:
    sys.exit("empty checks")
plan_json = os.path.join("${OUT}", "install-plan.json")
plan_md = os.path.join("${OUT}", "INSTALL_PLAN.md")
if not os.path.isfile(plan_json) or not os.path.isfile(plan_md):
    sys.exit("missing install-plan artifacts")
with open(plan_json) as f:
    plan = json.load(f)
pkgs = {p["id"] for p in plan["packages"]}
if "InputFilter" not in pkgs:
    sys.exit("install-plan missing InputFilter package")
detail = next(p["detail"] for p in plan["packages"] if p["id"] == "InputFilter")
if "RustMultiseatKbFilter.inf" not in detail:
    sys.exit("InputFilter detail missing INF paths")
soft_path = os.path.join("${OUT}", "soft_input_e2e.json")
if not os.path.isfile(soft_path):
    sys.exit("missing soft_input_e2e.json")
with open(soft_path) as f:
    soft = json.load(f)
if not soft.get("ok"):
    sys.exit("soft_input_e2e not ok: " + soft.get("detail", "?"))
needed = {"preview", "apply_no_arm", "apply_arm", "ping", "fail_open"}
got = {s["name"] for s in soft.get("steps", [])}
if not needed.issubset(got):
    sys.exit("soft_input_e2e missing steps: " + str(needed - got))
# CI / mock hosts must remain NO-GO until Windows Stage 0 PoC lands.
if go:
    print("NOTE: unexpected GO on this host — verify lab evidence", file=sys.stderr)
print("soft_input_e2e ok soft=", soft.get("soft"))
print("stage0 checklist tooling OK")
PY
