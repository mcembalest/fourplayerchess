//! Arena: run many seat-rotated games between a pool of agents (in parallel)
//! and print an Elo-style leaderboard.
//!
//!   cargo run -p fpc-arena --release -- [num_games] [max_steps]
//!
//! Elo is derived by decomposing each 4-player game into pairwise placement
//! results (the leaner, well-understood path; a proper multiplayer model like
//! TrueSkill can replace this later if needed).

use fpc_agents::*;
use fpc_core::*;
use rayon::prelude::*;

struct GResult {
    kinds: [usize; 4],  // pool index per seat
    ranks: [u32; 4],    // final placement per seat (1 = best, ties shared)
    scores: [i32; 4],   // final score per seat
}

fn play(pool: &[AgentKind], seats: [usize; 4], seed: u64, max_steps: usize) -> GResult {
    let mut agents: [Box<dyn Agent>; 4] = [
        pool[seats[0]].build(seed ^ 0xA1),
        pool[seats[1]].build(seed ^ 0xB2),
        pool[seats[2]].build(seed ^ 0xC3),
        pool[seats[3]].build(seed ^ 0xD4),
    ];
    let mut st = State::new_game();
    let mut steps = 0;
    while !st.over && steps < max_steps {
        let c = st.current.unwrap();
        let mv = agents[c.idx()].select(&st);
        st.make_move(mv);
        steps += 1;
    }

    // standard competition ranking by final score (1,2,2,4)
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&a, &b| st.scores[b].cmp(&st.scores[a]));
    let mut ranks = [0u32; 4];
    for i in 0..4 {
        ranks[order[i]] = if i > 0 && st.scores[order[i]] == st.scores[order[i - 1]] {
            ranks[order[i - 1]]
        } else {
            i as u32 + 1
        };
    }

    GResult { kinds: seats, ranks, scores: st.scores }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let num_games: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(400);
    let max_steps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(250);
    let model_path: Option<&String> = args.get(3);

    let mut pool = vec![AgentKind::Random, AgentKind::Heuristic, AgentKind::Search(2)];
    if let Some(path) = model_path {
        let net = std::sync::Arc::new(Net::load(path).expect("load model"));
        pool.push(AgentKind::Net { net: net.clone(), label: "net".into() });
        pool.push(AgentKind::NetSearch { net, depth: 2, label: "netsearch2".into() });
        eprintln!("loaded net from {path}");
    }

    let results: Vec<GResult> = (0..num_games)
        .into_par_iter()
        .map(|g| {
            let mut rng = Rng::new(0xC0FFEE ^ (g as u64).wrapping_mul(0x100000001B3));
            let seats = [
                rng.below(pool.len()),
                rng.below(pool.len()),
                rng.below(pool.len()),
                rng.below(pool.len()),
            ];
            play(&pool, seats, rng.next_u64(), max_steps)
        })
        .collect();

    // aggregate stats
    let n = pool.len();
    let mut games = vec![0u64; n];
    let mut sum_rank = vec![0u64; n];
    let mut sum_score = vec![0i64; n];
    for r in &results {
        for seat in 0..4 {
            let k = r.kinds[seat];
            games[k] += 1;
            sum_rank[k] += r.ranks[seat] as u64;
            sum_score[k] += r.scores[seat] as i64;
        }
    }

    // pairwise Elo (a few epochs over the fixed results to converge)
    let mut elo = vec![1000.0f64; n];
    for _ in 0..25 {
        for r in &results {
            for i in 0..4 {
                for j in (i + 1)..4 {
                    let (a, b) = (r.kinds[i], r.kinds[j]);
                    if a == b {
                        continue;
                    }
                    let sa = match r.ranks[i].cmp(&r.ranks[j]) {
                        std::cmp::Ordering::Less => 1.0,
                        std::cmp::Ordering::Greater => 0.0,
                        std::cmp::Ordering::Equal => 0.5,
                    };
                    let ea = 1.0 / (1.0 + 10f64.powf((elo[b] - elo[a]) / 400.0));
                    let k = 8.0;
                    elo[a] += k * (sa - ea);
                    elo[b] += k * (ea - sa);
                }
            }
        }
    }

    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| elo[b].partial_cmp(&elo[a]).unwrap());

    println!(
        "{} games, max {} plies each\n",
        num_games, max_steps
    );
    println!(
        "{:<12} {:>7} {:>9} {:>10} {:>7}",
        "agent", "seats", "avg_rank", "avg_score", "elo"
    );
    for &k in &idx {
        let g = games[k].max(1);
        println!(
            "{:<12} {:>7} {:>9.3} {:>10.2} {:>7.0}",
            pool[k].name(),
            games[k],
            sum_rank[k] as f64 / g as f64,
            sum_score[k] as f64 / g as f64,
            elo[k],
        );
    }
}
