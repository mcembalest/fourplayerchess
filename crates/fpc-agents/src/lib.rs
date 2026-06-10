//! Agents: everything that picks a move is an `Agent`. Classical agents live
//! here (Random, Heuristic = port of game.js botMove, Search = maxⁿ). The NN
//! agent will plug in here later behind the same trait.

use std::sync::Arc;

use fpc_core::*;

/// Picks a move for `st.current` from `st.current_legal`.
pub trait Agent {
    fn select(&mut self, st: &State) -> Move;
}

/// Small splitmix64 PRNG — no external crate, deterministic from a seed.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_add(0x9E3779B97F4A7C15))
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

#[derive(Clone)]
pub enum AgentKind {
    Random,
    Heuristic,
    Search(u32),
    Net { net: Arc<Net>, label: String },
    NetSearch { net: Arc<Net>, depth: u32, label: String },
    /// Paranoid alpha-beta (me vs. the field), material leaf eval.
    Paranoid(u32),
    /// Paranoid alpha-beta with the value net as leaf eval.
    ParanoidNet { net: Arc<Net>, depth: u32, label: String },
}

impl AgentKind {
    pub fn name(&self) -> String {
        match self {
            AgentKind::Random => "random".into(),
            AgentKind::Heuristic => "heuristic".into(),
            AgentKind::Search(d) => format!("search{d}"),
            AgentKind::Net { label, .. } => label.clone(),
            AgentKind::NetSearch { label, .. } => label.clone(),
            AgentKind::Paranoid(d) => format!("paranoid{d}"),
            AgentKind::ParanoidNet { label, .. } => label.clone(),
        }
    }
    /// Deterministic agent (argmax move selection).
    pub fn build(&self, seed: u64) -> Box<dyn Agent> {
        self.build_temp(seed, 0.0)
    }

    /// Agent with selection temperature `temp` (>0 = sample among near-best moves
    /// via range-normalized softmax — opening variety while staying sharp where
    /// one move dominates). `temp=0` reproduces `build`. Heuristic/Random ignore
    /// temp (Heuristic already has its own scoring noise; Random is uniform).
    pub fn build_temp(&self, seed: u64, temp: f64) -> Box<dyn Agent> {
        match self {
            AgentKind::Random => Box::new(RandomAgent { rng: Rng::new(seed) }),
            AgentKind::Heuristic => Box::new(HeuristicAgent { rng: Rng::new(seed) }),
            AgentKind::Search(d) => {
                Box::new(SearchAgent { rng: Rng::new(seed), depth: (*d).max(1), temp })
            }
            AgentKind::Net { net, .. } => {
                Box::new(NetAgent { net: net.clone(), rng: Rng::new(seed), temp })
            }
            AgentKind::NetSearch { net, depth, .. } => Box::new(NetSearchAgent {
                net: net.clone(),
                depth: (*depth).max(1),
                rng: Rng::new(seed),
                temp,
            }),
            AgentKind::Paranoid(d) => Box::new(ParanoidAgent {
                net: None,
                depth: (*d).max(1),
                rng: Rng::new(seed),
                temp,
            }),
            AgentKind::ParanoidNet { net, depth, .. } => Box::new(ParanoidAgent {
                net: Some(net.clone()),
                depth: (*depth).max(1),
                rng: Rng::new(seed),
                temp,
            }),
        }
    }
}

/// Pick an index into `vals` by range-normalized softmax sampling at temperature
/// `temp`. `temp<=0` (or a single candidate) returns the argmax. Normalizing by
/// the value range makes it scale-invariant across evaluators (score-shares vs
/// raw material): when all values are equal it's uniform (max variety), when one
/// dominates it concentrates there (sharp play).
pub fn sample_softmax(vals: &[f64], temp: f64, rng: &mut Rng) -> usize {
    let mut bi = 0;
    let mut bv = f64::MIN;
    for (i, &v) in vals.iter().enumerate() {
        if v > bv {
            bv = v;
            bi = i;
        }
    }
    if temp <= 0.0 || vals.len() <= 1 {
        return bi;
    }
    let min = vals.iter().cloned().fold(f64::MAX, f64::min);
    let range = (bv - min).max(1e-9);
    let probs: Vec<f64> = vals
        .iter()
        .map(|&v| ((v - bv) / (temp * range)).exp())
        .collect();
    let sum: f64 = probs.iter().sum();
    let mut r = rng.next_f64() * sum;
    for (i, &p) in probs.iter().enumerate() {
        r -= p;
        if r <= 0.0 {
            return i;
        }
    }
    bi
}

pub struct RandomAgent {
    rng: Rng,
}
impl Agent for RandomAgent {
    fn select(&mut self, st: &State) -> Move {
        let m = &st.current_legal;
        m[self.rng.below(m.len())]
    }
}

/// Faithful port of game.js `botMove` scoring (1-ply greedy with hang-avoidance
/// and check/mate bonuses).
pub struct HeuristicAgent {
    rng: Rng,
}
impl Agent for HeuristicAgent {
    fn select(&mut self, st: &State) -> Move {
        let color = st.current.unwrap();
        let mut best = st.current_legal[0];
        let mut best_score = -1e9;
        for &mv in &st.current_legal {
            let mut s = self.rng.next_f64() * 1.5;
            let cap = st.board[mv.tr as usize][mv.tc as usize];
            let cap_val = cap.map_or(0, |p| value(p.kind));
            s += cap_val as f64 * 10.0;
            if mv.promo {
                s += 80.0;
            }
            let mut nb = st.board;
            apply_to(&mut nb, mv);
            let piece_type = if mv.promo {
                Kind::Q
            } else {
                st.board[mv.fr as usize][mv.fc as usize].unwrap().kind
            };
            if attacked(&nb, &st.eliminated, mv.tr, mv.tc, color) {
                s -= (value(piece_type) - cap_val).max(0) as f64 * 7.0;
            }
            for &o in ORDER.iter() {
                if o == color || st.eliminated[o.idx()] {
                    continue;
                }
                if king_attacked(&nb, &st.eliminated, o) {
                    s += 3.0;
                    if legal_moves(&nb, &st.eliminated, o).is_empty() {
                        s += 1000.0;
                    }
                }
            }
            if s > best_score {
                best_score = s;
                best = mv;
            }
        }
        best
    }
}

/// Depth-limited maxⁿ search: each player maximizes their own component of the
/// value vector at their own nodes (the correct generalization of minimax to a
/// multiplayer general-sum game).
pub struct SearchAgent {
    rng: Rng,
    depth: u32,
    temp: f64,
}

/// Per-player value estimate: points banked + material on board (active only).
fn material_eval(st: &State) -> [f64; 4] {
    let mut v = [0.0f64; 4];
    for i in 0..4 {
        v[i] = st.scores[i] as f64;
    }
    for r in 0..14 {
        for c in 0..14 {
            if let Some(p) = st.board[r][c] {
                if !st.eliminated[p.color.idx()] {
                    v[p.color.idx()] += value(p.kind) as f64 * 0.5;
                }
            }
        }
    }
    v
}

/// Maxⁿ lookahead with a pluggable leaf evaluator: each player maximizes its own
/// component of the value vector at its own node.
fn maxn<F: Fn(&State) -> [f64; 4]>(st: &State, depth: u32, ev: &F) -> [f64; 4] {
    if st.over || depth == 0 {
        return ev(st);
    }
    let me = st.current.unwrap().idx();
    let mut best: Option<[f64; 4]> = None;
    for &mv in &st.current_legal {
        let mut ns = st.for_search();
        ns.make_move(mv);
        let v = maxn(&ns, depth - 1, ev);
        match best {
            None => best = Some(v),
            Some(b) if v[me] > b[me] => best = Some(v),
            _ => {}
        }
    }
    best.unwrap_or_else(|| ev(st))
}

/// Pick a move by the mover's maxⁿ value under evaluator `ev`, softmax-sampled at
/// temperature `temp` (temp=0 => argmax).
fn maxn_select<F: Fn(&State) -> [f64; 4]>(
    st: &State,
    depth: u32,
    rng: &mut Rng,
    ev: &F,
    temp: f64,
) -> Move {
    let me = st.current.unwrap().idx();
    let moves = &st.current_legal;
    let mut scores = Vec::with_capacity(moves.len());
    for &mv in moves {
        let mut ns = st.for_search();
        ns.make_move(mv);
        scores.push(maxn(&ns, depth.saturating_sub(1), ev)[me] as f64);
    }
    moves[sample_softmax(&scores, temp, rng)]
}

impl Agent for SearchAgent {
    fn select(&mut self, st: &State) -> Move {
        maxn_select(st, self.depth, &mut self.rng, &material_eval, self.temp)
    }
}

/// Maxⁿ search using the value net as the leaf evaluator (learned eval + lookahead).
pub struct NetSearchAgent {
    net: Arc<Net>,
    depth: u32,
    rng: Rng,
    temp: f64,
}

/// Net value as a normalized score-share 4-vector (true shares at terminal nodes).
fn net_eval(net: &Net, st: &State) -> [f64; 4] {
    if st.over {
        let s = score_shares(&st.scores);
        return [s[0] as f64, s[1] as f64, s[2] as f64, s[3] as f64];
    }
    let raw = net.value(st);
    let mut sum = 0.0f64;
    let mut o = [0.0f64; 4];
    for i in 0..4 {
        o[i] = (raw[i].max(0.0)) as f64;
        sum += o[i];
    }
    if sum <= 0.0 {
        return [0.25; 4];
    }
    for i in 0..4 {
        o[i] /= sum;
    }
    o
}

impl Agent for NetSearchAgent {
    fn select(&mut self, st: &State) -> Move {
        let net = self.net.clone();
        let ev = |s: &State| net_eval(&net, s);
        maxn_select(st, self.depth, &mut self.rng, &ev, self.temp)
    }
}

/// Paranoid alpha-beta search: collapse the 4-player game to 2-player zero-sum
/// from the root mover's perspective — the mover maximizes its own score-share,
/// every other (live) player is assumed to minimize it. This admits alpha-beta
/// pruning, so it searches far deeper than maxⁿ for the same cost (maxⁿ can't
/// prune). Less myopic than depth-2 maxⁿ (which only sees one opponent reply).
pub struct ParanoidAgent {
    net: Option<Arc<Net>>,
    depth: u32,
    rng: Rng,
    temp: f64,
}

/// Order moves captures-first (by captured value) to sharpen alpha-beta pruning.
fn ordered_moves(st: &State) -> Vec<Move> {
    let mut mv = st.current_legal.clone();
    order_moves(st, &mut mv, &[None, None]);
    mv
}

/// Max search ply the killer tables cover (depth is ≤8 in practice).
const MAX_PLY: usize = 16;
/// Two remembered cutoff moves per ply (killer heuristic).
type Killers = [[Option<Move>; 2]; MAX_PLY];

/// Sort `mv` in place: captures first (by captured value), then this ply's
/// killer moves (quiet moves that recently caused a cutoff here), then the rest.
fn order_moves(st: &State, mv: &mut [Move], killers: &[Option<Move>; 2]) {
    mv.sort_by_key(|m| {
        let cap = st.board[m.tr as usize][m.tc as usize];
        let v = cap.map_or(0, |p| value(p.kind));
        let mut s = 10 * (v + if m.promo { 9 } else { 0 });
        if s == 0 && (killers[0] == Some(*m) || killers[1] == Some(*m)) {
            s = 5;
        }
        std::cmp::Reverse(s)
    });
}

/// Remember a cutoff move in this ply's killer slots (most recent first).
#[inline]
fn note_killer(killers: &mut Killers, ply: usize, mv: Move) {
    let ks = &mut killers[ply.min(MAX_PLY - 1)];
    if ks[0] != Some(mv) {
        ks[1] = ks[0];
        ks[0] = Some(mv);
    }
}

/// Scalar paranoid alpha-beta returning the root mover `me`'s value.
/// Takes the node by `&mut` so the move list can be taken out of the state
/// instead of cloned per node (callers pass freshly made search states).
fn paranoid<F: Fn(&State) -> [f64; 4]>(
    st: &mut State,
    depth: u32,
    mut alpha: f64,
    mut beta: f64,
    me: usize,
    ev: &F,
    ply: usize,
    killers: &mut Killers,
) -> f64 {
    if st.over || depth == 0 {
        return ev(st)[me];
    }
    let mut moves = std::mem::take(&mut st.current_legal);
    order_moves(st, &mut moves, &killers[ply.min(MAX_PLY - 1)]);
    let maximizing = st.current.unwrap().idx() == me;
    if maximizing {
        let mut v = -1e9f64;
        for mv in moves {
            let mut ns = st.for_search();
            ns.make_move(mv);
            v = v.max(paranoid(&mut ns, depth - 1, alpha, beta, me, ev, ply + 1, killers));
            alpha = alpha.max(v);
            if alpha >= beta {
                note_killer(killers, ply, mv);
                break;
            }
        }
        v
    } else {
        let mut v = 1e9f64;
        for mv in moves {
            let mut ns = st.for_search();
            ns.make_move(mv);
            v = v.min(paranoid(&mut ns, depth - 1, alpha, beta, me, ev, ply + 1, killers));
            beta = beta.min(v);
            if alpha >= beta {
                note_killer(killers, ply, mv);
                break;
            }
        }
        v
    }
}

impl Agent for ParanoidAgent {
    fn select(&mut self, st: &State) -> Move {
        let me = st.current.unwrap().idx();
        let net = self.net.clone();
        let ev = |s: &State| match &net {
            Some(n) => net_eval(n, s),
            None => material_eval(s),
        };
        let mut moves = ordered_moves(st);
        let d = self.depth.saturating_sub(1);
        let mut killers: Killers = [[None; 2]; MAX_PLY];
        // Iterative deepening at the root: a cheap shallow pass orders the root
        // moves best-first (and warms the killer tables), so the full-depth loop
        // establishes a high alpha on the first move — much sharper cutoffs than
        // captures-first ordering alone.
        if d >= 2 {
            let mut alpha = -1e9;
            let mut shallow: Vec<(f64, Move)> = Vec::with_capacity(moves.len());
            for &mv in &moves {
                let mut ns = st.for_search();
                ns.make_move(mv);
                let v = paranoid(&mut ns, d - 2, alpha, 1e9, me, &ev, 0, &mut killers);
                alpha = alpha.max(v);
                shallow.push((v, mv));
            }
            shallow.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            moves = shallow.into_iter().map(|(_, m)| m).collect();
        }
        // Root alpha-beta with a running alpha (keeps it fast). Best/near-best
        // moves get accurate values; clearly-worse moves may be underestimated by
        // the cutoff — harmless for both argmax (temp=0) and softmax sampling
        // (they'd get low probability anyway).
        let mut scores = Vec::with_capacity(moves.len());
        let mut alpha = -1e9;
        for &mv in &moves {
            let mut ns = st.for_search();
            ns.make_move(mv);
            let v = paranoid(&mut ns, d, alpha, 1e9, me, &ev, 0, &mut killers);
            alpha = alpha.max(v);
            scores.push(v);
        }
        moves[sample_softmax(&scores, self.temp, &mut self.rng)]
    }
}

/// Hidden layer width — must match HIDDEN in the trainer.
pub const HIDDEN: usize = 128;
/// LayerNorm epsilon (must match the trainer).
pub const LN_EPS: f32 = 1e-5;

/// Tiny MLP with LayerNorm before each ReLU (PQN-style: LN stabilizes
/// bootstrapped TD training): in_dim -> [Linear,LN,ReLU] -> [Linear,LN,ReLU]
/// -> Linear -> 4. Weights are a flat f32 file in `from_bytes` order; the
/// (in_dim, hidden) shape is inferred from the blob length. in_dim doubles as
/// the format marker: FEAT_DIM = absolute features, FEAT_DIM_REL =
/// perspective-relative features with rotated outputs (see `value`).
pub struct Net {
    f: usize, // input dim (which feature format this net consumes)
    h: usize, // hidden width
    w1: Vec<f32>,
    b1: Vec<f32>,
    g1: Vec<f32>, // LayerNorm gain, layer 1
    n1: Vec<f32>, // LayerNorm bias, layer 1
    w2: Vec<f32>,
    b2: Vec<f32>,
    g2: Vec<f32>,
    n2: Vec<f32>,
    w3: Vec<f32>,
    b3: Vec<f32>,
}

/// Hidden widths the blob-shape inference will try.
const HIDDEN_CANDIDATES: [usize; 4] = [128, 256, 384, 512];
/// Upper bound on hidden width (stack buffers in `forward`).
pub const MAX_HIDDEN: usize = 512;

/// Dot product with 8 independent accumulator lanes. The naive single-
/// accumulator form is a sequential FMA dependency chain the compiler may not
/// reassociate (no fast-math), leaving the matvec latency-bound; explicit lanes
/// let it run at FMA throughput and auto-vectorize (NEON / wasm simd128).
#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut lanes = [0.0f32; 8];
    let mut ca = a[..n].chunks_exact(8);
    let mut cb = b[..n].chunks_exact(8);
    for (xa, xb) in (&mut ca).zip(&mut cb) {
        for l in 0..8 {
            lanes[l] += xa[l] * xb[l];
        }
    }
    let mut s = lanes.iter().sum::<f32>();
    for (xa, xb) in ca.remainder().iter().zip(cb.remainder()) {
        s += xa * xb;
    }
    s
}

/// Apply LayerNorm with gain `g` and bias `n` to `z` in place, returning nothing.
#[inline]
fn layernorm(z: &mut [f32], g: &[f32], n: &[f32]) {
    let h = z.len();
    let mean: f32 = z.iter().sum::<f32>() / h as f32;
    let var: f32 = z.iter().map(|&v| (v - mean) * (v - mean)).sum::<f32>() / h as f32;
    let inv = 1.0 / (var + LN_EPS).sqrt();
    for i in 0..h {
        z[i] = g[i] * ((z[i] - mean) * inv) + n[i];
    }
}

/// Flat weight-blob length in floats for a given (in_dim, hidden) shape.
pub fn net_blob_floats(f: usize, h: usize) -> usize {
    h * f + h + h + h + h * h + h + h + h + 4 * h + 4
}

/// Infer (in_dim, hidden) from a blob's float count. Tries the known hidden
/// widths against both feature formats; exactly one must match.
fn infer_dims(len: usize) -> (usize, usize) {
    let mut found = None;
    for &h in &HIDDEN_CANDIDATES {
        for &f in &[FEAT_DIM, FEAT_DIM_REL] {
            if net_blob_floats(f, h) == len {
                assert!(found.is_none(), "ambiguous model shape for {len} floats");
                found = Some((f, h));
            }
        }
    }
    found.unwrap_or_else(|| panic!("model size {len} floats matches no known (in_dim, hidden)"))
}

impl Net {
    pub fn load(path: &str) -> std::io::Result<Net> {
        let bytes = std::fs::read(path)?;
        Ok(Net::from_bytes(&bytes))
    }

    /// Parse a flat f32 weight blob in order:
    /// w1,b1,g1,n1, w2,b2,g2,n2, w3,b3 (each W out*in row-major).
    pub fn from_bytes(bytes: &[u8]) -> Net {
        let floats: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let (f, h) = infer_dims(floats.len());
        let mut o = 0;
        let mut take = |n: usize| {
            let s = floats[o..o + n].to_vec();
            o += n;
            s
        };
        let w1 = take(h * f);
        let b1 = take(h);
        let g1 = take(h);
        let n1 = take(h);
        let w2 = take(h * h);
        let b2 = take(h);
        let g2 = take(h);
        let n2 = take(h);
        let w3 = take(4 * h);
        let b3 = take(4);
        Net { f, h, w1, b1, g1, n1, w2, b2, g2, n2, w3, b3 }
    }

    /// Input dimension = which feature format this net consumes.
    pub fn in_dim(&self) -> usize {
        self.f
    }

    /// Raw forward pass; `x` must be in this net's own feature format.
    /// Zero-allocation (stack buffers) — this is the search-leaf hot path.
    pub fn forward(&self, x: &[f32]) -> [f32; 4] {
        let (f, h) = (self.f, self.h);
        debug_assert_eq!(x.len(), f);
        let mut y1 = [0.0f32; MAX_HIDDEN];
        let y1 = &mut y1[..h];
        for i in 0..h {
            y1[i] = self.b1[i] + dot(&self.w1[i * f..(i + 1) * f], x);
        }
        layernorm(y1, &self.g1, &self.n1);
        for v in y1.iter_mut() {
            *v = v.max(0.0);
        }
        let mut y2 = [0.0f32; MAX_HIDDEN];
        let y2 = &mut y2[..h];
        for i in 0..h {
            y2[i] = self.b2[i] + dot(&self.w2[i * h..(i + 1) * h], y1);
        }
        layernorm(y2, &self.g2, &self.n2);
        for v in y2.iter_mut() {
            *v = v.max(0.0);
        }
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            out[i] = self.b3[i] + dot(&self.w3[i * h..(i + 1) * h], y2);
        }
        out
    }

    /// Predicted final score-shares in ABSOLUTE colour order (R,B,Y,G), for any
    /// net format: picks the right feature extractor and, for perspective-
    /// relative nets, rotates the output back by the side to move. Only valid
    /// for non-terminal positions (callers use score_shares at terminals).
    pub fn value(&self, st: &State) -> [f32; 4] {
        if self.f == FEAT_DIM_REL {
            let mover = st.current.expect("value() needs a side to move").idx();
            let out = self.forward(&features_rel(st));
            let mut abs = [0.0f32; 4];
            for k in 0..4 {
                abs[(mover + k) % 4] = out[k];
            }
            abs
        } else {
            self.forward(&features(st))
        }
    }
}

/// Learned 1-ply evaluator: pick the move maximizing the net's predicted final
/// score-share for the moving player (terminal positions use the true share).
pub struct NetAgent {
    net: Arc<Net>,
    rng: Rng,
    temp: f64,
}
impl Agent for NetAgent {
    fn select(&mut self, st: &State) -> Move {
        let me = st.current.unwrap().idx();
        let moves = &st.current_legal;
        let mut scores = Vec::with_capacity(moves.len());
        for &mv in moves {
            let mut ns = st.for_search();
            ns.make_move(mv);
            let v = if ns.over {
                score_shares(&ns.scores)
            } else {
                self.net.value(&ns)
            };
            scores.push(v[me] as f64);
        }
        moves[sample_softmax(&scores, self.temp, &mut self.rng)]
    }
}
