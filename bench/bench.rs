// Language shootout — Rust. Build: rustc -O bench/bench.rs -o bench/bench_rust
// Run: ./bench/bench_rust rollout [steps] | ./bench/bench_rust mlp [iters]
// Two kernels that bracket the 4PC workload:
//   rollout — board scan + sliding move-gen + RNG + make-move (branchy scalar,
//             proxy for self-play/search)
//   mlp     — 52->128->128->4 value-net forward, one sample at a time (proxy
//             for per-leaf eval cost)
// Same algorithm + same LCG in every language => the printed checksum must match.

const N: usize = 14;
const BOARD: usize = N * N;
const F: usize = 52;
const H: usize = 128;

struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Lcg { Lcg(seed) }
    #[inline]
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    #[inline]
    fn unit(&mut self) -> f64 {
        (self.next_u32() as f64) / 4294967296.0 * 2.0 - 1.0
    }
}

fn rollout(steps: usize) -> u64 {
    let dirs: [(i32, i32); 8] =
        [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)];
    let mut board = [0i32; BOARD];
    for i in 0..BOARD {
        board[i] = if i % 5 == 0 { 1 } else { 0 };
    }
    let mut rng = Lcg::new(0x1234_5678);
    let mut moves: Vec<i32> = Vec::with_capacity(2048);
    let mut total: u64 = 0;
    for _ in 0..steps {
        moves.clear();
        for idx in 0..BOARD {
            if board[idx] == 0 {
                continue;
            }
            let r = (idx / N) as i32;
            let c = (idx % N) as i32;
            for d in 0..8 {
                let (dr, dc) = dirs[d];
                let mut nr = r + dr;
                let mut nc = c + dc;
                while nr >= 0 && nr < N as i32 && nc >= 0 && nc < N as i32 {
                    let ni = (nr * N as i32 + nc) as usize;
                    if board[ni] != 0 {
                        moves.push(((idx as i32) << 8) | ni as i32);
                        break;
                    }
                    moves.push(((idx as i32) << 8) | ni as i32);
                    nr += dr;
                    nc += dc;
                }
            }
        }
        total = total.wrapping_add(moves.len() as u64);
        if !moves.is_empty() {
            let m = moves[(rng.next_u32() as usize) % moves.len()];
            let from = (m >> 8) as usize;
            let to = (m & 0xFF) as usize;
            // swap (synthetic steady state: keeps piece population constant)
            let tmp = board[to];
            board[to] = board[from];
            board[from] = tmp;
        }
    }
    total
}

fn mlp(iters: usize) -> f64 {
    let mut rng = Lcg::new(0x9E37_79B9);
    let mut w1 = vec![0.0f64; H * F];
    let mut b1 = vec![0.0f64; H];
    let mut w2 = vec![0.0f64; H * H];
    let mut b2 = vec![0.0f64; H];
    let mut w3 = vec![0.0f64; 4 * H];
    let mut b3 = vec![0.0f64; 4];
    for v in w1.iter_mut() { *v = rng.unit() * 0.1; }
    for v in b1.iter_mut() { *v = rng.unit() * 0.1; }
    for v in w2.iter_mut() { *v = rng.unit() * 0.1; }
    for v in b2.iter_mut() { *v = rng.unit() * 0.1; }
    for v in w3.iter_mut() { *v = rng.unit() * 0.1; }
    for v in b3.iter_mut() { *v = rng.unit() * 0.1; }
    let mut x = vec![0.0f64; F];
    for v in x.iter_mut() { *v = rng.unit(); }

    let mut acc = 0.0f64;
    let mut h1 = vec![0.0f64; H];
    let mut h2 = vec![0.0f64; H];
    for n in 0..iters {
        for o in 0..H {
            let mut s = b1[o];
            let row = o * F;
            for j in 0..F { s += w1[row + j] * x[j]; }
            h1[o] = if s > 0.0 { s } else { 0.0 };
        }
        for o in 0..H {
            let mut s = b2[o];
            let row = o * H;
            for j in 0..H { s += w2[row + j] * h1[j]; }
            h2[o] = if s > 0.0 { s } else { 0.0 };
        }
        let mut out = [0.0f64; 4];
        for o in 0..4 {
            let mut s = b3[o];
            let row = o * H;
            for j in 0..H { s += w3[row + j] * h2[j]; }
            out[o] = s;
        }
        acc += out[0] + out[1] + out[2] + out[3];
        x[n % F] += 1e-6 * (out[0] - out[1]); // defeat hoisting/DCE
    }
    acc
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(|s| s.as_str()).unwrap_or("rollout");
    let t = std::time::Instant::now();
    match which {
        "rollout" => {
            let steps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50_000);
            let chk = rollout(steps);
            eprintln!("rust rollout  steps={steps} checksum={chk} time={:.3}s", t.elapsed().as_secs_f64());
        }
        "mlp" => {
            let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200_000);
            let chk = mlp(iters);
            eprintln!("rust mlp      iters={iters} checksum={chk:.6} time={:.3}s", t.elapsed().as_secs_f64());
        }
        _ => eprintln!("usage: bench_rust rollout|mlp [count]"),
    }
}
