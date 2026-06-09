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
}

impl AgentKind {
    pub fn name(&self) -> String {
        match self {
            AgentKind::Random => "random".into(),
            AgentKind::Heuristic => "heuristic".into(),
            AgentKind::Search(d) => format!("search{d}"),
            AgentKind::Net { label, .. } => label.clone(),
            AgentKind::NetSearch { label, .. } => label.clone(),
        }
    }
    pub fn build(&self, seed: u64) -> Box<dyn Agent> {
        match self {
            AgentKind::Random => Box::new(RandomAgent { rng: Rng::new(seed) }),
            AgentKind::Heuristic => Box::new(HeuristicAgent { rng: Rng::new(seed) }),
            AgentKind::Search(d) => Box::new(SearchAgent { rng: Rng::new(seed), depth: (*d).max(1) }),
            AgentKind::Net { net, .. } => {
                Box::new(NetAgent { net: net.clone(), rng: Rng::new(seed) })
            }
            AgentKind::NetSearch { net, depth, .. } => Box::new(NetSearchAgent {
                net: net.clone(),
                depth: (*depth).max(1),
                rng: Rng::new(seed),
            }),
        }
    }
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
        let mut ns = st.clone();
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

/// Pick the move maximizing the mover's maxⁿ value under evaluator `ev`.
fn maxn_select<F: Fn(&State) -> [f64; 4]>(
    st: &State,
    depth: u32,
    rng: &mut Rng,
    ev: &F,
) -> Move {
    let me = st.current.unwrap().idx();
    let mut best = st.current_legal[0];
    let mut best_val = -1e9;
    for &mv in &st.current_legal {
        let mut ns = st.for_search();
        ns.make_move(mv);
        let v = maxn(&ns, depth.saturating_sub(1), ev);
        let score = v[me] + rng.next_f64() * 1e-6; // random tie-break
        if score > best_val {
            best_val = score;
            best = mv;
        }
    }
    best
}

impl Agent for SearchAgent {
    fn select(&mut self, st: &State) -> Move {
        maxn_select(st, self.depth, &mut self.rng, &material_eval)
    }
}

/// Maxⁿ search using the value net as the leaf evaluator (learned eval + lookahead).
pub struct NetSearchAgent {
    net: Arc<Net>,
    depth: u32,
    rng: Rng,
}

/// Net value as a normalized score-share 4-vector (true shares at terminal nodes).
fn net_eval(net: &Net, st: &State) -> [f64; 4] {
    if st.over {
        let s = score_shares(&st.scores);
        return [s[0] as f64, s[1] as f64, s[2] as f64, s[3] as f64];
    }
    let raw = net.forward(&features(st));
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
        maxn_select(st, self.depth, &mut self.rng, &ev)
    }
}

/// Hidden layer width — must match HIDDEN in tools/train.py.
pub const HIDDEN: usize = 128;

/// Tiny MLP: FEAT_DIM -> HIDDEN -> HIDDEN -> 4, ReLU, no framework needed.
/// Weights are a flat f32 file: W1,b1,W2,b2,W3,b3 (each W is out*in, row-major).
pub struct Net {
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
    w3: Vec<f32>,
    b3: Vec<f32>,
}

impl Net {
    pub fn load(path: &str) -> std::io::Result<Net> {
        let bytes = std::fs::read(path)?;
        Ok(Net::from_bytes(&bytes))
    }

    /// Parse a flat f32 weight blob (W1,b1,W2,b2,W3,b3, each W out*in row-major).
    pub fn from_bytes(bytes: &[u8]) -> Net {
        let floats: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let f = FEAT_DIM;
        let h = HIDDEN;
        let expected = h * f + h + h * h + h + 4 * h + 4;
        assert_eq!(
            floats.len(),
            expected,
            "model size mismatch: got {} floats, expected {expected}",
            floats.len()
        );
        let mut o = 0;
        let mut take = |n: usize| {
            let s = floats[o..o + n].to_vec();
            o += n;
            s
        };
        let w1 = take(h * f);
        let b1 = take(h);
        let w2 = take(h * h);
        let b2 = take(h);
        let w3 = take(4 * h);
        let b3 = take(4);
        Net { w1, b1, w2, b2, w3, b3 }
    }

    /// Predicts the 4-vector of final score-shares for a position.
    pub fn forward(&self, x: &[f32]) -> [f32; 4] {
        let h = HIDDEN;
        let f = FEAT_DIM;
        let mut y1 = vec![0.0f32; h];
        for i in 0..h {
            let mut s = self.b1[i];
            let row = i * f;
            for j in 0..f {
                s += self.w1[row + j] * x[j];
            }
            y1[i] = s.max(0.0);
        }
        let mut y2 = vec![0.0f32; h];
        for i in 0..h {
            let mut s = self.b2[i];
            let row = i * h;
            for j in 0..h {
                s += self.w2[row + j] * y1[j];
            }
            y2[i] = s.max(0.0);
        }
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let mut s = self.b3[i];
            let row = i * h;
            for j in 0..h {
                s += self.w3[row + j] * y2[j];
            }
            out[i] = s;
        }
        out
    }
}

/// Learned 1-ply evaluator: pick the move maximizing the net's predicted final
/// score-share for the moving player (terminal positions use the true share).
pub struct NetAgent {
    net: Arc<Net>,
    rng: Rng,
}
impl Agent for NetAgent {
    fn select(&mut self, st: &State) -> Move {
        let me = st.current.unwrap().idx();
        let mut best = st.current_legal[0];
        let mut best_val = -1e9;
        for &mv in &st.current_legal {
            let mut ns = st.for_search();
            ns.make_move(mv);
            let v = if ns.over {
                score_shares(&ns.scores)
            } else {
                self.net.forward(&features(&ns))
            };
            let score = v[me] as f64 + self.rng.next_f64() * 1e-6;
            if score > best_val {
                best_val = score;
                best = mv;
            }
        }
        best
    }
}
