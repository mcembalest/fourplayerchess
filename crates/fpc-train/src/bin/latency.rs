//! Quick per-move latency probe for the live Expert bot (ParanoidNet d4 + temp),
//! to estimate browser cost (wasm ~2-3x native).
//!   cargo run -p fpc-train --release --bin latency -- data/champion.bin
use std::sync::Arc;
use std::time::Instant;
use fpc_agents::*;
use fpc_core::*;

fn main() {
    let path = std::env::args().nth(1).unwrap_or("data/champion.bin".into());
    let net = Arc::new(Net::load(&path).expect("load"));
    let kinds = [
        AgentKind::ParanoidNet { net, depth: 4, label: "pnet4".into() },
        AgentKind::Paranoid(4),
    ];

    // start position + a midgame position (after 24 random plies)
    let start = State::new_game();
    let mut mid = State::new_game();
    let mut r = Rng::new(7);
    for _ in 0..24 {
        if mid.over { break; }
        let m = mid.current_legal[r.below(mid.current_legal.len())];
        mid.make_move(m);
    }

    // leaf-eval split: features() vs net.forward() on the midgame position
    {
        let net = match &kinds[0] {
            AgentKind::ParanoidNet { net, .. } => net.clone(),
            _ => unreachable!(),
        };
        let n = 200_000;
        let t = Instant::now();
        let mut acc = 0.0f32;
        for _ in 0..n {
            acc += features(&mid)[0];
        }
        let feat_us = t.elapsed().as_secs_f64() * 1e6 / n as f64;
        let x = features(&mid);
        let t = Instant::now();
        for _ in 0..n {
            acc += net.forward(&x)[0];
        }
        let fwd_us = t.elapsed().as_secs_f64() * 1e6 / n as f64;
        eprintln!("leaf split (midgame): features {feat_us:.2} us, forward {fwd_us:.2} us  [{acc}]");
    }

    for kind in &kinds {
        for (name, st) in [("start", &start), ("midgame", &mid)] {
            let mut agent = kind.build_temp(0x1234, 0.12);
            // one warm call, then time a few
            let _ = agent.select(st);
            let n = 5;
            let t = Instant::now();
            for _ in 0..n { let _ = agent.select(st); }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            eprintln!(
                "{:>9} {name:>8}: {ms:.1} ms/move (native)  ~{:.1} ms est. wasm",
                kind.name(),
                ms * 2.5
            );
        }
    }
}
