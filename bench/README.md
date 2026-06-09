# Language shootout — is Rust justified for this project?

Goal: decide empirically whether the 4PC engine needs Rust, or whether a simpler
stack (TypeScript/Node, Go, or Python) is good enough. We compare the **simplest
idiomatic** implementation in each language — no SIMD, no hand-tuning — on two
kernels that bracket the real workload.

## The two kernels
1. **`rollout`** — init a 14×14 board, scan it, generate sliding moves in 8
   directions, pick one with an LCG, make the move; repeat. This is *branchy
   scalar code with dynamic allocation* — the shape of move-gen / self-play /
   tree search. **numpy/torch cannot vectorize this**, so it's where interpreted
   languages hurt most and the language/runtime itself is exposed.
2. **`mlp`** — the value-net forward pass (52→128→128→4, ReLU), evaluated **one
   sample at a time** (as in alpha-beta, where each leaf is scored individually).
   This is the eval cost. In a real Python build you'd reach for numpy/torch
   here — but per-call BLAS overhead is bad for single-sample eval; the win only
   appears when you can *batch* leaves (MCTS-style), which alpha-beta can't.

Same algorithm + same 64-bit LCG in every language ⇒ all print a **matching
checksum**. If the checksums differ, the implementations aren't doing equal work
and the timings are meaningless. (Validated: rollout=185851, mlp=-132.148604.)

All math is `f64` for cross-language comparability (Python/JS have no native
f32). The real engine uses `f32`, which would favor Rust/Go a bit more (SIMD +
half the memory traffic), so these numbers are, if anything, generous to the
interpreted/JIT'd langs on `mlp`.

## How to run
```
bench/run.sh                 # default: rollout 50000 steps, mlp 200000 iters
bench/run.sh 100000 400000   # custom counts
bench/run.sh - - smoke       # tiny correctness pass (checksums only)
```
Run it **single-threaded and uncontended** (nothing else heavy on the CPU) or the
numbers are noise.

## Results (single-thread, M3 Max, f64, 2026-06-08)

**`rollout`** — 50,000 steps, checksum 46360254 (all match). *This is the
self-play / search hot path.*

| runtime | time | vs Rust |
|---------|-----:|--------:|
| C (clang -O3) | 0.072s | **0.76× (fastest)** |
| Go      | 0.089s | 0.94× |
| Rust    | 0.095s | 1.0× |
| Bun (JSC) | 0.135s | 1.4× |
| Node (V8) | 0.153s | 1.6× |
| Python (3.12, pure) | 4.175s | **44×** |

**`mlp`** — 200,000 forwards, 52→128→128→4, checksum -52859.441731 (all match).
*This is the value-net eval.*

| runtime | time | vs Rust |
|---------|-----:|--------:|
| numpy **batched** (B=256) | 0.085s | **0.05× (19× faster)** |
| numpy single-sample | 1.379s | 0.83× (faster) |
| C (scalar) | 1.656s | ~tied |
| Rust (scalar) | 1.658s | 1.0× |
| Go (scalar) | 2.964s | 1.8× |
| Bun | 3.475s | 2.1× |
| Node | 3.513s | 2.1× |
| Python (pure) | 109.3s | 66× |

### Verdict
- **Proxy kernel:** raw game-sim *ceiling* is close — Go ties Rust, JS within
  1.4–1.7×, only pure Python disqualified (48×). **BUT the real engine tells a
  different story (see "Real-engine throughput" below): idiomatic `rules.js` is
  8–27× slower than `fpc-core`** because of allocation/GC. The proxy measured the
  language ceiling; the real engine measured idiomatic code as written.
- **The hand-coded scalar net forward is Rust's weak spot:** numpy-single already
  matches it and **batched numpy is 19× faster**. If eval is the bottleneck the
  answer is batched BLAS (numpy / MLX / torch), not scalar Rust. (A Rust BLAS/SIMD
  crate would close this, but that's not what the repo does today.)
- **Caveats:** f64 here; f32+SIMD would help Rust/Go a bit. numpy's batched win
  requires a *batchable* search (MCTS leaf-batching), not alpha-beta.

### Real-engine throughput (the decisive number)
Random self-play on the **actual** rules — `fpc-core` vs the production `rules.js`
(driven headlessly via vm, exactly like the oracle). Same ~500k plies of real
work (legal-move gen + make_move + draw bookkeeping), single-threaded:

| runtime | pos/sec | vs Rust | faithful? |
|---------|--------:|--------:|-----------|
| Rust (fpc-core)        | 90,935 | 1.0×  | — |
| C (port of fpc-core)   | 82,841 | 0.91× (≈tied) | ✅ exact 499967/1 |
| Go (port of fpc-core)  | 56,061 | 1.62× | ✅ exact 499967/1 |
| Bun (rules.js)         | 10,626 | 8.6×  | (diff RNG) |
| Node (rules.js)        | 3,309  | 27×   | (diff RNG) |

The C and Go ports use the **same splitmix64 RNG + identical move ordering** as the
Rust bin, so a faithful port reproduces Rust's exact `positions=499967 finished=1`
— both do, which proves the ports do equal work (not just "look fast"). Run:
```
cargo run -p fpc-train --release --bin throughput -- 2000 250   # Rust
cc -O3 -o bench/bench_engine_c bench/engine.c && ./bench/bench_engine_c 2000 250
go build -o bench/bench_goengine bench/goengine/main.go && ./bench/bench_goengine 2000 250
node bench/engine_throughput.mjs 2000 250   # or: bun ...
```

**Reading it:** C ≈ Rust (within ~10%). Go is 1.6× behind (GC + per-ply move-slice
& string-key allocation; the proxy's "Go ties Rust" held only for alloc-free code).
Idiomatic JS is 8.6–27× behind — overwhelmingly allocation/GC, since the *language
ceiling* (proxy) is ~1.5×. numpy never appears: it can't vectorize branchy move-gen
(but see MOVEGEN-VECTORIZATION.md for how youd actually vectorize this engine).

**This is 5–16× worse for JS than the proxy `rollout` predicted (1.7×).** The
gap is **allocation/GC**: `rules.js` clones a 14×14 array of fresh piece objects
for every pseudo-move legality check, plus per-move object literals, a `Set`, and
string-keyed repeat tables. `fpc-core` uses `Copy` types + fixed arrays = almost
no heap traffic. So the language *ceiling* is close (proxy: 1.7×), but **idiomatic
JS as actually written is 8–27× slower**. A flat-typed-array, make/unmake,
integer-move-encoded JS rewrite would recover most of that — but that's exactly
the work Rust gives you for free from idiomatic code.

### What actually argues for/against Rust here
- **Browser engine (shipped):** the game runs in-browser. JS runs there
  *natively* (no WASM build) at ~1.5× of Rust — for one bot move that's
  irrelevant. This argues **for TypeScript**, against the Rust→WASM toolchain
  (which was already a pain: the Homebrew/rustup wasm gotcha).
- **Offline training/arena (millions of positions, parallel):** wants all 14
  cores on shared state. Rust (rayon) and Go (goroutines) both parallelize
  easily; Node workers (no shared memory) and Python (GIL) are awkward. On the
  REAL engine Go is **1.62×** slower than Rust/C (GC + per-ply allocation), not
  tied — the proxy's "Go == Rust" held only for alloc-free code. So per-core Go
  trails by ~1.6×, but its goroutine ergonomics rival rayon.
- **Net training at scale:** batched numpy/MLX/torch is 19×+ and gets the GPU —
  a Python edge, not a Rust one.

## The other axis Rust was chosen for: parallelism
Self-play and the arena are *embarrassingly parallel* (independent games). The
single-thread number is only half the story — throughput = single-thread × cores
actually usable:

| lang | parallel story | verdict for this workload |
|------|----------------|---------------------------|
| Rust | `rayon` `into_par_iter` — one line, shared-memory, no GIL | trivial 14× |
| Go   | goroutines + channels — easy, GC pauses minor here | easy 14×, very ergonomic |
| Node/Bun | `worker_threads`, no shared memory by default (postMessage copies; SharedArrayBuffer is manual + clunky) | possible but painful for this |
| Python | GIL blocks threaded CPU parallelism; `multiprocessing` works but heavy (process spawn, pickling) | 14× only via multiprocessing, awkward |

So even if Node's single-thread `rollout` is within, say, 2–3× of Rust, the
parallel arena story still favors Rust/Go because we want all 14 cores on shared
game state with no ceremony.

## How to read the decision
- If **Rust ≈ Go ≈ Node** on `rollout` and all crush Python → the question is
  Rust-vs-Go-vs-TS on ergonomics + parallelism, not raw speed.
- If **Rust ≫ Node/Go** on `rollout` → the self-play/arena throughput (millions
  of positions) justifies the Rust cost directly.
- `mlp`: if pure Python is ~100× slower (expected), that's the case for *either*
  a compiled core *or* numpy/torch+batched-MCTS — but batching constrains the
  search design.
