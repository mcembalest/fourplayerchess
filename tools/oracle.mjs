// Oracle: loads the real game.js logic in a DOM-stubbed VM and plays seeded
// random games, dumping every position + its legal moves to JSON. This is the
// ground truth the Rust port (fpc-core) is validated against.
//
//   node tools/oracle.mjs [numGames] [baseSeed]
//
// Output: crates/fpc-core/tests/data/oracle.json

import fs from "node:fs";
import vm from "node:vm";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// rules.js = the DOM-free logic+state half of the old game.js (ui.js is the rest).
const src = fs.readFileSync(path.join(root, "rules.js"), "utf8");

// Minimal DOM stubs so game.js's render/wire-up code is harmless no-ops.
const makeEl = () => ({
  textContent: "",
  innerHTML: "",
  className: "",
  classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
  style: {},
  addEventListener() {},
  appendChild() {},
  insertAdjacentHTML() {},
  setAttribute() {},
});
const document = {
  getElementById: () => makeEl(),
  createElement: () => makeEl(),
  querySelector: () => makeEl(),
  querySelectorAll: () => [],
};

// setTimeout is a no-op so the JS bots never auto-move; our driver moves everyone.
const sandbox = { document, setTimeout: () => 0, clearTimeout: () => {}, console };
vm.createContext(sandbox);

// Driver appended in the same script scope, so it closes over game.js's
// lexical `G` and global functions (newGame, makeMove, ...).
// Board as a 196-char string, row-major over all 14x14 cells: "RP" piece,
// ".." empty, "##" blocked. Rust parses it the same way.
const epilogue = `
function __serializeBoard(b){
  let s = "";
  for (let r=0;r<14;r++) for (let c=0;c<14;c++){
    if (!isPlayable(r,c)) { s += "##"; continue; }
    const p = b[r][c];
    s += p ? (p.color + p.type) : "..";
  }
  return s;
}
function __snapshot(){
  return {
    board: __serializeBoard(G.board),
    eliminated: [...G.eliminated],
    scores: { R:G.scores.R, B:G.scores.B, Y:G.scores.Y, G:G.scores.G },
    current: G.current,
    legal: G.currentLegal.map(m=>({fr:m.fr,fc:m.fc,tr:m.tr,tc:m.tc,promo:!!m.promo})),
  };
}
function __mulberry32(a){
  return function(){
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
// Capture-biased random pick (breadth of unusual positions).
function __pickRandom(rng, capBias){
  const ms = G.currentLegal;
  const caps = ms.filter(m => !!G.board[m.tr][m.tc]);
  const pool = (caps.length && rng() < capBias) ? caps : ms;
  return pool[Math.floor(rng() * pool.length)];
}

// Heuristic pick: the same scoring game.js's botMove uses (so games actually
// reach checkmates, eliminations, and game-over), but seeded and side-agnostic.
function __pickHeuristic(rng){
  const color = G.current, moves = G.currentLegal;
  let best = null, bestScore = -1e9;
  for (const mv of moves){
    let s = rng() * 1.5;
    const cap = G.board[mv.tr][mv.tc];
    const capVal = cap ? VALUE[cap.type] : 0;
    s += capVal * 10;
    if (mv.promo) s += 80;
    const nb = cloneBoard(G.board); applyTo(nb, mv);
    const pieceType = mv.promo ? "Q" : G.board[mv.fr][mv.fc].type;
    if (attacked(nb, G.eliminated, mv.tr, mv.tc, color))
      s -= Math.max(0, VALUE[pieceType] - capVal) * 7;
    for (const o of ORDER){
      if (o === color || G.eliminated.has(o)) continue;
      if (kingAttacked(nb, G.eliminated, o)){
        s += 3;
        if (legalMoves(nb, G.eliminated, o).length === 0) s += 1000;
      }
    }
    if (s > bestScore){ bestScore = s; best = mv; }
  }
  return best;
}

globalThis.__run = function(mode, numGames, baseSeed, maxSteps, capBias){
  const games = [];
  for (let g = 0; g < numGames; g++){
    const rng = __mulberry32((baseSeed + g) >>> 0);
    newGame();
    const steps = [];
    while (!G.over && steps.length < maxSteps){
      const snap = __snapshot();
      const mv = mode === "heuristic" ? __pickHeuristic(rng) : __pickRandom(rng, capBias);
      snap.chosen = { fr:mv.fr, fc:mv.fc, tr:mv.tr, tc:mv.tc, promo:!!mv.promo };
      steps.push(snap);
      makeMove(mv);
    }
    games.push({
      steps,
      finalScores: { R:G.scores.R, B:G.scores.B, Y:G.scores.Y, G:G.scores.G },
      eliminated: [...G.eliminated],
      over: G.over,
    });
  }
  return games;
};
`;

vm.runInContext(src + "\n" + epilogue, sandbox, { filename: "rules.js" });

// Mixed dataset: heuristic games (reach checkmates / eliminations / game-over)
// + capture-biased random games (breadth of unusual positions).
// maxSteps high enough to let the new draw rules terminate games (frontend saw
// random games run ~842 plies avg, 1252 max), so diff.rs exercises draw detection.
const heuristic = sandbox.__run("heuristic", 40, 1300, 400, 0.0);
const random = sandbox.__run("random", 30, 9000, 1500, 0.6);
const games = heuristic.concat(random);

const outDir = path.join(root, "crates", "fpc-core", "tests", "data");
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "oracle.json"), JSON.stringify(games));

const positions = games.reduce((a, g) => a + g.steps.length, 0);
const finished = games.filter((g) => g.over).length;
const withElim = games.filter((g) => g.eliminated.length > 0).length;
console.error(
  `wrote ${games.length} games (${finished} finished, ${withElim} with >=1 elimination), ${positions} positions`
);
