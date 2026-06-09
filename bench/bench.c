// Language shootout — C. Build: cc -O3 -o bench_c bench.c
// Run: ./bench_c rollout [steps] | ./bench_c mlp [iters]
// Identical algorithm + LCG to bench.rs/.go/.js/.py => checksum must match.

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <time.h>

#define N 14
#define BOARD (N * N)
#define F 52
#define H 128

static uint64_t lcg_state;
static inline uint32_t next_u32(void) {
    lcg_state = lcg_state * 6364136223846793005ULL + 1442695040888963407ULL;
    return (uint32_t)(lcg_state >> 33);
}
static inline double unit(void) {
    return (double)next_u32() / 4294967296.0 * 2.0 - 1.0;
}

static uint64_t rollout(long steps) {
    const int dirs[8][2] = {{-1,-1},{-1,0},{-1,1},{0,-1},{0,1},{1,-1},{1,0},{1,1}};
    int32_t board[BOARD];
    for (int i = 0; i < BOARD; i++) board[i] = (i % 5 == 0) ? 1 : 0;
    lcg_state = 0x12345678ULL;
    int32_t *moves = malloc(sizeof(int32_t) * 8192);
    uint64_t total = 0;
    for (long st = 0; st < steps; st++) {
        int nm = 0;
        for (int idx = 0; idx < BOARD; idx++) {
            if (board[idx] == 0) continue;
            int r = idx / N, c = idx % N;
            for (int d = 0; d < 8; d++) {
                int dr = dirs[d][0], dc = dirs[d][1];
                int nr = r + dr, nc = c + dc;
                while (nr >= 0 && nr < N && nc >= 0 && nc < N) {
                    int ni = nr * N + nc;
                    if (board[ni] != 0) { moves[nm++] = (idx << 8) | ni; break; }
                    moves[nm++] = (idx << 8) | ni;
                    nr += dr; nc += dc;
                }
            }
        }
        total += (uint64_t)nm;
        if (nm > 0) {
            int32_t m = moves[next_u32() % (uint32_t)nm];
            int from = m >> 8, to = m & 0xFF;
            int32_t tmp = board[to]; board[to] = board[from]; board[from] = tmp;
        }
    }
    free(moves);
    return total;
}

static double mlp(long iters) {
    lcg_state = 0x9E3779B9ULL;
    static double w1[H*F], b1[H], w2[H*H], b2[H], w3[4*H], b3[4], x[F];
    for (int i = 0; i < H*F; i++) w1[i] = unit() * 0.1;
    for (int i = 0; i < H;   i++) b1[i] = unit() * 0.1;
    for (int i = 0; i < H*H; i++) w2[i] = unit() * 0.1;
    for (int i = 0; i < H;   i++) b2[i] = unit() * 0.1;
    for (int i = 0; i < 4*H; i++) w3[i] = unit() * 0.1;
    for (int i = 0; i < 4;   i++) b3[i] = unit() * 0.1;
    for (int i = 0; i < F;   i++) x[i]  = unit();
    double h1[H], h2[H], out[4], acc = 0.0;
    for (long n = 0; n < iters; n++) {
        for (int o = 0; o < H; o++) {
            double s = b1[o]; int row = o * F;
            for (int j = 0; j < F; j++) s += w1[row + j] * x[j];
            h1[o] = s > 0.0 ? s : 0.0;
        }
        for (int o = 0; o < H; o++) {
            double s = b2[o]; int row = o * H;
            for (int j = 0; j < H; j++) s += w2[row + j] * h1[j];
            h2[o] = s > 0.0 ? s : 0.0;
        }
        for (int o = 0; o < 4; o++) {
            double s = b3[o]; int row = o * H;
            for (int j = 0; j < H; j++) s += w3[row + j] * h2[j];
            out[o] = s;
        }
        acc += out[0] + out[1] + out[2] + out[3];
        x[n % F] += 1e-6 * (out[0] - out[1]);
    }
    return acc;
}

int main(int argc, char **argv) {
    const char *which = argc > 1 ? argv[1] : "rollout";
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    if (strcmp(which, "rollout") == 0) {
        long steps = argc > 2 ? atol(argv[2]) : 50000;
        uint64_t chk = rollout(steps);
        clock_gettime(CLOCK_MONOTONIC, &t1);
        double dt = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) / 1e9;
        fprintf(stderr, "c    rollout  steps=%ld checksum=%llu time=%.3fs\n",
                steps, (unsigned long long)chk, dt);
    } else if (strcmp(which, "mlp") == 0) {
        long iters = argc > 2 ? atol(argv[2]) : 200000;
        double chk = mlp(iters);
        clock_gettime(CLOCK_MONOTONIC, &t1);
        double dt = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) / 1e9;
        fprintf(stderr, "c    mlp      iters=%ld checksum=%.6f time=%.3fs\n", iters, chk, dt);
    } else {
        fprintf(stderr, "usage: bench_c rollout|mlp [count]\n");
    }
    return 0;
}
