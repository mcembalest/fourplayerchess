#!/usr/bin/env python3
# numpy variant of the mlp kernel — run with bench/.venv/bin/python.
#   bench_np.py single [iters]      one leaf at a time (alpha-beta style)
#   bench_np.py batch  [iters] [B]  B leaves at once (MCTS-style batching)
# Shows the key nuance: numpy/BLAS only wins the value-net eval when you can
# BATCH evaluations. Single-sample numpy pays per-call overhead. numpy does
# NOT apply to the branchy `rollout` kernel at all.

import sys
import time
import numpy as np

F, H = 52, 128
MASK64 = (1 << 64) - 1


class Lcg:
    def __init__(self, seed):
        self.s = seed & MASK64

    def next_u32(self):
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & MASK64
        return self.s >> 33

    def unit(self):
        return self.next_u32() / 4294967296.0 * 2.0 - 1.0


def weights():
    rng = Lcg(0x9E3779B9)
    w1 = np.array([rng.unit() * 0.1 for _ in range(H * F)]).reshape(H, F)
    b1 = np.array([rng.unit() * 0.1 for _ in range(H)])
    w2 = np.array([rng.unit() * 0.1 for _ in range(H * H)]).reshape(H, H)
    b2 = np.array([rng.unit() * 0.1 for _ in range(H)])
    w3 = np.array([rng.unit() * 0.1 for _ in range(4 * H)]).reshape(4, H)
    b3 = np.array([rng.unit() * 0.1 for _ in range(4)])
    x = np.array([rng.unit() for _ in range(F)])
    return w1, b1, w2, b2, w3, b3, x


def single(iters):
    w1, b1, w2, b2, w3, b3, x = weights()
    acc = 0.0
    for n in range(iters):
        h1 = np.maximum(w1 @ x + b1, 0.0)
        h2 = np.maximum(w2 @ h1 + b2, 0.0)
        out = w3 @ h2 + b3
        acc += float(out.sum())
        x[n % F] += 1e-6 * (out[0] - out[1])
    return acc


def batch(iters, B):
    w1, b1, w2, b2, w3, b3, x = weights()
    X = np.tile(x, (B, 1))  # (B, F)
    acc = 0.0
    outer = max(iters // B, 1)
    for n in range(outer):
        H1 = np.maximum(X @ w1.T + b1, 0.0)      # (B, H)
        H2 = np.maximum(H1 @ w2.T + b2, 0.0)     # (B, H)
        O = H2 @ w3.T + b3                        # (B, 4)
        acc += float(O.sum())
        X[:, n % F] += 1e-6 * (O[:, 0] - O[:, 1])
    return acc, outer * B


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "single"
    iters = int(sys.argv[2]) if len(sys.argv) > 2 else 200000
    t = time.perf_counter()
    if mode == "single":
        chk = single(iters)
        dt = time.perf_counter() - t
        print(f"np-1 mlp      iters={iters} checksum={chk:.6f} time={dt:.3f}s", file=sys.stderr)
    elif mode == "batch":
        B = int(sys.argv[3]) if len(sys.argv) > 3 else 256
        chk, done = batch(iters, B)
        dt = time.perf_counter() - t
        print(f"np-B mlp      iters={done} (B={B}) checksum~{chk:.3f} time={dt:.3f}s", file=sys.stderr)
    else:
        print("usage: bench_np.py single|batch [iters] [B]", file=sys.stderr)


if __name__ == "__main__":
    main()
