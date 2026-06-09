# Vectorizing 4PC move generation — design report

Brainstorm output (separate agent, 2026-06-09). Goal: how to make move-gen much
faster. Measured baseline: random self-play single-thread Rust `fpc-core` = 90.9k
pos/s; idiomatic `rules.js` = 3.3k (Node) / 10.6k (Bun). The hot path
(`crates/fpc-core/src/movegen.rs:239` `legal_moves`) per ply is roughly
`O(pseudo × (196 board-copy + 196 find_king + 196 × ray-walk))` — board clone +
per-move full-board attack scan dominate.

## Tier 0 — cheap wins that kill the bottleneck (do first, no SIMD)
- **0a. make/unmake instead of clone-per-move — HIGHEST ROI.** `legal_moves`
  clones the whole board per pseudo-move (movegen.rs:242) — ~392 bytes for a
  ≤3-cell change. Save the 1–3 affected cells, apply in place, test king-safety,
  restore. Est. 2–4× on Rust alone, more under search. (`State::for_search`
  already exists.)
- **0b. Lazy legality.** A move is illegal only if the piece is pinned to the
  king, it's a king move, or the king is in check. Compute pins + checkers once
  per ply (rays from the king); then most pseudo-moves are legal with NO scan.
  Turns per-ply cost from `O(pseudo × 196 × ray)` into one `O(196×ray)` pass +
  `O(pseudo)` classification — the biggest *algorithmic* win (80–95% of moves
  skip the scan). 4-player wrinkle: pins/checks from up to 3 enemy colours;
  double-check is common; a block must block all checking rays. Bug-prone →
  validate with perft vs rules.js.
- **0c. Incremental king-square cache + piece lists.** `find_king` is a full
  196-scan inside every legality test; a `king_sq[4]` updated in make/unmake
  removes it. Piece lists shrink `pseudo_moves` and `attacked`.

**Combined Tier 0 ≈ 5–15× single-thread**, and is the prerequisite for any SIMD
(vectorizing a clone-per-move design just vectorizes waste). Plus: offline
self-play/arena is embarrassingly parallel across games → `rayon` across games is
near-linear and orthogonal (already done in arena/selfplay).

## Tier 1 — precomputation (make king-safety ~O(1))
- 1a. Precomputed knight/king/pawn target masks per square (corner-masked).
- 1b. Ray tables `RAY[sq][dir]` terminated at edges AND corner holes.
- 1c. "Is square attacked" via **superpiece sets**: enemy knight ∩ `KNIGHT[S]`,
  slider rays ∩ enemy (rook|queen)/(bishop|queen) + nearest-blocker check. Pawn
  attack test is per-colour (4 orientations) → `PAWN_ATTACKERS_OF[sq][color]`.
  Converts the 196-cell `attacked()` scan into a few mask-ANDs.

## Tier 2 — bitboards for 196 squares
The 8×8/one-u64 literature breaks: 196 squares need `[u64;4]` (portable, WASM-ok)
or `u64x4` SIMD. **Magic bitboards: not worth it** — corner holes destroy the
rank/file regularity and balloon tables. **Use Kogge-Stone parallel-prefix fills**
(~4 shift-OR steps/dir) with a `PLAYABLE` mask ANDed each step so fills die at
holes. Write+unit-test `shift_{n,s,e,w,diag}` once (cross-limb/cross-lane is the
whole battle). On bitboards, make/unmake is just XOR from/to bits — subsumes 0a.
4 pawn orientations → 4 push/cap shift dirs + 4 promo center-lines (bakeable).

## Tier 3 — batch positions for SIMD (struct-of-arrays)
Data-parallel across N positions beats within-position SIMD for the *uniform*
stages: batched leaf eval, draw/terminal detection, is-king-attacked (each lane =
one position). Move *enumeration* is divergent (variable move counts) → keep it
scalar/bitboard. This is the MCTS-leaf-batching pattern; restructure search to
collect a frontier then evaluate in a batch (`u64x4/u64x8`). Not for random
self-play (just thread across games).

## Tier 4 — GPU (Metal/MLX)
Skip for branchy movegen (control divergence, tiny 196-byte positions, variable
output). Reserve Metal/MLX for **batched NN inference** in the RL phase (the real
GPU workload). cf. the bench `mlp` result: batched numpy already 19× the scalar
forward.

## Final recommendation
1. **Highest ROI: Tier 0** (make/unmake + lazy legality + king cache) ≈ 5–15×,
   no SIMD, keeps WASM portable. Foundation for everything else.
2. **Best true-SIMD: `[u64;4]`/`u64x4` bitboards + Kogge-Stone + `PLAYABLE` mask,
   used mainly to make king-safety O(1)** (Tier 1c). Avoid magics.
3. Later, **SoA position-batching** over uniform stages for MCTS; **GPU only for
   neural eval.** Validate every stage with perft-divergence vs rules.js.
