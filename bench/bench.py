#!/usr/bin/env python3
# Language shootout — pure Python (no numpy on this box).
#   python3 bench/bench.py rollout [steps] | python3 bench/bench.py mlp [iters]
# Identical algorithm + LCG to bench.rs/.go/.js => checksum must match.
# Idiomatic lists (what you'd actually write first). numpy would help the mlp
# matmul (BLAS) but NOT the rollout (branchy scalar) — see bench/README.md.

import sys
import time

N = 14
BOARD = N * N
F = 52
H = 128
MASK64 = (1 << 64) - 1


class Lcg:
    __slots__ = ("s",)

    def __init__(self, seed):
        self.s = seed & MASK64

    def next_u32(self):
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & MASK64
        return self.s >> 33

    def unit(self):
        return self.next_u32() / 4294967296.0 * 2.0 - 1.0


DIRS = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


def rollout(steps):
    board = [1 if i % 5 == 0 else 0 for i in range(BOARD)]
    rng = Lcg(0x12345678)
    total = 0
    for _ in range(steps):
        moves = []
        ap = moves.append
        for idx in range(BOARD):
            if board[idx] == 0:
                continue
            r = idx // N
            c = idx % N
            for dr, dc in DIRS:
                nr = r + dr
                nc = c + dc
                while 0 <= nr < N and 0 <= nc < N:
                    ni = nr * N + nc
                    if board[ni] != 0:
                        ap((idx << 8) | ni)
                        break
                    ap((idx << 8) | ni)
                    nr += dr
                    nc += dc
        total += len(moves)
        if moves:
            m = moves[rng.next_u32() % len(moves)]
            frm = m >> 8
            to = m & 0xFF
            board[to], board[frm] = board[frm], board[to]
    return total


def mlp(iters):
    rng = Lcg(0x9E3779B9)
    w1 = [rng.unit() * 0.1 for _ in range(H * F)]
    b1 = [rng.unit() * 0.1 for _ in range(H)]
    w2 = [rng.unit() * 0.1 for _ in range(H * H)]
    b2 = [rng.unit() * 0.1 for _ in range(H)]
    w3 = [rng.unit() * 0.1 for _ in range(4 * H)]
    b3 = [rng.unit() * 0.1 for _ in range(4)]
    x = [rng.unit() for _ in range(F)]
    h1 = [0.0] * H
    h2 = [0.0] * H
    out = [0.0] * 4
    acc = 0.0
    for n in range(iters):
        for o in range(H):
            s = b1[o]
            row = o * F
            for j in range(F):
                s += w1[row + j] * x[j]
            h1[o] = s if s > 0.0 else 0.0
        for o in range(H):
            s = b2[o]
            row = o * H
            for j in range(H):
                s += w2[row + j] * h1[j]
            h2[o] = s if s > 0.0 else 0.0
        for o in range(4):
            s = b3[o]
            row = o * H
            for j in range(H):
                s += w3[row + j] * h2[j]
            out[o] = s
        acc += out[0] + out[1] + out[2] + out[3]
        x[n % F] += 1e-6 * (out[0] - out[1])
    return acc


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "rollout"
    cnt = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    t = time.perf_counter()
    if which == "rollout":
        steps = cnt or 50000
        chk = rollout(steps)
        print(f"py   rollout  steps={steps} checksum={chk} time={time.perf_counter()-t:.3f}s", file=sys.stderr)
    elif which == "mlp":
        iters = cnt or 200000
        chk = mlp(iters)
        print(f"py   mlp      iters={iters} checksum={chk:.6f} time={time.perf_counter()-t:.3f}s", file=sys.stderr)
    else:
        print("usage: bench.py rollout|mlp [count]", file=sys.stderr)


if __name__ == "__main__":
    main()
