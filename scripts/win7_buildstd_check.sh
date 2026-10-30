#!/usr/bin/env bash
# Этап 1 (agent.md §2/§9): локальная проверка сборки user-mode компонентов
# под Tier-3 target x86_64-win7-windows-msvc через `-Z build-std` (std из
# исходников). Соответствует CI-job `win7-buildstd`.
#
# `cargo check` не линкует и работает на любом хосте (macOS/Linux/Windows);
# полноценный `--release` build с получением .exe требует Windows + MSVC
# (см. CI). Полная сверка импортов — лаборатория на реальной Windows 7.
#
# Usage: scripts/win7_buildstd_check.sh [--build]
#   WIN7_TOOLCHAIN — переопределение закрепленного nightly (по умолчанию
#   nightly-2026-10-29, та же дата, что в .github/workflows/ci.yml).
set -euo pipefail

TOOLCHAIN="${WIN7_TOOLCHAIN:-nightly-2026-10-29}"
TARGET="x86_64-win7-windows-msvc"
CRATES="-p multiseat-core -p multiseat-ipc -p multiseat-platform-windows -p multiseat-daemon -p multiseat-session-agent"

cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "==> installing $TOOLCHAIN (+rust-src)"
# --no-self-update: rustup не должен падать на самостоятельном обновлении в CI/лабе.
rustup toolchain install "$TOOLCHAIN" --profile minimal --component rust-src --no-self-update

# -D warnings не должен применяться к std, собираемому из исходников.
export RUSTFLAGS=""

if [[ "${1:-}" == "--build" ]]; then
  echo "==> cargo build (release) for $TARGET"
  exec cargo "+$TOOLCHAIN" build --release -Z build-std --target "$TARGET" $CRATES
fi

echo "==> cargo check for $TARGET"
exec cargo "+$TOOLCHAIN" check -Z build-std --target "$TARGET" $CRATES
