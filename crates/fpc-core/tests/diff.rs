//! Differential test: replay oracle games (generated from game.js) through
//! fpc-core, asserting board / turn / elimination / scores and the full
//! legal-move set match the JS engine at every position.
//!
//! Regenerate the data with:  node tools/oracle.mjs

use std::collections::BTreeSet;

use fpc_core::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct MoveJson {
    fr: i32,
    fc: i32,
    tr: i32,
    tc: i32,
    #[serde(default)]
    promo: bool,
}

#[derive(Deserialize)]
struct Step {
    board: String,
    eliminated: Vec<String>,
    scores: Scores,
    current: String,
    legal: Vec<MoveJson>,
    chosen: MoveJson,
}

#[derive(Deserialize)]
struct Game {
    steps: Vec<Step>,
    #[serde(rename = "finalScores")]
    final_scores: Scores,
    eliminated: Vec<String>,
    over: bool,
}

#[derive(Deserialize)]
struct Scores {
    #[serde(rename = "R")]
    r: i32,
    #[serde(rename = "B")]
    b: i32,
    #[serde(rename = "Y")]
    y: i32,
    #[serde(rename = "G")]
    g: i32,
}

impl Scores {
    fn to_arr(&self) -> [i32; 4] {
        [self.r, self.b, self.y, self.g]
    }
}

fn elim_arr(colors: &[String]) -> [bool; 4] {
    let mut e = [false; 4];
    for s in colors {
        e[Color::from_char(s.chars().next().unwrap()).idx()] = true;
    }
    e
}

/// Parse the 196-char row-major board string ("RP"/".."/"##") into a Board.
fn parse_board(s: &str) -> Board {
    let bytes = s.as_bytes();
    assert_eq!(bytes.len(), 14 * 14 * 2, "unexpected board string length");
    let mut b: Board = [[None; 14]; 14];
    for r in 0..14 {
        for c in 0..14 {
            let i = (r * 14 + c) * 2;
            let a = bytes[i] as char;
            let k = bytes[i + 1] as char;
            if a == '.' || a == '#' {
                continue;
            }
            b[r][c] = Some(Piece {
                color: Color::from_char(a),
                kind: Kind::from_char(k),
            });
        }
    }
    b
}

type MoveKey = (i32, i32, i32, i32, bool);

fn move_key(m: &Move) -> MoveKey {
    (m.fr, m.fc, m.tr, m.tc, m.promo)
}
fn move_key_json(m: &MoveJson) -> MoveKey {
    (m.fr, m.fc, m.tr, m.tc, m.promo)
}

fn boards_eq(a: &Board, b: &Board) -> bool {
    for r in 0..14 {
        for c in 0..14 {
            if a[r][c] != b[r][c] {
                return false;
            }
        }
    }
    true
}

#[test]
fn matches_game_js() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/oracle.json");
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!("could not read {path}: {e}\nRegenerate with: node tools/oracle.mjs");
    });
    let games: Vec<Game> = serde_json::from_slice(&bytes).expect("parse oracle.json");

    let mut total_positions = 0usize;
    for (gi, game) in games.iter().enumerate() {
        let mut st = State::new_game();
        for (si, step) in game.steps.iter().enumerate() {
            let ctx = format!("game {gi} step {si}");

            // current player
            let want_current = Color::from_char(step.current.chars().next().unwrap());
            assert_eq!(st.current, Some(want_current), "{ctx}: current player");

            // bookkeeping
            assert_eq!(st.eliminated, elim_arr(&step.eliminated), "{ctx}: eliminated");
            assert_eq!(st.scores, step.scores.to_arr(), "{ctx}: scores");
            assert!(boards_eq(&st.board, &parse_board(&step.board)), "{ctx}: board");

            // full legal-move set
            let got: BTreeSet<MoveKey> = st.current_legal.iter().map(move_key).collect();
            let want: BTreeSet<MoveKey> = step.legal.iter().map(move_key_json).collect();
            assert_eq!(got, want, "{ctx}: legal moves differ");

            // apply the same move game.js played
            st.make_move(Move {
                fr: step.chosen.fr,
                fc: step.chosen.fc,
                tr: step.chosen.tr,
                tc: step.chosen.tc,
                promo: step.chosen.promo,
            });
            total_positions += 1;
        }

        // terminal state must agree
        assert_eq!(st.over, game.over, "game {gi}: over flag");
        assert_eq!(st.scores, game.final_scores.to_arr(), "game {gi}: final scores");
        assert_eq!(st.eliminated, elim_arr(&game.eliminated), "game {gi}: final eliminated");
    }

    eprintln!("verified {} games, {} positions", games.len(), total_positions);
}
