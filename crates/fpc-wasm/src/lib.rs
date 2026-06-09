//! fpc-wasm: the browser-facing engine API (wasm-bindgen). All boundaries use
//! the agreed JSON position packet (see chat.md contract). The trained value net
//! is embedded via include_bytes!, so there is no runtime fetch.
//!
//! Exports:
//!   fpc_legal_moves(pos_json) -> Move[]
//!   fpc_best_move(pos_json, level) -> Move | null
//!   fpc_eval(pos_json) -> [f64;4]                 (normalized, R,B,Y,G)
//!   fpc_analyze(history_json, level) -> [{eval,best,label}]  per ply
//!   fpc_attack_map(pos_json, color) -> 196-char "0"/"1"
//!
//! Difficulty levels (engine agents, add-only numbering):
//!   0=Heuristic, 1=Search(2), 2=Net, 3=Random.
//! The UI labels these by *measured* strength, not by number.

use std::sync::{Arc, OnceLock};

use fpc_agents::*;
use fpc_core::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const MODEL_BYTES: &[u8] = include_bytes!("../../../data/model.bin");

fn net() -> Arc<Net> {
    static NET: OnceLock<Arc<Net>> = OnceLock::new();
    NET.get_or_init(|| Arc::new(Net::from_bytes(MODEL_BYTES))).clone()
}

/* ---------- JSON contract types ---------- */

#[derive(Deserialize)]
struct ScoresJson {
    #[serde(rename = "R")]
    r: i32,
    #[serde(rename = "B")]
    b: i32,
    #[serde(rename = "Y")]
    y: i32,
    #[serde(rename = "G")]
    g: i32,
}

#[derive(Deserialize)]
struct PosJson {
    board: String,
    eliminated: Vec<String>,
    scores: ScoresJson,
    current: String,
}

#[derive(Deserialize, Serialize, Clone, Copy)]
struct MoveJson {
    fr: i32,
    fc: i32,
    tr: i32,
    tc: i32,
    #[serde(default)]
    promo: bool,
}

impl From<Move> for MoveJson {
    fn from(m: Move) -> Self {
        MoveJson { fr: m.fr, fc: m.fc, tr: m.tr, tc: m.tc, promo: m.promo }
    }
}
impl From<MoveJson> for Move {
    fn from(m: MoveJson) -> Self {
        Move { fr: m.fr, fc: m.fc, tr: m.tr, tc: m.tc, promo: m.promo }
    }
}

#[derive(Serialize, Deserialize)]
struct AnalyzePly {
    eval: Vec<f64>,
    best: Option<MoveJson>,
    label: String,
}

#[derive(Serialize)]
struct ErrJson {
    error: String,
}

fn err(msg: &str) -> String {
    serde_json::to_string(&ErrJson { error: msg.into() }).unwrap()
}

/* ---------- packet -> State ---------- */

fn elim_arr(colors: &[String]) -> [bool; 4] {
    let mut e = [false; 4];
    for s in colors {
        if let Some(ch) = s.chars().next() {
            e[Color::from_char(ch).idx()] = true;
        }
    }
    e
}

fn state_from_pos(p: &PosJson) -> Result<State, String> {
    let board = board_from_str(&p.board).ok_or("bad board string")?;
    let elim = elim_arr(&p.eliminated);
    let scores = [p.scores.r, p.scores.b, p.scores.y, p.scores.g];
    let current = Color::from_char(p.current.chars().next().ok_or("bad current")?);
    Ok(State::from_position(board, elim, scores, current))
}

fn parse_pos(pos_json: &str) -> Result<State, String> {
    let p: PosJson = serde_json::from_str(pos_json).map_err(|e| e.to_string())?;
    state_from_pos(&p)
}

/* ---------- eval ---------- */

/// Normalize a 4-vector to a probability split (clamp >=0, scale to sum 1).
fn normalize(v: [f32; 4]) -> [f64; 4] {
    let mut o = [0.0f64; 4];
    let mut sum = 0.0f64;
    for i in 0..4 {
        o[i] = v[i].max(0.0) as f64;
        sum += o[i];
    }
    if sum <= 0.0 {
        return [0.25; 4];
    }
    for i in 0..4 {
        o[i] /= sum;
    }
    o
}

/// Predicted final score-share for a position (true shares if terminal).
fn eval_state(st: &State) -> [f64; 4] {
    if st.over || st.current.is_none() {
        let s = score_shares(&st.scores);
        return [s[0] as f64, s[1] as f64, s[2] as f64, s[3] as f64];
    }
    normalize(net().forward(&features(st)))
}

/* ---------- agents by difficulty level ---------- */

fn agent_for(level: u32) -> Box<dyn Agent> {
    let kind = match level {
        0 => AgentKind::Heuristic,
        1 => AgentKind::Search(2),
        3 => AgentKind::Random,
        _ => AgentKind::Net { net: net(), label: "net".into() }, // 2 = Net (default)
    };
    kind.build(0xA9)
}

/* ---------- exports ---------- */

/// The initial-position packet (so the UI and engine agree on the start).
#[wasm_bindgen]
pub fn fpc_new_game() -> String {
    let st = State::new_game();
    format!(
        r#"{{"board":"{}","eliminated":[],"scores":{{"R":0,"B":0,"Y":0,"G":0}},"current":"{}"}}"#,
        board_to_str(&st.board),
        st.current.unwrap().to_char()
    )
}

#[wasm_bindgen]
pub fn fpc_legal_moves(pos_json: &str) -> String {
    let st = match parse_pos(pos_json) {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let moves: Vec<MoveJson> = st.current_legal.iter().map(|&m| m.into()).collect();
    serde_json::to_string(&moves).unwrap()
}

#[wasm_bindgen]
pub fn fpc_best_move(pos_json: &str, level: u32) -> String {
    let st = match parse_pos(pos_json) {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    if st.current_legal.is_empty() {
        return "null".into();
    }
    let mut agent = agent_for(level);
    let mv: MoveJson = agent.select(&st).into();
    serde_json::to_string(&mv).unwrap()
}

#[wasm_bindgen]
pub fn fpc_eval(pos_json: &str) -> String {
    let st = match parse_pos(pos_json) {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    serde_json::to_string(&eval_state(&st)).unwrap()
}

#[wasm_bindgen]
pub fn fpc_analyze(history_json: &str, level: u32) -> String {
    let history: Vec<MoveJson> = match serde_json::from_str(history_json) {
        Ok(h) => h,
        Err(e) => return err(&e.to_string()),
    };
    let mut agent = agent_for(level);
    let mut st = State::new_game();
    let mut out: Vec<AnalyzePly> = Vec::new();

    for played in history {
        if st.over || st.current.is_none() {
            break;
        }
        let mover = st.current.unwrap().idx();
        let ev = eval_state(&st);

        // best move + label (own predicted-share drop, played vs best)
        let (best, label) = if st.current_legal.is_empty() {
            (None, "good".to_string())
        } else {
            let best_mv = agent.select(&st);
            let share_after = |mv: Move| -> f64 {
                let mut ns = st.clone();
                ns.make_move(mv);
                eval_state(&ns)[mover]
            };
            let delta = (share_after(best_mv) - share_after(played.into())).max(0.0);
            let label = if delta >= 0.16 {
                "blunder"
            } else if delta >= 0.09 {
                "mistake"
            } else if delta >= 0.04 {
                "inaccuracy"
            } else {
                "good"
            };
            (Some(best_mv.into()), label.to_string())
        };

        out.push(AnalyzePly { eval: ev.to_vec(), best, label });
        st.make_move(played.into());
    }

    serde_json::to_string(&out).unwrap()
}

#[wasm_bindgen]
pub fn fpc_attack_map(pos_json: &str, color: &str) -> String {
    let st = match parse_pos(pos_json) {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let col = match color.chars().next() {
        Some(ch) => Color::from_char(ch),
        None => return err("bad color"),
    };
    let mut s = String::with_capacity(14 * 14);
    let inactive = st.eliminated[col.idx()];
    for r in 0..14i32 {
        for c in 0..14i32 {
            let mut hit = false;
            if is_playable(r, c) && !inactive {
                // attacked by any of `col`'s pieces?
                'scan: for pr in 0..14i32 {
                    for pc in 0..14i32 {
                        if let Some(p) = st.board[pr as usize][pc as usize] {
                            if p.color == col && piece_attacks(&st.board, p, pr, pc, r, c) {
                                hit = true;
                                break 'scan;
                            }
                        }
                    }
                }
            }
            s.push(if hit { '1' } else { '0' });
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_packet() -> String {
        let st = State::new_game();
        let scores = st.scores;
        format!(
            r#"{{"board":"{}","eliminated":[],"scores":{{"R":{},"B":{},"Y":{},"G":{}}},"current":"R"}}"#,
            board_to_str(&st.board),
            scores[0],
            scores[1],
            scores[2],
            scores[3]
        )
    }

    #[test]
    fn legal_moves_at_start() {
        let out = fpc_legal_moves(&start_packet());
        let moves: Vec<MoveJson> = serde_json::from_str(&out).unwrap();
        // Red opens with the same count game.js produces.
        assert!(!moves.is_empty(), "expected legal moves, got: {out}");
        assert!(out.starts_with('['));
    }

    #[test]
    fn eval_is_normalized() {
        let out = fpc_eval(&start_packet());
        let v: Vec<f64> = serde_json::from_str(&out).unwrap();
        assert_eq!(v.len(), 4);
        let sum: f64 = v.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "eval should sum to 1, got {v:?}");
        assert!(v.iter().all(|&x| x >= 0.0));
    }

    #[test]
    fn best_move_is_legal() {
        let pkt = start_packet();
        let out = fpc_best_move(&pkt, 0);
        let mv: MoveJson = serde_json::from_str(&out).unwrap();
        let legal: Vec<MoveJson> = serde_json::from_str(&fpc_legal_moves(&pkt)).unwrap();
        assert!(
            legal.iter().any(|m| m.fr == mv.fr && m.fc == mv.fc && m.tr == mv.tr && m.tc == mv.tc),
            "best move {out} not in legal set"
        );
    }

    #[test]
    fn attack_map_shape() {
        let out = fpc_attack_map(&start_packet(), "R");
        assert_eq!(out.len(), 196);
        assert!(out.chars().all(|c| c == '0' || c == '1'));
        assert!(out.contains('1'), "red should attack some squares at start");
    }

    #[test]
    fn analyze_runs() {
        // a couple of plies of history
        let legal: Vec<MoveJson> =
            serde_json::from_str(&fpc_legal_moves(&start_packet())).unwrap();
        let hist = vec![legal[0]];
        let out = fpc_analyze(&serde_json::to_string(&hist).unwrap(), 0);
        let plies: Vec<AnalyzePly> = serde_json::from_str(&out).unwrap();
        assert_eq!(plies.len(), 1);
        assert_eq!(plies[0].eval.len(), 4);
    }
}
