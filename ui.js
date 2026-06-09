/* 4 Player Chess — UI: rendering, input, the in-page bot, and the new-game
   wiring. All rules/state live in rules.js (loaded first). This file owns the
   DOM and reacts to state changes through the onAdvance() hook. You are Red. */
"use strict";

const HUMAN = "R";
const NAME  = { R: "Red", B: "Blue", Y: "Yellow", G: "Green" };

const boardEl = document.getElementById("board");

/* ---------- the rules->UI hook (called by advanceTurn in rules.js) ---------- */
function onAdvance(){
  if(window.__rvSilent) return;   // review.js replays moves on a scratch state; don't render/dispatch bots
  viewPly=-1;                     // a new move/turn snaps the board back to the live position
  render();
  if(G.over){ showGameOver(); return; }
  if(G.current && G.current!==HUMAN) setTimeout(botMove,380);
}

function uiNewGame(){
  hideOverlay();
  newGame();   // rules.js: builds state, advances to Red, fires onAdvance -> render
}

/* ---------- bots: WASM engine when available, built-in heuristic otherwise ---------- */
function engineLevel(){
  const el=document.getElementById("difficulty");
  return el ? (+el.value|0) : 1;
}

function botMove(){
  if(G.over||G.current===HUMAN||!G.current) return;
  const E=window.Engine;
  // If we expect an engine (served over http) but it's still initializing, wait.
  if(window.__expectEngine && (!E || (!E.ready && !E.failed))){ setTimeout(botMove,120); return; }
  if(E && E.ready && !E.failed){
    try{
      const mv=E.bestMove(serializePosition(), engineLevel());
      if(mv){ makeMove(mv); return; }
    }catch(err){ console.warn("engine bestMove failed; using built-in bot.",err); }
  }
  heuristicBotMove();
}

/* in-page bot (1-ply heuristic) — fallback when the WASM engine isn't available */
function heuristicBotMove(){
  if(G.over||G.current===HUMAN||!G.current) return;
  const color=G.current;
  const moves=G.currentLegal;
  let best=null,bestScore=-1e9;
  for(const mv of moves){
    let s=Math.random()*1.5;
    const cap=G.board[mv.tr][mv.tc];
    const capVal=cap?VALUE[cap.type]:0;
    s+=capVal*10;
    if(mv.promo) s+=80;
    // simulate to gauge checks/mates and whether we hang the piece
    const nb=cloneBoard(G.board); applyTo(nb,mv);
    const pieceType = (mv.promo?"Q":G.board[mv.fr][mv.fc].type);
    if(attacked(nb,G.eliminated,mv.tr,mv.tc,color)){
      s-=Math.max(0,VALUE[pieceType]-capVal)*7;   // discourage hanging
    }
    for(const o of ORDER){
      if(o===color||G.eliminated.has(o)) continue;
      if(kingAttacked(nb,G.eliminated,o)){
        s+=3;
        if(legalMoves(nb,G.eliminated,o).length===0) s+=1000; // mate
      }
    }
    if(s>bestScore){ bestScore=s; best=mv; }
  }
  if(best) makeMove(best);
}

/* ---------- rendering ---------- */
function render(){ renderBoard(); renderPanel(); renderStatus(); }

function renderStatus(){
  const el=document.getElementById("status");
  if(G.over){ el.textContent="Game over"; return; }
  if(!G.current){ el.textContent=""; return; }
  if(G.current===HUMAN){
    const chk=kingAttacked(G.board,G.eliminated,HUMAN);
    el.innerHTML="Your move"+(chk?' — <span style="color:#ff6b6b">check!</span>':"");
  }else{
    el.textContent=NAME[G.current]+" is thinking…";
  }
}

function renderPanel(){
  const el=document.getElementById("panel");
  el.innerHTML="";
  for(const c of ORDER){
    const card=document.createElement("div");
    card.className="pcard"+(c===G.current?" active":"")+(G.eliminated.has(c)?" out":"");
    let tag="";
    if(G.eliminated.has(c)) tag="out";
    else if(c===G.current) tag="to move";
    else if(kingAttacked(G.board,G.eliminated,c)) tag="in check";
    card.innerHTML=
      `<span class="swatch" style="background:var(--${NAME[c].toLowerCase()})"></span>`+
      `<span class="nm">${NAME[c]}${c===HUMAN?" (you)":""}</span>`+
      (tag?`<span class="tag">${tag}</span>`:"")+
      `<span class="sc">${G.scores[c]}</span>`;
    el.appendChild(card);
  }
}

let lastAnimated = null;   // key of the move whose slide we've already played

function renderBoard(){
  boardEl.innerHTML="";
  const sel=G.selected;
  const targets=new Map(); // "r,c" -> isCapture
  if(sel){
    for(const mv of G.currentLegal){
      if(mv.fr===sel[0]&&mv.fc===sel[1])
        targets.set(mv.tr+","+mv.tc, !!G.board[mv.tr][mv.tc]);
    }
  }
  const checkColor = (!G.over&&G.current&&kingAttacked(G.board,G.eliminated,G.current))?G.current:null;
  const checkPos = checkColor?findKing(G.board,checkColor):null;

  // Animate only on a fresh move (not on selection/highlight re-renders).
  const mv = G.lastMove;
  const mvKey = mv ? `${mv.fr},${mv.fc}->${mv.tr},${mv.tc}` : null;
  const doSlide = mvKey && mvKey!==lastAnimated;
  lastAnimated = mvKey;
  let destCell = null;

  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const cell=document.createElement("div");
    if(!isPlayable(r,c)){ cell.className="cell blocked"; boardEl.appendChild(cell); continue; }
    let cls="cell "+(((r+c)%2)?"dark":"light");
    if(sel&&sel[0]===r&&sel[1]===c) cls+=" sel";
    if(mv&&((mv.fr===r&&mv.fc===c)||(mv.tr===r&&mv.tc===c))) cls+=" last";
    const key=r+","+c;
    if(targets.has(key)) cls+=" target"+(targets.get(key)?" cap":"");
    if(checkPos&&checkPos[0]===r&&checkPos[1]===c) cls+=" check";
    cell.className=cls;
    const p=G.board[r][c];
    if(p){
      const colorCls=G.eliminated.has(p.color)?"dead":"p-"+p.color;
      cell.insertAdjacentHTML("beforeend",
        `<svg class="piece ${colorCls}" viewBox="0 0 45 45"><use href="#pc-${p.type}"/></svg>`);
    }
    if(doSlide && r===mv.tr && c===mv.tc) destCell = cell;
    cell.addEventListener("click",()=>onClick(r,c));
    boardEl.appendChild(cell);
  }

  if(destCell) slidePiece(destCell, mv);
}

/* Slide the piece now sitting in `cell` so it appears to travel from (fromR,fromC)
   into place over `ms` (FLIP). Used for live moves and, at 2x speed, history scrubbing. */
const SLIDE_MS  = 200;
const REWIND_MS = SLIDE_MS / 2;        // history scrub animates at double speed
function slideInto(cell, fromR, fromC, toR, toC, ms){
  if(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const piece = cell.querySelector(".piece");
  if(!piece) return;
  const size = boardEl.clientWidth/14;
  const dx = (fromC - toC) * size;     // start offset back toward the source square
  const dy = (fromR - toR) * size;
  if(!dx && !dy) return;
  piece.style.transition = "none";
  piece.style.transform = `translate(${dx}px, ${dy}px)`;
  piece.style.zIndex = "5";            // ride above the pieces it passes over
  void piece.getBoundingClientRect();  // force the start frame to commit
  requestAnimationFrame(()=>{
    piece.style.transition = `transform ${ms}ms ease-out`;
    piece.style.transform = "translate(0, 0)";
  });
  piece.addEventListener("transitionend", ()=>{ piece.style.zIndex=""; piece.style.transition=""; }, {once:true});
}
function slidePiece(destCell, mv){ slideInto(destCell, mv.fr, mv.fc, mv.tr, mv.tc, SLIDE_MS); }
function cellAt(r,c){ return boardEl.children[r*14 + c]; }   // cells appended row-major, incl. blocked

function showGameOver(){
  const ranked=ORDER.slice().sort((a,b)=>G.scores[b]-G.scores[a]);
  const top=ranked[0];
  document.getElementById("overlayTitle").textContent =
    (top===HUMAN ? "You win! 🏆" : NAME[top]+" wins");
  document.getElementById("overlayBody").innerHTML = ranked.map((c,i)=>
    `<div class="rankrow"><span class="swatch" style="background:var(--${NAME[c].toLowerCase()})"></span>`+
    `<span>${i+1}. ${NAME[c]}${c===HUMAN?" (you)":""}${G.eliminated.has(c)?" ✗":""}</span>`+
    `<span class="sc">${G.scores[c]}</span></div>`).join("");
  document.getElementById("overlay").classList.remove("hidden");
}
function hideOverlay(){ document.getElementById("overlay").classList.add("hidden"); }

/* ---------- history scrubbing (left/right arrows step through past plies) ----------
   viewPly = -1 means "follow the live game"; otherwise it's an index into G.snapshots
   and the board is shown read-only. Stepping forward onto the latest ply returns to
   live, interactive play. */
let viewPly = -1;
function lastPly(){ return (G.snapshots ? G.snapshots.length : 1) - 1; }
function viewingHistory(){ return viewPly>=0 && viewPly<lastPly(); }

function stepView(delta){
  if(!G.snapshots || G.snapshots.length<2) return;   // nothing to scrub yet
  const last = lastPly();
  const from = (viewPly<0 ? last : viewPly);
  const k = Math.max(0, Math.min(last, from + delta));
  if(k===from) return;                                // already at an end
  if(k===last){                                       // caught up to live -> normal board
    viewPly=-1; render();
    const m=G.history[last-1];                        // replay the latest move into place (2x)
    if(m){ const cell=cellAt(m.tr,m.tc); if(cell) slideInto(cell, m.fr,m.fc, m.tr,m.tc, REWIND_MS); }
    return;
  }
  viewPly = k;
  renderHistoryPly(k, from);
}

function renderHistoryPly(k, from){
  const snap = G.snapshots[k];
  const board = parseBoard(snap.board);
  const elim  = new Set(snap.eliminated.map(s=>s[0]));
  const mv    = k>0 ? G.history[k-1] : null;          // the move that reached this ply
  const cur   = snap.current;                         // who was to move at this ply
  const checkPos = (cur && kingAttacked(board,elim,cur)) ? findKing(board,cur) : null;

  boardEl.innerHTML="";
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const cell=document.createElement("div");
    if(!isPlayable(r,c)){ cell.className="cell blocked"; boardEl.appendChild(cell); continue; }
    let cls="cell "+(((r+c)%2)?"dark":"light");
    if(mv&&((mv.fr===r&&mv.fc===c)||(mv.tr===r&&mv.tc===c))) cls+=" last";
    if(checkPos&&checkPos[0]===r&&checkPos[1]===c) cls+=" check";
    cell.className=cls;
    const p=board[r][c];
    if(p){
      const colorCls=elim.has(p.color)?"dead":"p-"+p.color;
      cell.insertAdjacentHTML("beforeend",
        `<svg class="piece ${colorCls}" viewBox="0 0 45 45"><use href="#pc-${p.type}"/></svg>`);
    }
    cell.addEventListener("click",()=>onClick(r,c));   // onClick is a no-op while viewing history
    boardEl.appendChild(cell);
  }
  // animate the one piece that differs from the previously-shown ply, at 2x speed
  if(from!=null && from!==k){
    if(k>from){                       // stepped forward: replay move that reached ply k
      const m=G.history[k-1];
      if(m){ const cell=cellAt(m.tr,m.tc); if(cell) slideInto(cell, m.fr,m.fc, m.tr,m.tc, REWIND_MS); }
    }else{                            // stepped back: un-move the move that reached ply k+1
      const m=G.history[k];
      if(m){ const cell=cellAt(m.fr,m.fc); if(cell) slideInto(cell, m.tr,m.tc, m.fr,m.fc, REWIND_MS); }
    }
  }
  renderHistoryPanel(snap,elim,cur,board);
  const el=document.getElementById("status");
  const where = k===0 ? "Start position" : `Move ${k} of ${lastPly()}`;
  el.innerHTML = `${where} · history — <b>←</b> prev · <b>→</b> next (→ to return to live)`;
}

function renderHistoryPanel(snap,elim,cur,board){
  const el=document.getElementById("panel");
  el.innerHTML="";
  for(const c of ORDER){
    const card=document.createElement("div");
    card.className="pcard"+(c===cur?" active":"")+(elim.has(c)?" out":"");
    let tag="";
    if(elim.has(c)) tag="out";
    else if(c===cur) tag="to move";
    else if(kingAttacked(board,elim,c)) tag="in check";
    card.innerHTML=
      `<span class="swatch" style="background:var(--${NAME[c].toLowerCase()})"></span>`+
      `<span class="nm">${NAME[c]}${c===HUMAN?" (you)":""}</span>`+
      (tag?`<span class="tag">${tag}</span>`:"")+
      `<span class="sc">${snap.scores[c]}</span>`;
    el.appendChild(card);
  }
}

/* ---------- input ---------- */
function onClick(r,c){
  if(viewingHistory()) return;    // board is read-only while scrubbing past plies
  if(G.over||G.current!==HUMAN) return;
  const b=G.board;
  if(G.selected){
    const [sr,sc]=G.selected;
    const mv=G.currentLegal.find(m=>m.fr===sr&&m.fc===sc&&m.tr===r&&m.tc===c);
    if(mv){ makeMove(mv); return; }
    if(b[r][c]&&b[r][c].color===HUMAN){ G.selected=[r,c]; render(); return; }
    G.selected=null; render(); return;
  }
  if(b[r][c]&&b[r][c].color===HUMAN){ G.selected=[r,c]; render(); }
}

/* ---------- engine status ---------- */
const LEVEL_NAME = { 0:"Hard", 1:"Medium", 2:"Easy", 3:"Beginner", 4:"Expert" };  // 4=ParanoidNet d4 (strongest), 0=heuristic, 3=random
function updateEngineStatus(){
  const el=document.getElementById("engineStatus");
  if(!el) return;
  const E=window.Engine, lvl=LEVEL_NAME[engineLevel()]||"?";
  if(E && E.ready)       el.textContent = `Engine: on · ${lvl} bots`;
  else if(E && E.failed) el.textContent = "Engine: off · built-in bots";
  else if(window.__expectEngine) el.textContent = "Engine: starting…";
  else el.textContent = "Engine: off · built-in bots (serve over http for AI)";
}

/* ---------- wire up ---------- */
document.getElementById("newGame").addEventListener("click",uiNewGame);
document.getElementById("overlayNew").addEventListener("click",uiNewGame);
document.getElementById("difficulty").addEventListener("change",updateEngineStatus);
document.addEventListener("engine-ready",updateEngineStatus);
document.addEventListener("engine-failed",updateEngineStatus);
// left/right arrows scrub the main board through game history
document.addEventListener("keydown", e=>{
  if(e.key!=="ArrowLeft" && e.key!=="ArrowRight") return;
  // while an overlay is open, leave the keys to it (review has its own scrubber)
  if(!document.getElementById("reviewOverlay").classList.contains("hidden")) return;
  if(!document.getElementById("overlay").classList.contains("hidden")) return;
  if(!G.snapshots || G.snapshots.length<2) return;
  e.preventDefault();
  stepView(e.key==="ArrowRight" ? 1 : -1);
});
updateEngineStatus();
uiNewGame();
