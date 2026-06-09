//! Position -> fixed feature vector, shared by self-play data generation, the
//! NN agent, and the Rust trainer (FEAT_DIM is the single source of truth).
//!
//! Layout (absolute, not perspective-relative):
//!   per colour c, base = c*8:
//!     base+0..6  piece counts [P,N,B,R,Q,K] / 8
//!     base+6     banked score / 100
//!     base+7     eliminated flag
//!   32..36       side-to-move one-hot
//!   per colour c, sbase = 36 + c*4   (spatial/positional block):
//!     sbase+0    pawn advancement sum (toward promotion), normalized
//!     sbase+1    central occupancy (pieces in the 6x6 centre) / 8
//!     sbase+2    king in check (0/1)
//!     sbase+3    king safety: safe escape squares / 8
//! => 4*8 + 4 + 4*4 = 52

use crate::*;

pub const FEAT_DIM: usize = 52;

/// How far a pawn at (r,c) has advanced from its home rank toward promotion (0..9).
#[inline]
fn pawn_advance(color: Color, r: i32, c: i32) -> i32 {
    match color {
        Color::R => 12 - r,
        Color::Y => r - 1,
        Color::B => c - 1,
        Color::G => 12 - c,
    }
}

pub fn features(st: &State) -> [f32; FEAT_DIM] {
    let mut f = [0.0f32; FEAT_DIM];
    let b = &st.board;

    let mut pawn_adv = [0.0f32; 4];
    let mut center = [0.0f32; 4];
    let mut king_pos: [Option<(i32, i32)>; 4] = [None; 4];

    // single board pass: counts, pawn advancement, central occupancy, king squares
    for r in 0..14i32 {
        for c in 0..14i32 {
            if let Some(p) = b[r as usize][c as usize] {
                let ci = p.color.idx();
                let base = ci * 8;
                let ki = match p.kind {
                    Kind::P => 0,
                    Kind::N => 1,
                    Kind::B => 2,
                    Kind::R => 3,
                    Kind::Q => 4,
                    Kind::K => 5,
                };
                f[base + ki] += 1.0;
                if p.kind == Kind::P {
                    pawn_adv[ci] += pawn_advance(p.color, r, c) as f32;
                }
                if p.kind == Kind::K {
                    king_pos[ci] = Some((r, c));
                }
                if (4..=9).contains(&r) && (4..=9).contains(&c) {
                    center[ci] += 1.0;
                }
            }
        }
    }

    for ci in 0..4 {
        let base = ci * 8;
        for k in 0..6 {
            f[base + k] /= 8.0;
        }
        f[base + 6] = st.scores[ci] as f32 / 100.0;
        f[base + 7] = if st.eliminated[ci] { 1.0 } else { 0.0 };

        let sbase = 36 + ci * 4;
        f[sbase + 0] = pawn_adv[ci] / 72.0; // 8 pawns * 9 max
        f[sbase + 1] = center[ci] / 8.0;

        // king-in-check and king safety, for active players only
        let color = Color::from_idx(ci);
        if !st.eliminated[ci] {
            if let Some((kr, kc)) = king_pos[ci] {
                if attacked(b, &st.eliminated, kr, kc, color) {
                    f[sbase + 2] = 1.0;
                }
                let mut safe = 0;
                for &(dr, dc) in ALL8.iter() {
                    let (r, c) = (kr + dr, kc + dc);
                    if !is_playable(r, c) {
                        continue;
                    }
                    // can the king step here? (empty or capturable non-live-king)
                    let landable = match b[r as usize][c as usize] {
                        None => true,
                        Some(occ) => {
                            occ.color != color
                                && !(occ.kind == Kind::K && !st.eliminated[occ.color.idx()])
                        }
                    };
                    if landable && !attacked(b, &st.eliminated, r, c, color) {
                        safe += 1;
                    }
                }
                f[sbase + 3] = safe as f32 / 8.0;
            }
        }
    }

    if let Some(c) = st.current {
        f[32 + c.idx()] = 1.0;
    }
    f
}

/// Each colour's share of total banked points (the value-net training target).
pub fn score_shares(scores: &[i32; 4]) -> [f32; 4] {
    let tot: i32 = scores.iter().sum();
    if tot <= 0 {
        return [0.25; 4];
    }
    let mut s = [0.0f32; 4];
    for i in 0..4 {
        s[i] = scores[i] as f32 / tot as f32;
    }
    s
}
