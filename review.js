/* 4 Player Chess — REVIEW: post-game (or anytime) analysis.
   Uses the WASM engine to draw a 4-line win-probability chart over the game,
   label every move (good/inaccuracy/mistake/blunder), and scrub the board
   ply-by-ply. Reads G.snapshots (one position packet per ply, captured in
   rules.js) so board reconstruction is exact — no turn-order re-derivation.

   Requires the engine (served over http). Falls back to a message otherwise. */
"use strict";

const REVIEW_LEVEL = 0;           // analyze with the strongest agent (Heuristic), regardless of bot difficulty
const RV_COLORS = { R:"--red", B:"--blue", Y:"--yellow", G:"--green" };

let rvData = null;                // { evals:[[R,B,Y,G]...], moves:[{label,best,played}|null...] }
let rvPly  = 0;                   // currently shown node (0..N)

/* coordinate like "g4": file a..n (col 0..13), rank 1..14 from the bottom (row 13..0) */
function rvCoord(r,c){ return String.fromCharCode(97+c) + (14-r); }
function rvMoveStr(m){ return m ? rvCoord(m.fr,m.fc)+"–"+rvCoord(m.tr,m.tc)+(m.promo?"=Q":"") : ""; }

/* ---------- open / close ---------- */
function openReview(){
  const ov = document.getElementById("reviewOverlay");
  ov.classList.remove("hidden");
  const E = window.Engine;
  if(!G.snapshots || G.snapshots.length<2){
    rvMessage("Play a few moves first, then come back to review the game.");
    return;
  }
  if(!(E && E.ready)){
    rvMessage(E && E.failed
      ? "Review needs the engine. Serve the folder over http (not file://) to enable it."
      : "Engine still loading… try again in a moment.");
    return;
  }
  rvMessage("Analyzing game…");
  setTimeout(runReview, 30);      // let the overlay paint before the (sync) crunch
}
function closeReview(){ document.getElementById("reviewOverlay").classList.add("hidden"); }

function rvMessage(msg){
  document.getElementById("rvBody").classList.add("hidden");
  const m = document.getElementById("rvMsg");
  m.classList.remove("hidden");
  m.textContent = msg;
}

/* eval a node's win-prob. A terminal snapshot (game over) has current:null, which
   the engine's position packet can't express — its share is just the final scores
   (mirrors fpc-core score_shares: normalized, or even split if no points yet). */
function rvEvalNode(snap){
  if(snap.current){ return window.Engine.eval(snap); }
  const s=snap.scores, tot=s.R+s.B+s.Y+s.G;
  if(tot<=0) return [0.25,0.25,0.25,0.25];
  return [s.R/tot, s.B/tot, s.Y/tot, s.G/tot];
}

/* Build a scratch game state from a position packet so rules.js can apply a
   hypothetical move with full semantics (captures, promotion, turn advance). */
function rvStateFromSnap(s){
  return {
    board: parseBoard(s.board),
    eliminated: new Set(s.eliminated.map(x=>x[0])),
    scores: { R:s.scores.R, B:s.scores.B, Y:s.scores.Y, G:s.scores.G },
    idx: ORDER.indexOf(s.current),   // advanceTurn() will step to the next player
    current: s.current,
    currentLegal: [],
    lastMover: null, lastMove: null, selected: null, over: false,
    history: [], noProgress: 0, repeats: {},
    snapshots: null,                 // makeMove's snapshot push is guarded on this
  };
}

/* The mover's predicted score-share after playing `mv` from position `snap`.
   Applies the move on a scratch state (live G is swapped out and restored). */
function rvShareAfter(snap, mv, moverIdx){
  const saved = G;
  window.__rvSilent = true;
  try{
    G = rvStateFromSnap(snap);
    makeMove(mv);                     // mutates scratch G, advances the turn
    return rvEvalNode(serializePosition())[moverIdx];
  } finally {
    G = saved;
    window.__rvSilent = false;
  }
}

function rvLabel(delta){
  if(delta >= 0.16) return "blunder";
  if(delta >= 0.09) return "mistake";
  if(delta >= 0.04) return "inaccuracy";
  return "good";
}
const rvSameMove = (a,b)=> a&&b && a.fr===b.fr&&a.fc===b.fc&&a.tr===b.tr&&a.tc===b.tc&&!!a.promo===!!b.promo;

/* ---------- analysis ---------- */
function runReview(){
  const E = window.Engine;
  const snaps = G.snapshots;            // length N+1
  const N = snaps.length-1;
  const hist = serializeHistory();
  let evals, moves;
  try{
    evals = snaps.map(rvEvalNode);      // [R,B,Y,G] per node — uses the real board, start-agnostic
    // moves[k] (k=1..N) describes the move that REACHED node k (played hist[k-1]).
    // Compare it to the engine's best move from the prior position; label by the
    // mover's predicted-share loss. All evaluated on the actual snapshots, so this
    // is correct for any starting setup (Modern/Classic/puzzles).
    moves = [null];
    for(let k=1;k<=N;k++){
      const prev = snaps[k-1];
      const moverIdx = ORDER.indexOf(prev.current);
      const played = hist[k-1];
      const best = E.bestMove(prev, REVIEW_LEVEL);
      let label = "good";
      if(best && !rvSameMove(best, played)){
        const playedShare = evals[k][moverIdx];                 // = eval of snaps[k]
        const bestShare   = rvShareAfter(prev, best, moverIdx);
        label = rvLabel(Math.max(0, bestShare - playedShare));
      }
      moves.push({ played, best: best||null, label });
    }
  }catch(err){
    console.warn("review analysis failed", err);
    rvMessage("Analysis failed — see console.");
    return;
  }
  rvData = { evals, moves, N };
  rvPly = N;                            // start at the final position
  document.getElementById("rvMsg").classList.add("hidden");
  document.getElementById("rvBody").classList.remove("hidden");
  const sl=document.getElementById("rvSlider"); sl.min=0; sl.max=N;
  drawChart();
  rvSetPly(N);
}

/* ---------- chart ---------- */
const CW=320, CH=150, PADL=4, PADR=4, PADT=8, PADB=4;
function rvX(k){ const N=rvData.N||1; return PADL + (k/(N||1))*(CW-PADL-PADR); }
function rvY(v){ return PADT + (1-v)*(CH-PADT-PADB); }

function drawChart(){
  const { evals, N } = rvData;
  let s = `<svg viewBox="0 0 ${CW} ${CH}" id="rvSvg" preserveAspectRatio="none">`;
  // faint gridlines at 25/50/75%
  for(const g of [0.25,0.5,0.75]){
    const y=rvY(g).toFixed(1);
    s += `<line x1="${PADL}" y1="${y}" x2="${CW-PADR}" y2="${y}" class="rv-grid"/>`;
  }
  // one polyline per player
  for(const col of ORDER){
    const ci = ORDER.indexOf(col);
    const pts = evals.map((e,k)=> `${rvX(k).toFixed(1)},${rvY(e[ci]).toFixed(1)}`).join(" ");
    s += `<polyline points="${pts}" fill="none" stroke="var(${RV_COLORS[col]})" stroke-width="2" stroke-linejoin="round"/>`;
  }
  // cursor
  s += `<line id="rvCursor" x1="0" y1="${PADT}" x2="0" y2="${CH-PADB}" class="rv-cursor"/>`;
  for(const col of ORDER){
    s += `<circle id="rvDot-${col}" r="3" fill="var(${RV_COLORS[col]})" stroke="#222" stroke-width="1"/>`;
  }
  s += `</svg>`;
  const host = document.getElementById("rvChart");
  host.innerHTML = s;
  // scrub by tapping/dragging the chart
  const svg = document.getElementById("rvSvg");
  const pick = ev => {
    const rect = svg.getBoundingClientRect();
    const px = ((ev.touches?ev.touches[0].clientX:ev.clientX) - rect.left)/rect.width*CW;
    const k = Math.round((px-PADL)/((CW-PADL-PADR)||1)*(N||1));
    rvSetPly(Math.max(0,Math.min(N,k)));
  };
  svg.addEventListener("pointerdown", pick);
  svg.addEventListener("pointermove", ev=>{ if(ev.buttons) pick(ev); });
}

function updateCursor(){
  const x = rvX(rvPly).toFixed(1);
  const cur = document.getElementById("rvCursor");
  if(cur){ cur.setAttribute("x1",x); cur.setAttribute("x2",x); }
  const e = rvData.evals[rvPly];
  for(const col of ORDER){
    const dot=document.getElementById("rvDot-"+col);
    if(dot){ dot.setAttribute("cx",x); dot.setAttribute("cy",rvY(e[ORDER.indexOf(col)]).toFixed(1)); }
  }
}

/* ---------- ply navigation ---------- */
function rvSetPly(k){
  rvPly = Math.max(0, Math.min(rvData.N, k|0));
  document.getElementById("rvSlider").value = rvPly;
  updateCursor();
  rvRenderBoard();
  rvRenderInfo();
}

/* ---------- mini board at the selected ply ---------- */
function rvRenderBoard(){
  const snap = G.snapshots[rvPly];
  const board = parseBoard(snap.board);
  const elim = new Set(snap.eliminated.map(s=>s[0]));
  const mv = rvPly>0 ? rvData.moves[rvPly].played : null;  // move that reached here
  const host = document.getElementById("rvBoard");
  host.innerHTML="";
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const cell=document.createElement("div");
    if(!isPlayable(r,c)){ cell.className="rvcell blocked"; host.appendChild(cell); continue; }
    let cls="rvcell "+(((r+c)%2)?"dark":"light");
    if(mv && ((mv.fr===r&&mv.fc===c)||(mv.tr===r&&mv.tc===c))) cls+=" last";
    cell.className=cls;
    const p=board[r][c];
    if(p){
      const colorCls = elim.has(p.color) ? "dead" : "p-"+p.color;
      cell.insertAdjacentHTML("beforeend",
        `<svg class="piece ${colorCls}" viewBox="0 0 45 45"><use href="#pc-${p.type}"/></svg>`);
    }
    host.appendChild(cell);
  }
}

/* ---------- info / labels ---------- */
function rvRenderInfo(){
  const N=rvData.N, e=rvData.evals[rvPly];
  // win-prob legend
  let legend="";
  for(const col of ORDER){
    const pct=Math.round(e[ORDER.indexOf(col)]*100);
    legend += `<span class="rv-chip"><span class="swatch" style="background:var(${RV_COLORS[col]})"></span>${pct}%</span>`;
  }
  document.getElementById("rvLegend").innerHTML = legend;

  const head=document.getElementById("rvMoveHead");
  const detail=document.getElementById("rvMoveDetail");
  if(rvPly===0){
    head.innerHTML = `<b>Starting position</b>`;
    detail.innerHTML = "";
    return;
  }
  const m = rvData.moves[rvPly];
  const mover = G.snapshots[rvPly-1].current;     // who was to move at the prior node
  head.innerHTML =
    `<span class="swatch" style="background:var(${RV_COLORS[mover]})"></span>`+
    `<b>${NAME[mover]}</b> ${rvMoveStr(m.played)} `+
    `<span class="rv-badge ${m.label}">${m.label}</span>`+
    `<span class="rv-ply">${rvPly}/${N}</span>`;
  let d="";
  if(m.label!=="good" && m.best &&
     !(m.best.fr===m.played.fr&&m.best.fc===m.played.fc&&m.best.tr===m.played.tr&&m.best.tc===m.played.tc)){
    d = `Best was <b>${rvMoveStr(m.best)}</b>`;
  }
  detail.innerHTML = d;
}

/* ---------- wire up ---------- */
document.getElementById("reviewBtn").addEventListener("click", openReview);
document.getElementById("rvClose").addEventListener("click", closeReview);
document.getElementById("rvFirst").addEventListener("click", ()=>rvSetPly(0));
document.getElementById("rvPrev").addEventListener("click", ()=>rvSetPly(rvPly-1));
document.getElementById("rvNext").addEventListener("click", ()=>rvSetPly(rvPly+1));
document.getElementById("rvLast").addEventListener("click", ()=>rvSetPly(rvData.N));
document.getElementById("rvSlider").addEventListener("input", e=>rvSetPly(+e.target.value));
{
  const ob=document.getElementById("overlayReview");
  if(ob) ob.addEventListener("click", ()=>{ hideOverlay(); openReview(); });
}
