//! Self-play data generation. Plays games with a configurable field of agents,
//! records every visited position in trajectory order, and writes flat files:
//!   X.bin    (n * FEAT_DIM)            features, in trajectory order
//!   Y.bin    (n * 4)                   per-row terminal reward (final shares)
//!   lens.bin (num_traj * u32)          length of each trajectory (for TD(λ))
//!
//!   cargo run -p fpc-train --release -- [num_games] [max_steps] [model.bin?] [depth] [eps] [tag?]
//!
//! With a `tag` (6th arg) the files go to data/buffer/<tag>/ — an append-only
//! replay buffer of generations the trainer reads in full (recency-weighted).
//! Without a tag they go to data/ directly (legacy single-generation mode).
//!
//! Without a model: heuristic/random fields (bootstrap data). With a model: the
//! net plays a share of the games (iterated self-play — the net learns from its
//! own, stronger, policy).

use std::io::Write;
use std::sync::Arc;

use fpc_agents::*;
use fpc_core::*;
use rayon::prelude::*;

/// One game -> (features per visited position in order, terminal reward 4-vec).
/// `eps` is the per-ply probability of playing a uniformly random legal move
/// instead of the agent's choice (exploration noise to diversify positions).
fn play_and_record(
    seats: [AgentKind; 4],
    seed: u64,
    max_steps: usize,
    eps: f64,
) -> (Vec<f32>, [f32; 4], u32) {
    let mut agents: [Box<dyn Agent>; 4] = [
        seats[0].build(seed ^ 0x11),
        seats[1].build(seed ^ 0x22),
        seats[2].build(seed ^ 0x33),
        seats[3].build(seed ^ 0x44),
    ];
    let mut explore = Rng::new(seed ^ 0xE5E5);
    let mut st = State::new_game();
    let mut xs: Vec<f32> = Vec::new();
    let mut steps = 0;
    let mut len = 0u32;
    while !st.over && steps < max_steps {
        xs.extend_from_slice(&features(&st));
        len += 1;
        let c = st.current.unwrap();
        let mv = if eps > 0.0 && explore.next_f64() < eps {
            st.current_legal[explore.below(st.current_legal.len())]
        } else {
            agents[c.idx()].select(&st)
        };
        st.make_move(mv);
        steps += 1;
    }
    (xs, score_shares(&st.scores), len)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let num_games: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4000);
    let max_steps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(250);
    let model_path: Option<&String> = args.get(3);
    let depth: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let eps: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let out_dir = match args.get(6) {
        Some(tag) => format!("data/buffer/{tag}"),
        None => "data".to_string(),
    };

    // Build the per-game seat fields. With a model + search depth, the strong
    // paranoid agents (which beat the heuristic) are the teachers — the value
    // target then reflects above-heuristic play, distilling search into the net.
    let fields: Vec<[AgentKind; 4]> = if let (Some(path), d) = (model_path, depth) {
        if d == 0 {
            panic!("give a paranoid depth (4th arg) for net self-play, e.g. 4");
        }
        let net = Arc::new(Net::load(path).expect("load model"));
        let pn = || AgentKind::ParanoidNet { net: net.clone(), depth: d, label: "pn".into() };
        let pm = || AgentKind::Paranoid(d);
        let nn = || AgentKind::Net { net: net.clone(), label: "net".into() };
        let h = || AgentKind::Heuristic;
        let r = || AgentKind::Random;
        eprintln!("distillation self-play: paranoid d{d} teachers, net from {path}, eps={eps}");
        // Broad mix spanning strengths so the value net sees varied positions
        // (not just strong-vs-strong endgames — gen-2 overfit those). Strong
        // fields keep quality; mixed/weak fields keep breadth.
        vec![
            [pn(), pn(), pm(), pm()],         // strong (quality)
            [pn(), h(), pm(), h()],           // strong vs heuristic
            [pn(), nn(), h(), r()],           // wide strength spread
            [nn(), h(), nn(), h()],           // mid: 1-ply net vs heuristic
            [h(), h(), r(), r()],             // weak/varied (breadth)
            [pm(), nn(), h(), r()],           // very mixed
        ]
    } else if let Some(path) = model_path {
        let net = Arc::new(Net::load(path).expect("load model"));
        let n = || AgentKind::Net { net: net.clone(), label: "net".into() };
        let ns = || AgentKind::NetSearch { net: net.clone(), depth: 2, label: "ns".into() };
        eprintln!("iterated self-play with net from {path}");
        vec![
            [ns(), ns(), ns(), ns()],
            [n(), n(), n(), n()],
            [ns(), AgentKind::Heuristic, ns(), AgentKind::Heuristic],
            [n(), AgentKind::Heuristic, n(), AgentKind::Heuristic],
            [AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Heuristic],
            [ns(), n(), AgentKind::Heuristic, AgentKind::Random],
        ]
    } else {
        vec![
            [AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Heuristic],
            [AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Random, AgentKind::Random],
            [AgentKind::Heuristic, AgentKind::Random, AgentKind::Heuristic, AgentKind::Random],
            [AgentKind::Random, AgentKind::Random, AgentKind::Random, AgentKind::Random],
        ]
    };

    // Collect per-game results in order (preserves trajectory boundaries).
    let done = std::sync::atomic::AtomicUsize::new(0);
    let step = (num_games / 20).max(1);
    let games: Vec<(Vec<f32>, [f32; 4], u32)> = (0..num_games)
        .into_par_iter()
        .map(|g| {
            let field = fields[g % fields.len()].clone();
            let r = play_and_record(
                field,
                (g as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xDEAD,
                max_steps,
                eps,
            );
            let c = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if c % step == 0 || c == num_games {
                eprintln!("progress: {c}/{num_games} games");
            }
            r
        })
        .collect();

    let mut xs: Vec<f32> = Vec::new();
    let mut ys: Vec<f32> = Vec::new();
    let mut lens: Vec<u32> = Vec::with_capacity(games.len());
    for (gx, label, len) in &games {
        xs.extend_from_slice(gx);
        for _ in 0..*len {
            ys.extend_from_slice(label);
        }
        lens.push(*len);
    }

    let n = ys.len() / 4;
    std::fs::create_dir_all(&out_dir)?;
    write_f32(&format!("{out_dir}/X.bin"), &xs)?;
    write_f32(&format!("{out_dir}/Y.bin"), &ys)?;
    let mut fl = std::io::BufWriter::new(std::fs::File::create(format!("{out_dir}/lens.bin"))?);
    for v in &lens {
        fl.write_all(&v.to_le_bytes())?;
    }
    fl.flush()?;

    eprintln!(
        "wrote {} positions from {} games (FEAT_DIM={}) -> {out_dir}/{{X,Y,lens}}.bin",
        n, num_games, FEAT_DIM
    );
    Ok(())
}

fn write_f32(path: &str, data: &[f32]) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    for v in data {
        f.write_all(&v.to_le_bytes())?;
    }
    f.flush()
}
