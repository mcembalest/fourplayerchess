#!/usr/bin/env bash
# Build + run the language shootout. Single-threaded, one kernel at a time.
# Usage: bench/run.sh [rollout_steps] [mlp_iters] [smoke]
#   pass "smoke" as 3rd arg for a tiny correctness pass (checksums only).
set -e
cd "$(dirname "$0")"

STEPS="${1:-50000}"
ITERS="${2:-200000}"
MODE="${3:-bench}"
if [ "$MODE" = "smoke" ]; then STEPS=200; ITERS=500; fi

echo "building rust + go + c ..."
rustc -O bench.rs -o bench_rust
go build -o bench_go main.go
cc -O3 -o bench_c bench.c

# Python: prefer the uv venv (Python 3.12 + numpy); fall back to system python3.
PY="./.venv/bin/python"; [ -x "$PY" ] || PY="python3"
HAS_NP=$("$PY" -c "import numpy" 2>/dev/null && echo 1 || echo 0)
echo "python: $("$PY" --version 2>&1)  numpy=$HAS_NP"
echo

run() { # kernel count
  local k="$1" n="$2"
  echo "=== $k (count=$n) ==="
  ./bench_c    "$k" "$n"
  ./bench_rust "$k" "$n"
  ./bench_go   "$k" "$n"
  node         bench.js "$k" "$n"
  command -v bun >/dev/null && bun bench.js "$k" "$n"
  "$PY"        bench.py "$k" "$n"
}

run rollout "$STEPS"
echo "(numpy does not apply to rollout: branchy move-gen can't be vectorized)"
echo
run mlp "$ITERS"
if [ "$HAS_NP" = "1" ]; then
  "$PY" bench_np.py single "$ITERS"
  "$PY" bench_np.py batch  "$ITERS" 256
fi
echo
echo "NOTE: checksums within a kernel must match across languages (same work)."
echo "      numpy may differ in low bits (BLAS summation order) — that's fine."
