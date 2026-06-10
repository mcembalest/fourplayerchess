# 4PC value-net autoresearch log

Goal: raise the learned value net's arena Elo (anchors: random≈544, search2≈1140,
heuristic≈1266 are fixed reference agents in the pool). Method track: PQN-style
TD(λ) + LayerNorm + iterated self-play (see memory `ref-pqn-td`).

Metric: `arena <games> 250 <model.bin>` Elo of the `net` (1-ply value-greedy) and
`netsearch2` (maxⁿ depth-2 w/ net leaf) agents vs the fixed anchors.

| # | change | net elo | netsearch2 elo | notes |
|---|--------|--------:|---------------:|-------|
| E0 | baseline (MC label, MLP 52→128→128→4, no LN) | 946 | 1104 | 600 games. heuristic 1266 / search2 1140 / random 544 |
| E1 | + LayerNorm (PQN arch), λ=1 (still MC) | 1008 | 1047 | 600 games. heuristic 1242 / random 533. LN alone = +62 on 1-ply net. ~11min/30ep (scalar trainer). |
| E2 | + TD(λ=0.65), bootstrap=E1, same teacher data | 960 | 1075 | 600 games. heuristic 1281 / random 550. MSE→0.0004 (self-consistent) but no play gain: TD(λ) on fixed data = policy *evaluation* only. Trainer parallelized w/ rayon (~6×, 2min/30ep). |

**Key finding after E2:** search2 (material leaf, 1134) > netsearch2 (net leaf, 1075).
The net value is *worse for shallow search than raw material counting* — it's
tactically blind (smooth long-horizon target). Pivot: improve the SEARCH
(paranoid/BRS + deeper, alpha-beta) so a net+search policy beats the heuristic
(1240+), THEN iterate self-play on that stronger teacher.

### Engine perf (oracle-validated; random self-play pos/s, single-thread)
86.6k baseline → +build profile (LTO/cgu1/native) 87.6k → +make/unmake in
`legal_moves` (no board clone per pseudo-move + king-sq cached once) 112k →
+reverse-lookup `attacked()` (probe outward from target, ~40 checks vs 196-cell
scan) **285k = 3.3×**. diff.rs green throughout; throughput stays exact 499967/1.
Data-gen CPU work ~2.36× less (pn leaves still do unbatched net forwards). Next
lever if needed: lazy legality (pins/checkers) — see bench/MOVEGEN-VECTORIZATION.md.

### Net/search perf pass (2026-06-09)
Leaf split measured (latency.rs probe): features 0.54µs vs **Net::forward 8.7µs**
— forward was 94% of leaf cost, single-accumulator dots = latency-bound FMA chain.
Fixes: 8-lane `dot()` + stack buffers in forward (8.7→2.2µs, 4×); mem::take move
lists + for_search in maxⁿ (no per-node clones); trainer on same dot + zero-alloc
backprop + rayon TD-target pass (~4s→0.4s/epoch, ~10×); arena forces ≥1 candidate
seat/game (anchor-vs-anchor games carried no new info; NOTE anchor Elo not
comparable to pre-change runs). **pnet4: 603→239ms/move start, 379→154ms midgame
(~2.5×); arena 100g/d4 = 2:35 (was ~6.5min); data-gen 24g/d4+eps = 41.5s.**
All behavior-preserving (fp reassociation only); diff.rs green; wasm builds.

Round 2 (same day): **killer moves + root iterative deepening** in paranoid
(TT skipped on purpose: round-robin turn order = no transpositions until d8).
Killers alone: identical trajectories, −23% CPU; +ID: −15% more (slight
tie-break drift, 5452→5468 pos/24g). **Worktree-verified totals vs HEAD:
data-gen 24g/d4 568→180s CPU (3.2×), 86→33s wall; arena 60g/d4 = 45s (~5× the
13min/200g era).** Plus **replay buffer shipped**: `selfplay ... <tag>` →
data/buffer/<tag>/; `train ... [decay]` loads ALL gens, samples rows at
decay^age per epoch (decay=1 uniform keep-all; legacy flat files if no buffer).
Validated: 2-gen load, decay=0.5 sampled 3644/~3646 expected rows.
Next levers: batched leaf eval (GEMM), lazy legality (needs sign-off), migrate
gen data into data/buffer/ tags and rerun the gen-3 iterate with accumulation.

| E9 | **gen-4: buffer accumulation** (g3 93k + g4 87k rows, both from gen2b/champion teachers, TD λ=0.65, bootstrap=champion, decay=1) | pnet4 1274 vs **champ4 1242 (direct h2h)**, paranoid4 1278 | 200g/d4, arena now seats challenger+champion in one pool (`arena ... model2.bin`). +32 = within noise → **no champion change; gen2b stays.** |

**E9 findings:** (1) 2× data from the SAME teacher generation ≠ the accumulation
the plateau called for — pre-gen3 diverse data was overwritten before the buffer
existed; real test needs ≥2 distinct teacher gens in the buffer (start tagging
every gen from now on). (2) **Metric break:** killer+ID move ordering made
paranoid4 itself much stronger (tie-breaks by shallow search instead of capture
order; material eval ties constantly, net eval doesn't). pnet4−paranoid4
value-add is NOT comparable across the search change — at d4 it's now ~0.
Use direct challenger-vs-champ4 h2h (same pool, same conditions) going forward.
(3) Open question for next session: does the net leaf still add anything over
material at d4 under the improved search, and does the answer flip at d6 (now
affordable: d4 search is 3.2× cheaper)?

| E10 | **d6 probe** (48g, champion net, depth 6) | paranoid6 1417 / pnet6 1403 — tied | 4.9 CPU-h. Costs: pnet6 3.4s/move start, 19s midgame; paranoid6 0.9–2.4s. |

**E10 conclusion:** the E4 "net leaf pulls ahead with depth" pattern does NOT
reappear at d6 under killer+ID ordering (at d4 it's also gone, E9). Better move
ordering closed the gap the net's smooth values used to provide. The value NET
is now the bottleneck, not search depth → next lever is the net itself:
**perspective-relative features** (current 52 are absolute — the net learns
every pattern 4×, once per seat rotation) **+ wider hidden layer** (H=128 was
sized when forward cost 8.7µs; it's 2.2µs now, trainer 0.4s/epoch). Breaks
.bin/FEAT_DIM compat — old champion keeps working at old commits / current wasm
until a new champion ships. d6 teachers parked until a net is worth deepening.

### Search track (paranoid alpha-beta = me vs. the field, scalar my-share)
| # | agent | elo | notes |
|---|-------|----:|-------|
| E3 | paranoid3 (material) | 1140 | 400g. heuristic 1200 anchor. pnet3 (net leaf) 1127, search2 1093. |
| E4 | **paranoid d4** | **pnet4 1270 / paranoid4 1219** | 200g. **BOTH beat heuristic (1190).** net(1-ply) 843, search2 1022. Cost: 13min/200g (d4 in pool). |

| E5 | **distill paranoid-d4 self-play → net (TD λ=0.65, LN)** | **pnet4 1396 / paranoid4 1230** | 200g/d4. net leaf distilled from 240 above-heuristic games (54.7k pos), bootstrap=E1. pnet4 **+126** vs E4, **+263 over heuristic (1133)**. 1-ply net flat (831). Distillation loop works. |

| E6 | **gen-2: distill from pnet4@E4 teacher** (TD λ=0.65, bootstrap=E4) | **pnet4 1323 / paranoid4 1263** | 200g/d4. **REGRESSED.** Clean anchor: pnet4−paranoid4 (net's value-add over material leaf) fell +166→+60. model_e4 stays champion. |

| E7 | **gen-2b: diversity fix** (ε=0.1 explore + broad teacher mix random→net→heuristic→paranoid, 400 games), bootstrap=E4 | **pnet4 1430 / paranoid4 1235** | NEW CHAMPION. net value-add pnet4−paranoid4 **+195** (gen-1 +166, gen-2 +60). +329 over heuristic. Diversity fix confirmed. model_gen2b.bin. |

| E8 | **gen-3**: diverse iterate from gen-2b teacher (ε=0.1, 400g, bootstrap=gen2b) | **pnet4 1337 / paranoid4 1213** | net value-add +124 (gen-2b +195). REGRESSED — gains peaked at gen-2b, non-monotonic. Champion stays gen-2b (1430, shipped). |

**ITERATION PLATEAU (gen-3):** distillation gains are non-monotonic — net value-add
pnet4−paranoid4 went +166 (g1) → +195 (g2b, peak) → +124 (g3). gen-2b = champion
(shipped as data/champion.bin). Beyond here needs a different lever (accumulate data
across gens / much larger diverse buffer / true PUCT-MCTS), not another naive iterate.

**ITERATION FAILURE (E6/gen-2):** naive iteration on 240 games of near-deterministic
strong-vs-strong play collapsed diversity → net overfit those lines, worse as a
search leaf. Fix = exploration noise + larger DIVERSE buffer (mix strong + varied
teachers + keep prior-gen data), à la AlphaZero. Champion remains **model_e4** (gen-1,
pnet4=1396). Gen-1 worked because its e1-net teachers were weak+varied (broad data).

**BREAKTHROUGH (E4):** at even depth-4, paranoid alpha-beta beats the heuristic.
And the **net leaf beats the material leaf at depth (pnet4 1270 > paranoid4 1219)**
— reversal of the depth-2 result; the net's smooth positional value compounds
over deeper search. We now have a teacher (pnet4=1270) stronger than the
heuristic → distill it into the fast net via iterated self-play + TD(λ). Even
depths only (parity matters: d3 < d4).
