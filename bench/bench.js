// Language shootout — JavaScript (run under Node and Bun).
//   node bench/bench.js rollout [steps] | bun bench/bench.js mlp [iters]
// Identical algorithm + LCG to bench.rs/.go/.py => checksum must match.
// Uses typed arrays (Int32Array/Float64Array) — the idiomatic fast-path for
// numeric JS, still a one-word change from plain arrays.

const N = 14, BOARD = N * N, F = 52, H = 128;

class Lcg {
  constructor(seed) { this.s = BigInt.asUintN(64, seed); }
  nextU32() {
    this.s = BigInt.asUintN(64, this.s * 6364136223846793005n + 1442695040888963407n);
    return Number(this.s >> 33n);
  }
  unit() { return this.nextU32() / 4294967296.0 * 2.0 - 1.0; }
}

function rollout(steps) {
  const dirs = [[-1,-1],[-1,0],[-1,1],[0,-1],[0,1],[1,-1],[1,0],[1,1]];
  const board = new Int32Array(BOARD);
  for (let i = 0; i < BOARD; i++) board[i] = (i % 5 === 0) ? 1 : 0;
  const rng = new Lcg(0x12345678n);
  let moves = new Int32Array(4096);
  let total = 0n;
  for (let st = 0; st < steps; st++) {
    let nm = 0;
    for (let idx = 0; idx < BOARD; idx++) {
      if (board[idx] === 0) continue;
      const r = (idx / N) | 0, c = idx % N;
      for (let d = 0; d < 8; d++) {
        const dr = dirs[d][0], dc = dirs[d][1];
        let nr = r + dr, nc = c + dc;
        while (nr >= 0 && nr < N && nc >= 0 && nc < N) {
          const ni = nr * N + nc;
          if (board[ni] !== 0) { moves[nm++] = (idx << 8) | ni; break; }
          moves[nm++] = (idx << 8) | ni;
          nr += dr; nc += dc;
        }
      }
    }
    total += BigInt(nm);
    if (nm > 0) {
      const m = moves[rng.nextU32() % nm];
      const from = m >> 8, to = m & 0xFF;
      const tmp = board[to]; board[to] = board[from]; board[from] = tmp;
    }
  }
  return total;
}

function mlp(iters) {
  const rng = new Lcg(0x9E3779B9n);
  const mk = (len, scale) => {
    const a = new Float64Array(len);
    for (let i = 0; i < len; i++) a[i] = rng.unit() * scale;
    return a;
  };
  const w1 = mk(H * F, 0.1), b1 = mk(H, 0.1);
  const w2 = mk(H * H, 0.1), b2 = mk(H, 0.1);
  const w3 = mk(4 * H, 0.1), b3 = mk(4, 0.1);
  const x = new Float64Array(F);
  for (let i = 0; i < F; i++) x[i] = rng.unit(); // matches weights-then-x draw order
  const h1 = new Float64Array(H), h2 = new Float64Array(H), out = new Float64Array(4);
  let acc = 0.0;
  for (let n = 0; n < iters; n++) {
    for (let o = 0; o < H; o++) {
      let s = b1[o]; const row = o * F;
      for (let j = 0; j < F; j++) s += w1[row + j] * x[j];
      h1[o] = s > 0 ? s : 0;
    }
    for (let o = 0; o < H; o++) {
      let s = b2[o]; const row = o * H;
      for (let j = 0; j < H; j++) s += w2[row + j] * h1[j];
      h2[o] = s > 0 ? s : 0;
    }
    for (let o = 0; o < 4; o++) {
      let s = b3[o]; const row = o * H;
      for (let j = 0; j < H; j++) s += w3[row + j] * h2[j];
      out[o] = s;
    }
    acc += out[0] + out[1] + out[2] + out[3];
    x[n % F] += 1e-6 * (out[0] - out[1]);
  }
  return acc;
}

const which = process.argv[2] || "rollout";
const cnt = parseInt(process.argv[3] || "0", 10);
const rt = (typeof Bun !== "undefined") ? "bun " : "node";
const t = performance.now();
if (which === "rollout") {
  const steps = cnt || 50000;
  const chk = rollout(steps);
  console.error(`${rt} rollout  steps=${steps} checksum=${chk} time=${((performance.now()-t)/1000).toFixed(3)}s`);
} else if (which === "mlp") {
  const iters = cnt || 200000;
  const chk = mlp(iters);
  console.error(`${rt} mlp      iters=${iters} checksum=${chk.toFixed(6)} time=${((performance.now()-t)/1000).toFixed(3)}s`);
} else {
  console.error("usage: bench.js rollout|mlp [count]");
}
