# TODO

Strength research backlog, in rough expected-value order. Context and numbers:
`experiments/LOG.md` (E12–E14 are the current frontier).

## Value net
- [ ] **More tactical dims**: per-seat attacker/defender *counts* (not just the
      binary attacked/hanging split in FEAT_DIM_TAC=60), biggest-hanging-piece
      value, and "mover threatens X" interaction terms. E13 showed +180 Elo from
      12 tactical dims — this is the proven lever.
- [ ] **H=256 hidden width**: `Net` inference already handles it (shape
      inference, `MAX_HIDDEN=512`), but `train.rs` hardcodes `HIDDEN=128` stack
      arrays — needs a hidden-width arg / const generic.
- [ ] **Value target**: score-shares are smooth and long-horizon; consider
      placement-aware targets (rank points) or a short-horizon material-delta
      auxiliary head.
- [ ] tac gen-3 iterate only after one of the above lands (E14: another iterate
      alone ≈ tie; gains come from new signal, not more same-recipe data).

## Performance
- [ ] **Optimize `tac_stats`**: ~+30–50% leaf cost from per-piece reverse
      probes. Options: incremental attack info, probe only pieces near the last
      move, or one forward attack-map pass shared by all 4 seats.
- [ ] Fix `latency.rs` leaf-split probe to use the loaded net's feature format
      (it still times `features_rel` even for tac nets).
- [ ] Batched leaf eval (GEMM) if net width grows.
- [ ] Lazy legality / bitboards (bench/MOVEGEN-VECTORIZATION.md) — **parked,
      needs explicit sign-off** (fpc-core safe-wins-only decision).

## Arena / methodology
- [ ] Arena seed is hardcoded (`0xC0FFEE`): add a seed arg so reruns give
      independent samples; report ±Elo error bars from seat counts.
- [ ] TrueSkill/Weng-Lin rating (skillratings crate) to replace pairwise-Elo
      decomposition; SPRT-style stopping for promotion gates.

## Shipped state (2026-06-09)
- Champion = `data/model_tac_b.bin` (tac gen-1, FEAT_DIM_TAC=60), promoted to
  `data/champion.bin`, embedded in `pkg/`. Beat prior champion +116 h2h (E13).
- Buffer roots: `data/buffer` (rel 48d), `data/buffer-tac` (tac 60d),
  `data/buffer52` (archived absolute 52d). One feature format per root; trainer
  arg 6 selects the root.
