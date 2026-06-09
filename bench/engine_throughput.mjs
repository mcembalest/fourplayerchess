// Single-threaded random self-play throughput on the REAL rules.js — the
// apples-to-apples counterpart to `throughput.rs` (real fpc-core). Same workload:
// uniform-random legal move each ply, draw bookkeeping ON, snapshots OFF, capped
// at max_steps. Measures positions/sec of the actual production rules engine.
//
//   node bench/engine_throughput.mjs [games] [max_steps]
//   bun  bench/engine_throughput.mjs [games] [max_steps]

import fs from "node:fs";
import vm from "node:vm";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const src = fs.readFileSync(path.join(root, "rules.js"), "utf8");

// DOM stubs so any UI hooks are harmless no-ops (same as oracle.mjs).
const makeEl = () => ({
  textContent: "", innerHTML: "", className: "",
  classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
  style: {}, addEventListener() {}, appendChild() {}, insertAdjacentHTML() {}, setAttribute() {},
});
const document = {
  getElementById: () => makeEl(), createElement: () => makeEl(),
  querySelector: () => makeEl(), querySelectorAll: () => [],
};
const sandbox = { document, setTimeout: () => 0, clearTimeout: () => {}, console };
vm.createContext(sandbox);

// Driver appended in rules.js's scope (closes over G, newGame, makeMove, ...).
const driver = `
function __mulberry32(a){ return function(){
  a |= 0; a = (a + 0x6D2B79F5) | 0;
  let t = Math.imul(a ^ (a >>> 15), 1 | a);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}; }
globalThis.__throughput = function(games, maxSteps){
  let positions = 0, finished = 0;
  for (let g = 0; g < games; g++){
    const rng = __mulberry32((0xBEEF + g) >>> 0);
    newGame();
    G.snapshots = null;          // match Rust: no snapshotting in the hot loop
    let steps = 0;
    while (!G.over && steps < maxSteps){
      positions++;
      const ms = G.currentLegal;
      makeMove(ms[Math.floor(rng() * ms.length)]);
      steps++;
    }
    if (G.over) finished++;
  }
  return { positions, finished };
};
`;
vm.runInContext(src + "\n" + driver, sandbox, { filename: "rules.js" });

const games = parseInt(process.argv[2] || "2000", 10);
const maxSteps = parseInt(process.argv[3] || "250", 10);
const rt = (typeof Bun !== "undefined") ? "bun " : "node";

// warm the JIT, then measure
sandbox.__throughput(20, maxSteps);
const t = performance.now();
const { positions, finished } = sandbox.__throughput(games, maxSteps);
const dt = (performance.now() - t) / 1000;
console.error(
  `${rt} engine  games=${games} positions=${positions} finished=${finished} ` +
  `time=${dt.toFixed(3)}s  => ${(positions / dt).toFixed(0)} pos/s`
);
