//! Game state, turn flow, elimination, scoring, and draw detection.
//! Faithful port of rules.js (the canonical JS engine).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::movegen::*;
use crate::types::*;

/// End the game if this many plies pass with no capture and no pawn move.
pub const DRAW_NO_PROGRESS: i32 = 100;

#[derive(Clone)]
pub struct State {
    pub board: Board,
    pub eliminated: [bool; 4],
    pub scores: [i32; 4],
    pub idx: usize,
    pub current: Option<Color>,
    pub current_legal: Vec<Move>,
    pub last_mover: Option<Color>,
    pub last_move: Option<Move>,
    pub over: bool,
    /// Plies since the last capture or pawn move (no-progress draw counter).
    pub no_progress: i32,
    /// (board, side-to-move, eliminated) -> times seen, for threefold repetition.
    pub repeats: HashMap<u64, u8>,
    /// When false (search lookahead), skip repetition bookkeeping + draw checks
    /// so cloning stays cheap. The canonical game path keeps this true.
    pub track_draws: bool,
}

impl State {
    pub fn new_game() -> Self {
        let mut s = State {
            board: new_board(),
            eliminated: [false; 4],
            scores: [0; 4],
            idx: 3, // so the first advance lands on Red
            current: None,
            current_legal: Vec::new(),
            last_mover: None,
            last_move: None,
            over: false,
            no_progress: 0,
            repeats: HashMap::new(),
            track_draws: true,
        };
        s.advance_turn();
        s
    }

    /// Build a live state from a position packet (no move history). Used by the
    /// WASM API, which receives standalone positions. `current_legal` is computed;
    /// `over` is left false (callers handle the no-legal-moves case). Draw history
    /// is unknown from a bare packet, so the no-progress/repetition state starts
    /// fresh.
    pub fn from_position(
        board: Board,
        eliminated: [bool; 4],
        scores: [i32; 4],
        current: Color,
    ) -> Self {
        let current_legal = legal_moves(&board, &eliminated, current);
        State {
            board,
            eliminated,
            scores,
            idx: current.idx(),
            current: Some(current),
            current_legal,
            last_mover: None,
            last_move: None,
            over: false,
            no_progress: 0,
            repeats: HashMap::new(),
            track_draws: true,
        }
    }

    /// A cheap clone for search lookahead: drops the repetition table and turns
    /// off draw bookkeeping, so deeper clones don't copy/populate the map.
    pub fn for_search(&self) -> State {
        State {
            board: self.board,
            eliminated: self.eliminated,
            scores: self.scores,
            idx: self.idx,
            current: self.current,
            current_legal: Vec::new(), // overwritten by the imminent make_move
            last_mover: self.last_mover,
            last_move: self.last_move,
            over: self.over,
            no_progress: self.no_progress,
            repeats: HashMap::new(),
            track_draws: false,
        }
    }

    #[inline]
    pub fn active_count(&self) -> usize {
        ORDER.iter().filter(|c| !self.eliminated[c.idx()]).count()
    }

    /// Advance to the next player who has a legal move, eliminating players who
    /// are checkmated or stalemated and crediting the appropriate scorer.
    pub fn advance_turn(&mut self) {
        loop {
            if self.active_count() <= 1 {
                self.current = None;
                self.over = true;
                return;
            }
            self.idx = (self.idx + 1) % 4;
            let c = ORDER[self.idx];
            if self.eliminated[c.idx()] {
                continue;
            }
            let legal = legal_moves(&self.board, &self.eliminated, c);
            if legal.is_empty() {
                let chk = checkers(&self.board, &self.eliminated, c);
                self.eliminated[c.idx()] = true;
                if let Some(&first) = chk.first() {
                    // checkmate: credit a checker
                    self.scores[first.idx()] += 20;
                } else if let Some(lm) = self.last_mover {
                    // stalemate: credit the stalemater
                    if lm != c && !self.eliminated[lm.idx()] {
                        self.scores[lm.idx()] += 20;
                    }
                }
                continue;
            }
            // Settled turn: `c` has a legal move. Check draws (canonical path only).
            self.current = Some(c);
            self.current_legal = legal;
            if self.track_draws && self.is_draw(c) {
                self.current = None;
                self.over = true;
            }
            return;
        }
    }

    pub fn make_move(&mut self, mv: Move) {
        let p = self.board[mv.fr as usize][mv.fc as usize].expect("no piece at source");
        let cap = self.board[mv.tr as usize][mv.tc as usize];
        if let Some(cap) = cap {
            // dead pieces (owner already eliminated) are worth 0; live captures score material
            if !self.eliminated[cap.color.idx()] {
                self.scores[p.color.idx()] += value(cap.kind);
            }
        }
        // no-progress counter resets on a capture or any pawn move (incl. promotion);
        // `p` is the piece before promotion, matching rules.js.
        self.no_progress = if cap.is_some() || p.kind == Kind::P {
            0
        } else {
            self.no_progress + 1
        };
        apply_to(&mut self.board, mv);
        self.last_mover = Some(p.color);
        self.last_move = Some(mv);
        self.advance_turn();
    }

    /// True if the position is a draw for the settled mover `c`. Order matches
    /// rules.js: no-progress and insufficient-material return before the
    /// repetition counter is touched.
    fn is_draw(&mut self, c: Color) -> bool {
        if self.no_progress >= DRAW_NO_PROGRESS {
            return true;
        }
        if self.only_kings_left() {
            return true;
        }
        let key = self.repeat_key(c);
        let n = self.repeats.entry(key).or_insert(0);
        *n += 1;
        *n >= 3
    }

    /// True if no active player has any piece other than a king.
    fn only_kings_left(&self) -> bool {
        for r in 0..N {
            for c in 0..N {
                if let Some(p) = self.board[r][c] {
                    if !self.eliminated[p.color.idx()] && p.kind != Kind::K {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Hash of (board, side-to-move, eliminated) — the threefold repetition key.
    fn repeat_key(&self, c: Color) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.board.hash(&mut h);
        c.hash(&mut h);
        self.eliminated.hash(&mut h);
        h.finish()
    }

    /// Final placement, colours ranked by score descending (ties keep ORDER).
    pub fn ranking(&self) -> Vec<Color> {
        let mut r: Vec<Color> = ORDER.to_vec();
        r.sort_by(|a, b| self.scores[b.idx()].cmp(&self.scores[a.idx()]));
        r
    }
}
