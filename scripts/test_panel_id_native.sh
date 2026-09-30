#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$(mktemp -t aimonitor-panel-id.XXXXXX)"
trap 'unlink "$OUT"' EXIT
g++ -std=c++17 -Wall -Wextra -I"$ROOT/src" \
  "$ROOT/tests/panel_id_native.cpp" "$ROOT/src/panel_id.cpp" -o "$OUT"
"$OUT" "$@"
