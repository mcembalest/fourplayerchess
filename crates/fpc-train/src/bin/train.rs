//! Trains the value net (FEAT_DIM -> HIDDEN -> HIDDEN -> 4) on self-play data
//! with mini-batch SGD + momentum, MSE to final score-shares. Writes the flat
//! f32 weight file that fpc_agents::Net::load expects.
//!
//!   cargo run -p fpc-train --release --bin train -- [epochs] [max_rows]
//!
//! Output: data/model.bin

use std::io::Write;

use fpc_agents::{Rng, HIDDEN};
use fpc_core::FEAT_DIM;

fn read_f32(path: &str) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("read data file");
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let epochs: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let max_rows: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300_000);

    let f = FEAT_DIM;
    let h = HIDDEN;
    let x = read_f32("data/X.bin");
    let y = read_f32("data/Y.bin");
    let n_all = y.len() / 4;
    assert_eq!(x.len(), n_all * f);

    let mut rng = Rng::new(0x5EED);

    // shuffle indices and cap dataset
    let mut idx: Vec<usize> = (0..n_all).collect();
    for i in (1..idx.len()).rev() {
        idx.swap(i, rng.below(i + 1));
    }
    idx.truncate(max_rows.min(n_all));
    let n = idx.len();

    // params, Xavier-ish init
    let mut w1 = init(&mut rng, h * f, f, h);
    let mut b1 = vec![0.0f32; h];
    let mut w2 = init(&mut rng, h * h, h, h);
    let mut b2 = vec![0.0f32; h];
    let mut w3 = init(&mut rng, 4 * h, h, 4);
    let mut b3 = vec![0.0f32; 4];

    // momentum buffers
    let mut vw1 = vec![0.0f32; w1.len()];
    let mut vb1 = vec![0.0f32; b1.len()];
    let mut vw2 = vec![0.0f32; w2.len()];
    let mut vb2 = vec![0.0f32; b2.len()];
    let mut vw3 = vec![0.0f32; w3.len()];
    let mut vb3 = vec![0.0f32; b3.len()];

    let batch = 256usize;
    let lr = 0.05f32;
    let mom = 0.9f32;

    for ep in 0..epochs {
        for i in (1..n).rev() {
            idx.swap(i, rng.below(i + 1));
        }
        let mut epoch_loss = 0.0f64;
        let mut seen = 0usize;

        let mut bstart = 0;
        while bstart < n {
            let bend = (bstart + batch).min(n);
            let bsize = (bend - bstart) as f32;

            // gradient accumulators
            let mut gw1 = vec![0.0f32; w1.len()];
            let mut gb1 = vec![0.0f32; b1.len()];
            let mut gw2 = vec![0.0f32; w2.len()];
            let mut gb2 = vec![0.0f32; b2.len()];
            let mut gw3 = vec![0.0f32; w3.len()];
            let mut gb3 = vec![0.0f32; b3.len()];

            for &row in &idx[bstart..bend] {
                let xi = &x[row * f..row * f + f];
                let yi = &y[row * 4..row * 4 + 4];

                // forward
                let mut a1 = vec![0.0f32; h];
                for o in 0..h {
                    let mut s = b1[o];
                    let r = o * f;
                    for j in 0..f {
                        s += w1[r + j] * xi[j];
                    }
                    a1[o] = s.max(0.0);
                }
                let mut a2 = vec![0.0f32; h];
                for o in 0..h {
                    let mut s = b2[o];
                    let r = o * h;
                    for j in 0..h {
                        s += w2[r + j] * a1[j];
                    }
                    a2[o] = s.max(0.0);
                }
                let mut out = [0.0f32; 4];
                for o in 0..4 {
                    let mut s = b3[o];
                    let r = o * h;
                    for j in 0..h {
                        s += w3[r + j] * a2[j];
                    }
                    out[o] = s;
                }

                // MSE loss + output grad (dL/dout = 2/4*(out-y))
                let mut dout = [0.0f32; 4];
                for o in 0..4 {
                    let e = out[o] - yi[o];
                    epoch_loss += (e * e) as f64;
                    dout[o] = 0.5 * e; // 2*e/4
                }

                // backward layer 3
                let mut da2 = vec![0.0f32; h];
                for o in 0..4 {
                    let r = o * h;
                    gb3[o] += dout[o];
                    for j in 0..h {
                        gw3[r + j] += dout[o] * a2[j];
                        da2[j] += w3[r + j] * dout[o];
                    }
                }
                // through ReLU2
                for j in 0..h {
                    if a2[j] <= 0.0 {
                        da2[j] = 0.0;
                    }
                }
                // backward layer 2
                let mut da1 = vec![0.0f32; h];
                for o in 0..h {
                    let r = o * h;
                    gb2[o] += da2[o];
                    for j in 0..h {
                        gw2[r + j] += da2[o] * a1[j];
                        da1[j] += w2[r + j] * da2[o];
                    }
                }
                // through ReLU1
                for j in 0..h {
                    if a1[j] <= 0.0 {
                        da1[j] = 0.0;
                    }
                }
                // backward layer 1
                for o in 0..h {
                    let r = o * f;
                    gb1[o] += da1[o];
                    for j in 0..f {
                        gw1[r + j] += da1[o] * xi[j];
                    }
                }
            }

            // SGD + momentum update (gradient averaged over batch)
            step(&mut w1, &mut vw1, &gw1, lr, mom, bsize);
            step(&mut b1, &mut vb1, &gb1, lr, mom, bsize);
            step(&mut w2, &mut vw2, &gw2, lr, mom, bsize);
            step(&mut b2, &mut vb2, &gb2, lr, mom, bsize);
            step(&mut w3, &mut vw3, &gw3, lr, mom, bsize);
            step(&mut b3, &mut vb3, &gb3, lr, mom, bsize);

            seen += (bend - bstart) * 4;
            bstart = bend;
        }
        eprintln!("epoch {:>3}  mse {:.5}", ep, epoch_loss / seen as f64);
    }

    // save in Net::load order
    std::fs::create_dir_all("data")?;
    let mut out = std::io::BufWriter::new(std::fs::File::create("data/model.bin")?);
    for buf in [&w1, &b1, &w2, &b2, &w3, &b3] {
        for v in buf {
            out.write_all(&v.to_le_bytes())?;
        }
    }
    out.flush()?;
    eprintln!("wrote data/model.bin ({} rows trained)", n);
    Ok(())
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
