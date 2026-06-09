//! Self-play data generation. Plays games with a mix of agents (heuristic for
//! quality, random for exploration), labels every visited position with that
//! game's final score-shares (Monte-Carlo return), and writes flat f32 files:
//!   data/X.bin  (n * FEAT_DIM)
//!   data/Y.bin  (n * 4)
//!
//!   cargo run -p fpc-train --release -- [num_games] [max_steps]

use std::io::Write;

use fpc_agents::*;
use fpc_core::*;
use rayon::prelude::*;

/// One game -> (features per visited position, repeated label per position).
fn play_and_record(seats: [AgentKind; 4], seed: u64, max_steps: usize) -> (Vec<f32>, Vec<f32>) {
    let mut agents: [Box<dyn Agent>; 4] = [
        seats[0].build(seed ^ 0x11),
        seats[1].build(seed ^ 0x22),
        seats[2].build(seed ^ 0x33),
        seats[3].build(seed ^ 0x44),
    ];
    let mut st = State::new_game();
    let mut feats: Vec<[f32; FEAT_DIM]> = Vec::new();
    let mut steps = 0;
    while !st.over && steps < max_steps {
        feats.push(features(&st));
        let c = st.current.unwrap();
        let mv = agents[c.idx()].select(&st);
        st.make_move(mv);
        steps += 1;
    }
    let label = score_shares(&st.scores);
    let mut xs = Vec::with_capacity(feats.len() * FEAT_DIM);
    let mut ys = Vec::with_capacity(feats.len() * 4);
    for f in &feats {
        xs.extend_from_slice(f);
        ys.extend_from_slice(&label);
    }
    (xs, ys)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let num_games: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4000);
    let max_steps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(250);

    // Per-game seat fields: mix heuristic (signal) with random (exploration).
    let fields: [[AgentKind; 4]; 4] = [
        [AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Heuristic],
        [AgentKind::Heuristic, AgentKind::Heuristic, AgentKind::Random, AgentKind::Random],
        [AgentKind::Heuristic, AgentKind::Random, AgentKind::Heuristic, AgentKind::Random],
        [AgentKind::Random, AgentKind::Random, AgentKind::Random, AgentKind::Random],
    ];

    let (xs, ys): (Vec<f32>, Vec<f32>) = (0..num_games)
        .into_par_iter()
        .map(|g| {
            let field = fields[g % fields.len()].clone();
            play_and_record(field, (g as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xDEAD, max_steps)
        })
        .reduce(
            || (Vec::new(), Vec::new()),
            |mut a, b| {
                a.0.extend(b.0);
                a.1.extend(b.1);
                a
            },
        );

    let n = ys.len() / 4;
    std::fs::create_dir_all("data")?;
    let mut fx = std::io::BufWriter::new(std::fs::File::create("data/X.bin")?);
    for v in &xs {
        fx.write_all(&v.to_le_bytes())?;
    }
    fx.flush()?;
    let mut fy = std::io::BufWriter::new(std::fs::File::create("data/Y.bin")?);
    for v in &ys {
        fy.write_all(&v.to_le_bytes())?;
    }
    fy.flush()?;

    eprintln!(
        "wrote {} positions from {} games (FEAT_DIM={}) -> data/X.bin, data/Y.bin",
        n, num_games, FEAT_DIM
    );
    Ok(())
}
