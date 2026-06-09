/* 4 Player Chess — RULES + STATE (no DOM, no rendering).
   This is the canonical game logic. The Rust engine (fpc-core) is validated
   move-for-move against this file via tools/oracle.mjs.

   UI side effects are decoupled: advanceTurn()/makeMove() are pure state
   transitions and call the optional global hook `onAdvance()` (defined in
   ui.js) when the turn resolves. In a headless context (the oracle) the hook
   is simply absent, so this file touches no DOM at all. */
"use strict";

/* ---------- constants ---------- */
const ORDER = ["R", "B", "Y", "G"];
const VALUE = { P: 1, N: 3, B: 5, R: 5, Q: 9, K: 20 };

const ORTH = [[-1,0],[1,0],[0,-1],[0,1]];
const DIAG = [[-1,-1],[-1,1],[1,-1],[1,1]];
const ALL8 = ORTH.concat(DIAG);
const KNIGHT = [[-2,-1],[-2,1],[2,-1],[2,1],[-1,-2],[1,-2],[-1,2],[1,2]];

// pawn capture offsets (forward diagonals) per colour
const PAWN_CAPS = {
  R: [[-1,-1],[-1,1]],
  Y: [[ 1,-1],[ 1,1]],
  B: [[-1, 1],[ 1,1]],
  G: [[-1,-1],[ 1,-1]],
};
const PAWN_FWD = { R:[-1,0], Y:[1,0], B:[0,1], G:[0,-1] };

function isPlayable(r,c){
  if (r<0||r>13||c<0||c>13) return false;
  return !((r<3||r>10) && (c<3||c>10)); // four 3x3 corners removed
}
function pawnHome(color,r,c){
  return (color==="R"&&r===12)||(color==="Y"&&r===1)||
         (color==="B"&&c===1)||(color==="G"&&c===12);
}
function pawnPromo(color,r,c){
  // chess.com rule: promote on the 8th rank — the first square past the centre
  // line (board halves split rows/cols 0–6 | 7–13), not at the far edge.
  return (color==="R"&&r===6)||(color==="Y"&&r===7)||
         (color==="B"&&c===7)||(color==="G"&&c===6);
}

/* ---------- game state ---------- */
let G = null;

function newBoard(){
  const b = Array.from({length:14}, ()=>Array(14).fill(null));
  // chess.com "Modern" FFA: every player has Queen on their own left, King on
  // their own right (relative to facing the centre). Index order is along each
  // player's back rank as written below; Blue/Green are indexed top->bottom.
  const RED    = ["R","N","B","Q","K","B","N","R"];  // bottom, faces up:    Q@col6  K@col7
  const YELLOW = ["R","N","B","K","Q","B","N","R"];  // top, faces down:     K@col6  Q@col7
  const BLUE   = ["R","N","B","Q","K","B","N","R"];  // left, faces right:   Q@row6  K@row7
  const GREEN  = ["R","N","B","K","Q","B","N","R"];  // right, faces left:   K@row6  Q@row7
  for (let i=0;i<8;i++){
    const col = 3+i, row = 3+i;
    b[13][col] = {color:"R", type:RED[i]};
    b[12][col] = {color:"R", type:"P"};
    b[0][col]  = {color:"Y", type:YELLOW[i]};
    b[1][col]  = {color:"Y", type:"P"};
    b[row][0]  = {color:"B", type:BLUE[i]};
    b[row][1]  = {color:"B", type:"P"};
    b[row][13] = {color:"G", type:GREEN[i]};
    b[row][12] = {color:"G", type:"P"};
  }
  return b;
}

function newGame(){
  G = {
    board: newBoard(),
    eliminated: new Set(),
    scores: { R:0, B:0, Y:0, G:0 },
    idx: 3,            // so first advance lands on Red
    current: null,
    currentLegal: [],
    lastMover: null,
    lastMove: null,
    selected: null,
    over: false,
    history: [],       // [{fr,fc,tr,tc,promo}] from the start position
    noProgress: 0,     // plies since last capture or pawn move (draw rule)
    repeats: {},       // position-key -> times seen (threefold rule)
    snapshots: [],     // position packet after each ply (snapshots[k]=after k moves); drives Review
  };
  advanceTurn();
  G.snapshots.push(serializePosition());   // node 0: the start position
}

/* ---------- board helpers ---------- */
function cloneBoard(b){ return b.map(row=>row.map(c=> c?{color:c.color,type:c.type}:null )); }

function findKing(b,color){
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const p=b[r][c];
    if(p&&p.color===color&&p.type==="K") return [r,c];
  }
  return null;
}

// can piece p at (pr,pc) attack square (tr,tc)?  (ignores turn/elimination)
function pieceAttacks(b,p,pr,pc,tr,tc){
  const dr=tr-pr, dc=tc-pc;
  if(dr===0&&dc===0) return false;
  switch(p.type){
    case "K": return Math.max(Math.abs(dr),Math.abs(dc))===1;
    case "N": return (Math.abs(dr)===1&&Math.abs(dc)===2)||(Math.abs(dr)===2&&Math.abs(dc)===1);
    case "P": return PAWN_CAPS[p.color].some(o=>o[0]===dr&&o[1]===dc);
    case "B": if(Math.abs(dr)!==Math.abs(dc)) return false; return clearPath(b,pr,pc,tr,tc);
    case "R": if(dr!==0&&dc!==0) return false; return clearPath(b,pr,pc,tr,tc);
    case "Q":
      if(!(dr===0||dc===0||Math.abs(dr)===Math.abs(dc))) return false;
      return clearPath(b,pr,pc,tr,tc);
  }
  return false;
}
function clearPath(b,pr,pc,tr,tc){
  const sr=Math.sign(tr-pr), sc=Math.sign(tc-pc);
  let r=pr+sr, c=pc+sc;
  while(r!==tr||c!==tc){
    if(!isPlayable(r,c)) return false;
    if(b[r][c]) return false;
    r+=sr; c+=sc;
  }
  return true;
}

// is (tr,tc) attacked by any ACTIVE piece not of defColor?
function attacked(b,elim,tr,tc,defColor){
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const p=b[r][c];
    if(!p||p.color===defColor||elim.has(p.color)) continue;
    if(pieceAttacks(b,p,r,c,tr,tc)) return true;
  }
  return false;
}
function kingAttacked(b,elim,color){
  const k=findKing(b,color);
  if(!k) return true;
  return attacked(b,elim,k[0],k[1],color);
}
// active colours whose piece attacks color's king
function checkers(b,elim,color){
  const k=findKing(b,color); if(!k) return [];
  const out=new Set();
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const p=b[r][c];
    if(!p||p.color===color||elim.has(p.color)) continue;
    if(pieceAttacks(b,p,r,c,k[0],k[1])) out.add(p.color);
  }
  return [...out];
}

/* ---------- move generation ---------- */
function addSlide(b,elim,color,fr,fc,dirs,out){
  for(const [dr,dc] of dirs){
    let r=fr+dr,c=fc+dc;
    while(isPlayable(r,c)){
      const occ=b[r][c];
      if(!occ){ out.push({fr,fc,tr:r,tc:c}); }
      else{
        if(occ.color!==color && !(occ.type==="K" && !elim.has(occ.color)))
          out.push({fr,fc,tr:r,tc:c});
        break;
      }
      r+=dr;c+=dc;
    }
  }
}
function canLand(b,elim,color,r,c){            // for step pieces
  if(!isPlayable(r,c)) return false;
  const occ=b[r][c];
  if(!occ) return true;
  if(occ.color===color) return false;
  if(occ.type==="K" && !elim.has(occ.color)) return false; // can't capture live king
  return true;
}
function pseudoMoves(b,elim,color){
  const out=[];
  for(let fr=0;fr<14;fr++)for(let fc=0;fc<14;fc++){
    const p=b[fr][fc];
    if(!p||p.color!==color) continue;
    switch(p.type){
      case "P":{
        const [fdr,fdc]=PAWN_FWD[color];
        const r1=fr+fdr,c1=fc+fdc;
        if(isPlayable(r1,c1)&&!b[r1][c1]){
          out.push({fr,fc,tr:r1,tc:c1,promo:pawnPromo(color,r1,c1)});
          if(pawnHome(color,fr,fc)){
            const r2=fr+2*fdr,c2=fc+2*fdc;
            if(isPlayable(r2,c2)&&!b[r2][c2]) out.push({fr,fc,tr:r2,tc:c2});
          }
        }
        for(const [cdr,cdc] of PAWN_CAPS[color]){
          const r=fr+cdr,c=fc+cdc;
          if(!isPlayable(r,c)) continue;
          const occ=b[r][c];
          if(occ&&occ.color!==color&&!(occ.type==="K"&&!elim.has(occ.color)))
            out.push({fr,fc,tr:r,tc:c,promo:pawnPromo(color,r,c)});
        }
        break;
      }
      case "N": for(const [dr,dc] of KNIGHT){ const r=fr+dr,c=fc+dc; if(canLand(b,elim,color,r,c)) out.push({fr,fc,tr:r,tc:c}); } break;
      case "K": for(const [dr,dc] of ALL8){ const r=fr+dr,c=fc+dc; if(canLand(b,elim,color,r,c)) out.push({fr,fc,tr:r,tc:c}); } break;
      case "B": addSlide(b,elim,color,fr,fc,DIAG,out); break;
      case "R": addSlide(b,elim,color,fr,fc,ORTH,out); break;
      case "Q": addSlide(b,elim,color,fr,fc,ALL8,out); break;
    }
  }
  return out;
}
function applyTo(b,mv){
  const p=b[mv.fr][mv.fc];
  b[mv.fr][mv.fc]=null;
  if(p.type==="P"&&pawnPromo(p.color,mv.tr,mv.tc)) b[mv.tr][mv.tc]={color:p.color,type:"Q"};
  else b[mv.tr][mv.tc]=p;
}
function legalMoves(b,elim,color){
  const out=[];
  for(const mv of pseudoMoves(b,elim,color)){
    const nb=cloneBoard(b);
    applyTo(nb,mv);
    if(!kingAttacked(nb,elim,color)) out.push(mv);
  }
  return out;
}

/* ---------- turn flow (pure; UI reacts via the onAdvance hook) ---------- */
function activeCount(){ return ORDER.filter(c=>!G.eliminated.has(c)).length; }

function makeMove(mv){
  const b=G.board;
  const p=b[mv.fr][mv.fc];
  const cap=b[mv.tr][mv.tc];
  // dead pieces (owner already eliminated) are worth 0; live captures score material
  if(cap && !G.eliminated.has(cap.color)) G.scores[p.color]+=VALUE[cap.type];
  // draw clock: reset on any capture or any pawn move (incl. promotion), else +1
  G.noProgress = (cap || p.type==="P") ? 0 : G.noProgress+1;
  applyTo(b,mv);
  G.lastMover=p.color;
  G.lastMove={fr:mv.fr,fc:mv.fc,tr:mv.tr,tc:mv.tc};
  G.history.push({fr:mv.fr,fc:mv.fc,tr:mv.tr,tc:mv.tc,promo:!!mv.promo});
  G.selected=null;
  advanceTurn();
  if(G.snapshots) G.snapshots.push(serializePosition());   // node k: position after this move
}

function advanceTurn(){
  while(true){
    if(activeCount()<=1){
      G.current=null;
      G.over=true;
      if(typeof onAdvance==="function") onAdvance();
      return;
    }
    G.idx=(G.idx+1)%4;
    const c=ORDER[G.idx];
    if(G.eliminated.has(c)) continue;
    const legal=legalMoves(G.board,G.eliminated,c);
    if(legal.length===0){
      const chk=checkers(G.board,G.eliminated,c);
      G.eliminated.add(c);
      if(chk.length){                       // checkmate: credit a checker
        G.scores[chk[0]]+=20;
      }else if(G.lastMover&&G.lastMover!==c&&!G.eliminated.has(G.lastMover)){
        G.scores[G.lastMover]+=20;          // stalemate: credit the stalemater
      }
      continue;
    }
    G.current=c;
    G.currentLegal=legal;
    if(isDraw(c)){            // FFA: a draw just ends the game; scores stand
      G.current=null;
      G.over=true;
    }
    if(typeof onAdvance==="function") onAdvance();
    return;
  }
}

/* ---------- draw detection (game-ending; final ranking by score) ----------
   Spec locked with the engine (chat.md). Each condition ends the game.
   Called once per settled turn for player `cur`. */
const DRAW_NO_PROGRESS = 100;   // plies without a capture or pawn move

function onlyKingsLeft(){       // insufficient material among ALL active players
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const p=G.board[r][c];
    if(p && !G.eliminated.has(p.color) && p.type!=="K") return false;
  }
  return true;
}
function isDraw(cur){
  if(G.noProgress>=DRAW_NO_PROGRESS) return true;
  if(onlyKingsLeft()) return true;
  // threefold: identical (board, side-to-move, eliminated) seen 3x
  const key=serializeBoard(G.board)+"|"+cur+"|"+[...G.eliminated].sort().join("");
  const n=(G.repeats[key]||0)+1;
  G.repeats[key]=n;
  return n>=3;
}

/* ---------- serialization (the cross-boundary "position packet") ----------
   Format locked with the engine (see chat.md). The 196-char board string is
   row-major over all 14x14 cells: "RP" piece, ".." empty, "##" blocked —
   identical to tools/oracle.mjs, which the Rust port parses the same way. */
function serializeBoard(b){
  let s="";
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    if(!isPlayable(r,c)){ s+="##"; continue; }
    const p=b[r][c];
    s+= p ? (p.color+p.type) : "..";
  }
  return s;
}
function parseBoard(str){
  const b=Array.from({length:14},()=>Array(14).fill(null));
  let i=0;
  for(let r=0;r<14;r++)for(let c=0;c<14;c++){
    const tok=str.slice(i,i+2); i+=2;
    if(tok==="##"||tok==="..") continue;
    b[r][c]={color:tok[0], type:tok[1]};
  }
  return b;
}
// Full position packet sent to the engine for legal_moves / best_move / eval.
function serializePosition(st){
  st = st || G;
  return {
    board: serializeBoard(st.board),
    eliminated: [...st.eliminated],
    scores: { R:st.scores.R, B:st.scores.B, Y:st.scores.Y, G:st.scores.G },
    current: st.current,
  };
}
// Move list replayed from newGame() — drives analyze()/review.
function serializeHistory(st){
  st = st || G;
  return st.history.map(m=>({fr:m.fr,fc:m.fc,tr:m.tr,tc:m.tc,promo:!!m.promo}));
}
