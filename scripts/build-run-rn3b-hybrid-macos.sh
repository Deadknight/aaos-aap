#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  ./scripts/docker-build.sh
fi

./scripts/run-rn3b-hybrid-bridge-android.sh
