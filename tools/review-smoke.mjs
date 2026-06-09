// Smoke test for the Review pipeline. Plays real games with rules.js (DOM-stubbed
// VM, like the oracle), captures G.snapshots + history, then reproduces exactly what
// review.js computes: per-node win-prob (fpc_eval) and per-move labels (engine
// bestMove vs played, scored by the mover's predicted-share loss — applied on a
// scratch rules.js state, so it's start-agnostic). Also asserts the Modern setup.
//
//   node tools/review-smoke.mjs
import fs from "node:fs";
import vm from "node:vm";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { initSync, fpc_eval, fpc_best_move } from "../pkg/fpc_wasm.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
initSync(fs.readFileSync(path.join(root, "pkg", "fpc_wasm_bg.wasm")));

const src = fs.readFileSync(path.join(root, "rules.js"), "utf8");
const makeEl = () => ({ textContent:"",innerHTML:"",className:"",
  classList:{add(){},remove(){},toggle(){},contains(){return false;}},
  style:{},addEventListener(){},appendChild(){},insertAdjacentHTML(){},setAttribute(){} });
const sandbox = { document:{ getElementById:()=>makeEl(), createElement:()=>makeEl(),
  querySelector:()=>makeEl(), querySelectorAll:()=>[] }, setTimeout:()=>0, clearTimeout:()=>{}, console };
vm.createContext(sandbox);

const driver = `
globalThis.__startRanks = function(){
  newGame();
  const red=[],yel=[],blue=[],green=[];
  for(let c=3;c<=10;c++){ red.push(G.board[13][c].type); yel.push(G.board[0][c].type); }
  for(let r=3;r<=10;r++){ blue.push(G.board[r][0].type); green.push(G.board[r][13].type); }
  return { red:red.join(""), yel:yel.join(""), blue:blue.join(""), green:green.join("") };
};
globalThis.__playGame = function(seed){
  function rng(){ seed|=0; seed=(seed+0x6D2B79F5)|0; let t=Math.imul(seed^(seed>>>15),1|seed);
    t=(t+Math.imul(t^(t>>>7),61|t))^t; return ((t^(t>>>14))>>>0)/4294967296; }
  newGame();
  let steps=0;
  while(!G.over && steps<400){
    const color=G.current, moves=G.currentLegal; let best=null,bs=-1e9;
    for(const mv of moves){ let s=rng()*1.5; const cap=G.board[mv.tr][mv.tc];
      const cv=cap?VALUE[cap.type]:0; s+=cv*10; if(mv.promo)s+=80;
      const nb=cloneBoard(G.board); applyTo(nb,mv);
      const pt=mv.promo?"Q":G.board[mv.fr][mv.fc].type;
      if(attacked(nb,G.eliminated,mv.tr,mv.tc,color)) s-=Math.max(0,VALUE[pt]-cv)*7;
      for(const o of ORDER){ if(o===color||G.eliminated.has(o))continue;
        if(kingAttacked(nb,G.eliminated,o)){ s+=3; if(legalMoves(nb,G.eliminated,o).length===0)s+=1000; } }
      if(s>bs){bs=s;best=mv;} }
    makeMove(best); steps++;
  }
  return { snapshots:G.snapshots, history:serializeHistory(), over:G.over };
};
globalThis.__playRandom = function(seed, cap){
  function rng(){ seed|=0; seed=(seed+0x6D2B79F5)|0; let t=Math.imul(seed^(seed>>>15),1|seed);
    t=(t+Math.imul(t^(t>>>7),61|t))^t; return ((t^(t>>>14))>>>0)/4294967296; }
  newGame();
  let n=0;
  while(!G.over && n<cap){ const m=G.currentLegal[Math.floor(rng()*G.currentLegal.length)]; makeMove(m); n++; }
  return { snapshots:G.snapshots, history:serializeHistory(), over:G.over };
};
// Apply a hypothetical move on a scratch state (mirrors review.js rvShareAfter).
globalThis.__afterMove = function(snap, mv){
  G = { board:parseBoard(snap.board), eliminated:new Set(snap.eliminated.map(x=>x[0])),
        scores:{R:snap.scores.R,B:snap.scores.B,Y:snap.scores.Y,G:snap.scores.G},
        idx:ORDER.indexOf(snap.current), current:snap.current, currentLegal:[],
        lastMover:null,lastMove:null,selected:null,over:false,history:[],noProgress:0,repeats:{},snapshots:null };
  makeMove(mv);
  return serializePosition();
};`;
vm.runInContext(src + "\n" + driver, sandbox, { filename: "rules.js" });

let fail = 0;
const ok = (cond, msg) => { if(!cond){ console.error("FAIL:", msg); fail++; } };

// ---- Modern setup (Q on each player's left, K on the right) ----
const sr = sandbox.__startRanks();
ok(sr.red   === "RNBQKBNR", `Red back rank Modern (got ${sr.red})`);
ok(sr.yel   === "RNBKQBNR", `Yellow back rank Modern (got ${sr.yel})`);
ok(sr.blue  === "RNBQKBNR", `Blue back col Modern top->bottom (got ${sr.blue})`);
ok(sr.green === "RNBKQBNR", `Green back col Modern top->bottom (got ${sr.green})`);
console.error(`start: R=${sr.red} Y=${sr.yel} B=${sr.blue} G=${sr.green}`);

const evalNode = s => {
  if(s.current) return JSON.parse(fpc_eval(JSON.stringify(s)));
  const sc=s.scores, tot=sc.R+sc.B+sc.Y+sc.G;
  return tot<=0 ? [0.25,0.25,0.25,0.25] : [sc.R/tot,sc.B/tot,sc.Y/tot,sc.G/tot];
};
const ORDER = ["R","B","Y","G"];
const same = (a,b)=> a&&b && a.fr===b.fr&&a.fc===b.fc&&a.tr===b.tr&&a.tc===b.tc&&!!a.promo===!!b.promo;
const labelOf = d => d>=0.16?"blunder":d>=0.09?"mistake":d>=0.04?"inaccuracy":"good";

for(const seed of [1300, 7, 9001]){
  const { snapshots, history, over } = sandbox.__playGame(seed);
  const N = snapshots.length - 1;
  ok(N === history.length, `seed ${seed}: snapshots(${N}) == history(${history.length})`);
  ok(snapshots[0].current === "R", `seed ${seed}: node 0 is Red to move`);

  const evals = snapshots.map(evalNode);
  for(let k=0;k<evals.length;k++){
    const e = evals[k], sum = e.reduce((a,b)=>a+b,0);
    ok(Array.isArray(e)&&e.length===4 && Math.abs(sum-1)<1e-6 && e.every(x=>x>=0),
       `seed ${seed} node ${k}: eval normalized (got ${JSON.stringify(e)})`);
  }

  // client-side labels (exactly review.js's algorithm)
  const dist = {};
  for(let k=1;k<=N;k++){
    const prev = snapshots[k-1];
    const moverIdx = ORDER.indexOf(prev.current);
    const played = history[k-1];
    const bestStr = fpc_best_move(JSON.stringify(prev), 0);
    const best = bestStr==="null" ? null : JSON.parse(bestStr);
    let label = "good";
    if(best && !same(best, played)){
      const playedShare = evals[k][moverIdx];
      const bestShare = evalNode(sandbox.__afterMove(prev, best))[moverIdx];
      // best is the Heuristic agent's pick; share is the net's view, so the delta
      // may be negative (heuristic != net) — clamped to 0, exactly like the engine.
      label = labelOf(Math.max(0, bestShare - playedShare));
    }
    dist[label] = (dist[label]||0)+1;
  }
  console.error(`seed ${seed}: ${N} plies, over=${over}, ` +
    `final share R/B/Y/G=${evals[N].map(x=>(x*100|0)).join("/")}, labels=${JSON.stringify(dist)}`);
}

// A random-played game must surface non-"good" labels (proves the label path fires).
{
  const { snapshots, history } = sandbox.__playRandom(55, 120);
  const N = snapshots.length-1;
  const evals = snapshots.map(evalNode);
  const dist = {};
  for(let k=1;k<=N;k++){
    const prev=snapshots[k-1], moverIdx=ORDER.indexOf(prev.current), played=history[k-1];
    const bestStr=fpc_best_move(JSON.stringify(prev),0);
    const best=bestStr==="null"?null:JSON.parse(bestStr);
    let label="good";
    if(best && !same(best,played))
      label=labelOf(Math.max(0, evalNode(sandbox.__afterMove(prev,best))[moverIdx]-evals[k][moverIdx]));
    dist[label]=(dist[label]||0)+1;
  }
  ok((dist.inaccuracy||0)+(dist.mistake||0)+(dist.blunder||0) > 0, `random game surfaces non-good labels (got ${JSON.stringify(dist)})`);
  console.error(`random seed 55: ${N} plies, labels=${JSON.stringify(dist)}`);
}

console.error(fail ? `\n${fail} check(s) FAILED` : "\nall review-pipeline checks passed ✓");
process.exit(fail ? 1 : 0);
