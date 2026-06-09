//! Single-threaded random self-play throughput on the REAL fpc-core rules —
//! the apples-to-apples counterpart to bench/engine_throughput.mjs (which drives
//! the real rules.js). Measures positions/sec of the actual engine hot path
//! (legal-move gen + make_move + draw bookkeeping), not a proxy kernel.
//!
//!   cargo run -p fpc-train --release --bin throughput -- [games] [max_steps]

use fpc_agents::Rng;
use fpc_core::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let max_steps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(250);

    let t = std::time::Instant::now();
    let mut positions: u64 = 0;
    let mut finished: u64 = 0;
    for g in 0..games {
        let mut rng = Rng::new((g as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xBEEF);
        let mut st = State::new_game();
        let mut steps = 0;
        while !st.over && steps < max_steps {
            positions += 1;
            let m = &st.current_legal;
            let mv = m[rng.below(m.len())];
            st.make_move(mv);
            steps += 1;
        }
        if st.over {
            finished += 1;
        }
    }
    let dt = t.elapsed().as_secs_f64();
    eprintln!(
        "rust engine  games={games} positions={positions} finished={finished} \
         time={dt:.3}s  => {:.0} pos/s",
        positions as f64 / dt
    );
}
