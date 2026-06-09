/* engine.js — ES module bridge to the Rust WASM engine (crates/fpc-wasm -> pkg/).
   Exposes window.Engine for the classic-script UI (ui.js) to call. The engine is
   the same rules core the game uses, so its moves are legal in our state.

   NOTE: WASM init fetches pkg/fpc_wasm_bg.wasm, which requires http(s) — over
   file:// the module won't load and the UI falls back to the built-in bots. */
import init, {
  fpc_best_move, fpc_eval, fpc_legal_moves, fpc_analyze, fpc_attack_map, fpc_new_game
} from "./pkg/fpc_wasm.js";

const Engine = {
  ready: false,
  failed: false,
  bestMove(pos, level){ const r = fpc_best_move(JSON.stringify(pos), level>>>0); return r ? JSON.parse(r) : null; },
  eval(pos){ return JSON.parse(fpc_eval(JSON.stringify(pos))); },
  legalMoves(pos){ return JSON.parse(fpc_legal_moves(JSON.stringify(pos))); },
  analyze(history, level){ return JSON.parse(fpc_analyze(JSON.stringify(history), level>>>0)); },
  attackMap(pos, color){ return fpc_attack_map(JSON.stringify(pos), color); },
  newGamePacket(){ return JSON.parse(fpc_new_game()); },
};
window.Engine = Engine;

init().then(() => {
  Engine.ready = true;
  document.dispatchEvent(new CustomEvent("engine-ready"));
  console.log("fpc engine ready (WASM)");
}).catch(err => {
  Engine.failed = true;
  document.dispatchEvent(new CustomEvent("engine-failed"));
  console.warn("fpc engine failed to load; using built-in bots.", err);
});
