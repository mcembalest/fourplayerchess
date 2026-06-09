//! Trains the value net (FEAT_DIM -> [Lin,LN,ReLU] -> [Lin,LN,ReLU] -> 4) on
//! self-play data. PQN-style: LayerNorm before each ReLU stabilizes bootstrapped
//! TD; the target is a TD(λ) return (λ=1 reproduces the pure Monte-Carlo label).
//! Mini-batch SGD + momentum, MSE, small ℓ₂ on the output layer.
//!
//!   cargo run -p fpc-train --release --bin train -- [epochs] [max_rows] [lambda] [bootstrap.bin?] [decay]
//!
//! Data: if data/buffer/<tag>/ generations exist (written by selfplay with a
//! tag), ALL of them are loaded, oldest..newest by tag order, and each epoch
//! samples rows with probability decay^age (age = generations back from the
//! newest; decay=1 keeps everything uniformly). Otherwise the legacy flat
//! data/{X,Y,lens}.bin files are used.
//!
//! TD(λ) targets are computed once up front from the frozen `bootstrap` net
//! (one round of generalized policy iteration). With λ<1 a bootstrap net is
//! required; without one we fall back to λ=1 (Monte-Carlo).
//!
//! Output: data/model.bin

use std::io::Write;

use fpc_agents::{dot, Net, Rng, HIDDEN, LN_EPS};
use fpc_core::FEAT_DIM;
use rayon::prelude::*;

/// Read-only network parameters, passed to the parallel per-sample backward.
struct Params<'a> {
    w1: &'a [f32],
    b1: &'a [f32],
    g1: &'a [f32],
    n1: &'a [f32],
    w2: &'a [f32],
    b2: &'a [f32],
    g2: &'a [f32],
    n2: &'a [f32],
    w3: &'a [f32],
    b3: &'a [f32],
}

/// Gradient (and loss) accumulator, shaped like the parameters.
struct Grads {
    w1: Vec<f32>,
    b1: Vec<f32>,
    g1: Vec<f32>,
    n1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
    g2: Vec<f32>,
    n2: Vec<f32>,
    w3: Vec<f32>,
    b3: Vec<f32>,
    loss: f64,
}

impl Grads {
    fn zero(f: usize, h: usize) -> Grads {
        Grads {
            w1: vec![0.0; h * f],
            b1: vec![0.0; h],
            g1: vec![0.0; h],
            n1: vec![0.0; h],
            w2: vec![0.0; h * h],
            b2: vec![0.0; h],
            g2: vec![0.0; h],
            n2: vec![0.0; h],
            w3: vec![0.0; 4 * h],
            b3: vec![0.0; 4],
            loss: 0.0,
        }
    }
    fn add(&mut self, o: &Grads) {
        for (a, b) in self.w1.iter_mut().zip(&o.w1) { *a += b; }
        for (a, b) in self.b1.iter_mut().zip(&o.b1) { *a += b; }
        for (a, b) in self.g1.iter_mut().zip(&o.g1) { *a += b; }
        for (a, b) in self.n1.iter_mut().zip(&o.n1) { *a += b; }
        for (a, b) in self.w2.iter_mut().zip(&o.w2) { *a += b; }
        for (a, b) in self.b2.iter_mut().zip(&o.b2) { *a += b; }
        for (a, b) in self.g2.iter_mut().zip(&o.g2) { *a += b; }
        for (a, b) in self.n2.iter_mut().zip(&o.n2) { *a += b; }
        for (a, b) in self.w3.iter_mut().zip(&o.w3) { *a += b; }
        for (a, b) in self.b3.iter_mut().zip(&o.b3) { *a += b; }
        self.loss += o.loss;
    }
}

/// Forward + backward for one sample, accumulating gradients into `g`.
/// Stack buffers throughout — no per-sample heap allocation.
fn backprop(p: &Params, h: usize, f: usize, xi: &[f32], ti: &[f32], g: &mut Grads) {
    let (a1, zhat1, inv1) = lin_ln_relu(xi, p.w1, p.b1, p.g1, p.n1, h, f);
    let (a2, zhat2, inv2) = lin_ln_relu(&a1, p.w2, p.b2, p.g2, p.n2, h, h);
    let mut out = [0.0f32; 4];
    for o in 0..4 {
        out[o] = p.b3[o] + dot(&p.w3[o * h..(o + 1) * h], &a2);
    }
    let mut dout = [0.0f32; 4];
    for o in 0..4 {
        let e = out[o] - ti[o];
        g.loss += (e * e) as f64;
        dout[o] = 0.5 * e;
    }
    // layer 3 (linear)
    let mut da2 = [0.0f32; HIDDEN];
    for o in 0..4 {
        let r = o * h;
        g.b3[o] += dout[o];
        for j in 0..h {
            g.w3[r + j] += dout[o] * a2[j];
            da2[j] += p.w3[r + j] * dout[o];
        }
    }
    let dz2 = ln_relu_backward(&da2, &a2, &zhat2, inv2, p.g2, &mut g.g2, &mut g.n2);
    let mut da1 = [0.0f32; HIDDEN];
    for o in 0..h {
        let r = o * h;
        g.b2[o] += dz2[o];
        for j in 0..h {
            g.w2[r + j] += dz2[o] * a1[j];
            da1[j] += p.w2[r + j] * dz2[o];
        }
    }
    let dz1 = ln_relu_backward(&da1, &a1, &zhat1, inv1, p.g1, &mut g.g1, &mut g.n1);
    for o in 0..h {
        let r = o * f;
        g.b1[o] += dz1[o];
        for j in 0..f {
            g.w1[r + j] += dz1[o] * xi[j];
        }
    }
}

fn read_f32(path: impl AsRef<std::path::Path>) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("read data file");
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn read_u32(path: impl AsRef<std::path::Path>) -> Vec<u32> {
    let bytes = std::fs::read(path).expect("read data file");
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Load training data: every generation under data/buffer/ (oldest..newest by
/// tag order) if any exist, else the legacy flat files. Returns (x, y, lens,
/// per-row age) where age = generations back from the newest (newest = 0).
fn load_data(f: usize) -> (Vec<f32>, Vec<f32>, Vec<u32>, Vec<u32>) {
    let mut gens: Vec<std::path::PathBuf> = std::fs::read_dir("data/buffer")
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    gens.sort();
    if gens.is_empty() {
        let x = read_f32("data/X.bin");
        let y = read_f32("data/Y.bin");
        let lens = read_u32("data/lens.bin");
        let age = vec![0u32; y.len() / 4];
        return (x, y, lens, age);
    }
    let (mut x, mut y, mut lens, mut age) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (gi, dir) in gens.iter().enumerate() {
        let gx = read_f32(dir.join("X.bin"));
        let gy = read_f32(dir.join("Y.bin"));
        let gl = read_u32(dir.join("lens.bin"));
        let rows = gy.len() / 4;
        assert_eq!(gx.len(), rows * f, "bad X size in {}", dir.display());
        assert_eq!(gl.iter().map(|&l| l as usize).sum::<usize>(), rows, "bad lens in {}", dir.display());
        let a = (gens.len() - 1 - gi) as u32;
        eprintln!("buffer gen {} : {} rows (age {a})", dir.display(), rows);
        x.extend_from_slice(&gx);
        y.extend_from_slice(&gy);
        lens.extend_from_slice(&gl);
        age.extend(std::iter::repeat(a).take(rows));
    }
    (x, y, lens, age)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let epochs: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let max_rows: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let mut lambda: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let bootstrap: Option<&String> = args.get(4);
    let decay: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(1.0);

    let f = FEAT_DIM;
    let h = HIDDEN;
    let (x, y, lens, age) = load_data(f);
    let n_all = y.len() / 4;
    assert_eq!(x.len(), n_all * f);
    assert_eq!(lens.iter().map(|&l| l as usize).sum::<usize>(), n_all);
    // Per-row inclusion probability: decay^age (newest generation always 1).
    let weight: Vec<f64> = age.iter().map(|&a| decay.powi(a as i32)).collect();

    // ---- TD(λ) targets, computed once from the frozen bootstrap net ----
    let mut target = y.clone(); // λ=1 default: target = terminal reward (MC)
    if lambda < 1.0 {
        match bootstrap {
            None => {
                eprintln!("lambda<1 but no bootstrap net given; falling back to lambda=1 (MC)");
                lambda = 1.0;
            }
            Some(path) => {
                let net = Net::load(path).expect("load bootstrap net");
                // V(s) for every recorded state (rows are independent -> rayon).
                let mut v = vec![0.0f32; n_all * 4];
                v.par_chunks_mut(4).enumerate().for_each(|(r, out)| {
                    out.copy_from_slice(&net.forward(&x[r * f..r * f + f]));
                });
                // Per trajectory, fold λ-returns backward (γ=1, terminal reward = y).
                let mut start = 0usize;
                for &len in &lens {
                    let len = len as usize;
                    if len == 0 {
                        continue;
                    }
                    let last = start + len - 1;
                    // last row's target stays the terminal reward (already in `target`).
                    for i in (0..len - 1).rev() {
                        let row = start + i;
                        let nxt = row + 1;
                        for k in 0..4 {
                            let vnext = v[nxt * 4 + k];
                            let tnext = target[nxt * 4 + k];
                            target[row * 4 + k] = (1.0 - lambda) * vnext + lambda * tnext;
                        }
                    }
                    let _ = last;
                    start += len;
                }
                eprintln!("computed TD(λ={lambda}) targets from bootstrap {path}");
            }
        }
    }

    let mut rng = Rng::new(0x5EED);

    // params: Linear weights Xavier-ish, LayerNorm gain=1 bias=0
    let mut w1 = init(&mut rng, h * f, f, h);
    let mut b1 = vec![0.0f32; h];
    let mut g1 = vec![1.0f32; h];
    let mut n1 = vec![0.0f32; h];
    let mut w2 = init(&mut rng, h * h, h, h);
    let mut b2 = vec![0.0f32; h];
    let mut g2 = vec![1.0f32; h];
    let mut n2 = vec![0.0f32; h];
    let mut w3 = init(&mut rng, 4 * h, h, 4);
    let mut b3 = vec![0.0f32; 4];

    // momentum buffers
    let mut vw1 = vec![0.0f32; w1.len()];
    let mut vb1 = vec![0.0f32; b1.len()];
    let mut vg1 = vec![0.0f32; g1.len()];
    let mut vn1 = vec![0.0f32; n1.len()];
    let mut vw2 = vec![0.0f32; w2.len()];
    let mut vb2 = vec![0.0f32; b2.len()];
    let mut vg2 = vec![0.0f32; g2.len()];
    let mut vn2 = vec![0.0f32; n2.len()];
    let mut vw3 = vec![0.0f32; w3.len()];
    let mut vb3 = vec![0.0f32; b3.len()];

    let batch = 256usize;
    let lr = 0.05f32;
    let mom = 0.9f32;
    let wd = 1e-4f32; // ℓ₂ on output layer (PQN regularizer)

    let mut n = 0usize;
    for ep in 0..epochs {
        // Per-epoch sample: keep each row with prob decay^age, shuffle, cap.
        let mut idx: Vec<usize> = (0..n_all)
            .filter(|&r| decay >= 1.0 || rng.next_f64() < weight[r])
            .collect();
        for i in (1..idx.len()).rev() {
            idx.swap(i, rng.below(i + 1));
        }
        idx.truncate(max_rows.min(idx.len()));
        n = idx.len();
        let mut epoch_loss = 0.0f64;
        let mut seen = 0usize;

        let mut bstart = 0;
        while bstart < n {
            let bend = (bstart + batch).min(n);
            let bsize = (bend - bstart) as f32;

            let p = Params {
                w1: &w1, b1: &b1, g1: &g1, n1: &n1,
                w2: &w2, b2: &b2, g2: &g2, n2: &n2,
                w3: &w3, b3: &b3,
            };
            // Data-parallel batch gradient: each row's backward is independent;
            // fold into thread-local Grads, then sum (PQN scales via parallelism).
            let grad = idx[bstart..bend]
                .par_iter()
                .fold(
                    || Grads::zero(f, h),
                    |mut g, &row| {
                        let xi = &x[row * f..row * f + f];
                        let ti = &target[row * 4..row * 4 + 4];
                        backprop(&p, h, f, xi, ti, &mut g);
                        g
                    },
                )
                .reduce(
                    || Grads::zero(f, h),
                    |mut a, b| { a.add(&b); a },
                );

            epoch_loss += grad.loss;
            step(&mut w1, &mut vw1, &grad.w1, lr, mom, bsize);
            step(&mut b1, &mut vb1, &grad.b1, lr, mom, bsize);
            step(&mut g1, &mut vg1, &grad.g1, lr, mom, bsize);
            step(&mut n1, &mut vn1, &grad.n1, lr, mom, bsize);
            step(&mut w2, &mut vw2, &grad.w2, lr, mom, bsize);
            step(&mut b2, &mut vb2, &grad.b2, lr, mom, bsize);
            step(&mut g2, &mut vg2, &grad.g2, lr, mom, bsize);
            step(&mut n2, &mut vn2, &grad.n2, lr, mom, bsize);
            step_decay(&mut w3, &mut vw3, &grad.w3, lr, mom, bsize, wd);
            step(&mut b3, &mut vb3, &grad.b3, lr, mom, bsize);

            seen += (bend - bstart) * 4;
            bstart = bend;
        }
        eprintln!("epoch {:>3}  mse {:.5}", ep, epoch_loss / seen as f64);
    }

    // save in Net::from_bytes order
    std::fs::create_dir_all("data")?;
    let mut out = std::io::BufWriter::new(std::fs::File::create("data/model.bin")?);
    for buf in [&w1, &b1, &g1, &n1, &w2, &b2, &g2, &n2, &w3, &b3] {
        for v in buf.iter() {
            out.write_all(&v.to_le_bytes())?;
        }
    }
    out.flush()?;
    eprintln!("wrote data/model.bin ({} rows trained, λ={lambda})", n);
    Ok(())
}

/// Linear -> LayerNorm -> ReLU. Returns (post-relu activations, normalized
/// pre-activations zhat, inverse std) — the last two are needed for LN backward.
fn lin_ln_relu(
    x: &[f32],
    w: &[f32],
    b: &[f32],
    g: &[f32],
    n: &[f32],
    h: usize,
    fin: usize,
) -> ([f32; HIDDEN], [f32; HIDDEN], f32) {
    let mut z = [0.0f32; HIDDEN];
    for o in 0..h {
        z[o] = b[o] + dot(&w[o * fin..(o + 1) * fin], x);
    }
    let mean: f32 = z.iter().sum::<f32>() / h as f32;
    let var: f32 = z.iter().map(|&v| (v - mean) * (v - mean)).sum::<f32>() / h as f32;
    let inv = 1.0 / (var + LN_EPS).sqrt();
    let mut zhat = [0.0f32; HIDDEN];
    let mut a = [0.0f32; HIDDEN];
    for o in 0..h {
        zhat[o] = (z[o] - mean) * inv;
        let yv = g[o] * zhat[o] + n[o];
        a[o] = yv.max(0.0);
    }
    (a, zhat, inv)
}

/// Backprop through ReLU then LayerNorm. `da` is grad wrt post-ReLU activations
/// `a`; accumulates LN gain/bias grads into `gg`/`gn`; returns grad wrt the
/// pre-LN preactivation.
fn ln_relu_backward(
    da: &[f32],
    a: &[f32],
    zhat: &[f32],
    inv: f32,
    g: &[f32],
    gg: &mut [f32],
    gn: &mut [f32],
) -> [f32; HIDDEN] {
    let h = da.len();
    // through ReLU: dy = da * 1[a>0]
    let mut dy = [0.0f32; HIDDEN];
    for o in 0..h {
        dy[o] = if a[o] > 0.0 { da[o] } else { 0.0 };
    }
    // LN param grads + grad wrt normalized zhat
    let mut dzhat = [0.0f32; HIDDEN];
    for o in 0..h {
        gg[o] += dy[o] * zhat[o];
        gn[o] += dy[o];
        dzhat[o] = dy[o] * g[o];
    }
    let mean_dzhat: f32 = dzhat.iter().sum::<f32>() / h as f32;
    let mean_dzhat_zhat: f32 =
        dzhat.iter().zip(zhat.iter()).map(|(&d, &z)| d * z).sum::<f32>() / h as f32;
    let mut dz = [0.0f32; HIDDEN];
    for o in 0..h {
        dz[o] = inv * (dzhat[o] - mean_dzhat - zhat[o] * mean_dzhat_zhat);
    }
    dz
}

fn init(rng: &mut Rng, len: usize, fan_in: usize, fan_out: usize) -> Vec<f32> {
    let a = (6.0 / (fan_in + fan_out) as f32).sqrt();
    (0..len)
        .map(|_| (rng.next_f64() as f32 * 2.0 - 1.0) * a)
        .collect()
}

fn step(w: &mut [f32], v: &mut [f32], g: &[f32], lr: f32, mom: f32, bsize: f32) {
    for i in 0..w.len() {
        v[i] = mom * v[i] - lr * g[i] / bsize;
        w[i] += v[i];
    }
}

/// SGD+momentum with decoupled ℓ₂ weight decay.
fn step_decay(w: &mut [f32], v: &mut [f32], g: &[f32], lr: f32, mom: f32, bsize: f32, wd: f32) {
    for i in 0..w.len() {
        v[i] = mom * v[i] - lr * g[i] / bsize;
        w[i] += v[i] - lr * wd * w[i];
    }
}
