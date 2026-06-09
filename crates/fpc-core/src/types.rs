//! Core types, constants, and board setup. Faithful port of game.js.

pub const N: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Color {
    R,
    B,
    Y,
    G,
}

/// Turn order, matching game.js `ORDER = ["R","B","Y","G"]`.
pub const ORDER: [Color; 4] = [Color::R, Color::B, Color::Y, Color::G];

impl Color {
    #[inline]
    pub fn idx(self) -> usize {
        match self {
            Color::R => 0,
            Color::B => 1,
            Color::Y => 2,
            Color::G => 3,
        }
    }
    #[inline]
    pub fn from_idx(i: usize) -> Color {
        ORDER[i]
    }
    pub fn from_char(ch: char) -> Color {
        match ch {
            'R' => Color::R,
            'B' => Color::B,
            'Y' => Color::Y,
            'G' => Color::G,
            _ => panic!("bad color char: {ch}"),
        }
    }
    pub fn to_char(self) -> char {
        match self {
            Color::R => 'R',
            Color::B => 'B',
            Color::Y => 'Y',
            Color::G => 'G',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    P,
    N,
    B,
    R,
    Q,
    K,
}

impl Kind {
    pub fn from_char(ch: char) -> Kind {
        match ch {
            'P' => Kind::P,
            'N' => Kind::N,
            'B' => Kind::B,
            'R' => Kind::R,
            'Q' => Kind::Q,
            'K' => Kind::K,
            _ => panic!("bad kind char: {ch}"),
        }
    }
    pub fn to_char(self) -> char {
        match self {
            Kind::P => 'P',
            Kind::N => 'N',
            Kind::B => 'B',
            Kind::R => 'R',
            Kind::Q => 'Q',
            Kind::K => 'K',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: Kind,
}

pub type Cell = Option<Piece>;
pub type Board = [[Cell; N]; N];

/// Material values (chess.com 4PC), matching game.js `VALUE`.
#[inline]
pub fn value(k: Kind) -> i32 {
    match k {
        Kind::P => 1,
        Kind::N => 3,
        Kind::B => 5,
        Kind::R => 5,
        Kind::Q => 9,
        Kind::K => 20,
    }
}

pub const ORTH: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
pub const DIAG: [(i32, i32); 4] = [(-1, -1), (-1, 1), (1, -1), (1, 1)];
pub const ALL8: [(i32, i32); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (-1, 1),
    (1, -1),
    (1, 1),
];
pub const KNIGHT: [(i32, i32); 8] = [
    (-2, -1),
    (-2, 1),
    (2, -1),
    (2, 1),
    (-1, -2),
    (1, -2),
    (-1, 2),
    (1, 2),
];

/// Pawn capture (forward-diagonal) offsets per colour.
#[inline]
pub fn pawn_caps(c: Color) -> [(i32, i32); 2] {
    match c {
        Color::R => [(-1, -1), (-1, 1)],
        Color::Y => [(1, -1), (1, 1)],
        Color::B => [(-1, 1), (1, 1)],
        Color::G => [(-1, -1), (1, -1)],
    }
}

/// Pawn forward offset per colour.
#[inline]
pub fn pawn_fwd(c: Color) -> (i32, i32) {
    match c {
        Color::R => (-1, 0),
        Color::Y => (1, 0),
        Color::B => (0, 1),
        Color::G => (0, -1),
    }
}

/// Playable squares: 14x14 minus the four 3x3 corners.
#[inline]
pub fn is_playable(r: i32, c: i32) -> bool {
    if r < 0 || r > 13 || c < 0 || c > 13 {
        return false;
    }
    !((r < 3 || r > 10) && (c < 3 || c > 10))
}

#[inline]
pub fn pawn_home(color: Color, r: i32, c: i32) -> bool {
    (color == Color::R && r == 12)
        || (color == Color::Y && r == 1)
        || (color == Color::B && c == 1)
        || (color == Color::G && c == 12)
}

#[inline]
pub fn pawn_promo(color: Color, r: i32, c: i32) -> bool {
    // chess.com: promote on the 8th rank — the first square past the centre line.
    (color == Color::R && r == 6)
        || (color == Color::Y && r == 7)
        || (color == Color::B && c == 7)
        || (color == Color::G && c == 6)
}

/// Parse the 196-cell row-major board string ("RP"|".."|"##") into a Board.
/// This is the wire format used across the JS/Rust/WASM boundary.
pub fn board_from_str(s: &str) -> Option<Board> {
    let b = s.as_bytes();
    if b.len() != N * N * 2 {
        return None;
    }
    let mut board: Board = [[None; N]; N];
    for r in 0..N {
        for c in 0..N {
            let i = (r * N + c) * 2;
            let a = b[i] as char;
            let k = b[i + 1] as char;
            if a == '.' || a == '#' {
                continue;
            }
            board[r][c] = Some(Piece {
                color: Color::from_char(a),
                kind: Kind::from_char(k),
            });
        }
    }
    Some(board)
}

/// Serialize a Board to the 196-cell row-major wire string.
pub fn board_to_str(board: &Board) -> String {
    let mut s = String::with_capacity(N * N * 2);
    for r in 0..N {
        for c in 0..N {
            if !is_playable(r as i32, c as i32) {
                s.push_str("##");
            } else if let Some(p) = board[r][c] {
                s.push(p.color.to_char());
                s.push(p.kind.to_char());
            } else {
                s.push_str("..");
            }
        }
    }
    s
}

/// Initial position, matching game.js `newBoard()`.
pub fn new_board() -> Board {
    use Kind::*;
    let mut b: Board = [[None; 14]; 14];
    // chess.com "Modern" FFA: Queen on each player's own left, King on own right.
    let red = [R, N, B, Q, K, B, N, R];
    let yellow = [R, N, B, K, Q, B, N, R];
    let blue = [R, N, B, Q, K, B, N, R];
    let green = [R, N, B, K, Q, B, N, R];
    for i in 0..8 {
        let col = 3 + i;
        let row = 3 + i;
        b[13][col] = Some(Piece { color: Color::R, kind: red[i] });
        b[12][col] = Some(Piece { color: Color::R, kind: P });
        b[0][col] = Some(Piece { color: Color::Y, kind: yellow[i] });
        b[1][col] = Some(Piece { color: Color::Y, kind: P });
        b[row][0] = Some(Piece { color: Color::B, kind: blue[i] });
        b[row][1] = Some(Piece { color: Color::B, kind: P });
        b[row][13] = Some(Piece { color: Color::G, kind: green[i] });
        b[row][12] = Some(Piece { color: Color::G, kind: P });
    }
    b
}
