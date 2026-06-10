//! Position -> fixed feature vector, shared by self-play data generation, the
//! NN agent, and the Rust trainer.
//!
//! Two formats:
//!
//! `features` (FEAT_DIM=52, absolute — legacy, consumed by pre-rel models):
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
//!
//! `features_rel` (FEAT_DIM_REL=48, perspective-relative): the same per-colour
//! blocks, but seats are ordered relative to the side to move — k=0 me, k=1
//! next to act, k=2 across, k=3 previous — so the net learns each pattern once
//! instead of once per seat rotation. The side-to-move one-hot disappears
//! (seat 0 is always the mover). Output convention follows: a net trained on
//! these predicts score-shares in the same rotated order.
//!   per relative seat k, base = k*12:
//!     base+0..6  piece counts / 8
//!     base+6     banked score / 100
//!     base+7     eliminated flag
//!     base+8     pawn advancement, normalized
//!     base+9     central occupancy / 8
//!     base+10    king in check (0/1)
//!     base+11    king safety / 8
//! => 4*12 = 48
//!
//! `features_tac` (FEAT_DIM_TAC=60): `features_rel` plus a tactical block of 3
//! dims per relative seat k, base = 48 + k*3:
//!     base+0     own piece value under enemy attack / 24
//!     base+1     hanging value (attacked and not defended) / 24
//!     base+2     attacked piece count / 8

use crate::*;

pub const FEAT_DIM: usize = 52;
pub const FEAT_DIM_REL: usize = 48;
pub const FEAT_DIM_TAC: usize = 60;

/// Raw (unnormalized) per-colour stats behind both feature formats.
struct SeatStats {
    counts: [f32; 6], // piece counts [P,N,B,R,Q,K]
    pawn_adv: f32,    // advancement sum toward promotion
    center: f32,      // pieces in the 6x6 centre
    check: f32,       // king in check (0/1, active players only)
    safety: f32,      // safe king escape squares (0..8, active players only)
}

/// One board pass + king-safety probes, shared by `features`/`features_rel`.
fn seat_stats(st: &State) -> [SeatStats; 4] {
    let b = &st.board;
    let mut s: [SeatStats; 4] = std::array::from_fn(|_| SeatStats {
        counts: [0.0; 6],
        pawn_adv: 0.0,
        center: 0.0,
        check: 0.0,
        safety: 0.0,
    });
    let mut king_pos: [Option<(i32, i32)>; 4] = [None; 4];

    for r in 0..14i32 {
        for c in 0..14i32 {
            if let Some(p) = b[r as usize][c as usize] {
                let ci = p.color.idx();
                let ki = match p.kind {
                    Kind::P => 0,
                    Kind::N => 1,
                    Kind::B => 2,
                    Kind::R => 3,
                    Kind::Q => 4,
                    Kind::K => 5,
                };
                s[ci].counts[ki] += 1.0;
                if p.kind == Kind::P {
                    s[ci].pawn_adv += pawn_advance(p.color, r, c) as f32;
                }
                if p.kind == Kind::K {
                    king_pos[ci] = Some((r, c));
                }
                if (4..=9).contains(&r) && (4..=9).contains(&c) {
                    s[ci].center += 1.0;
                }
            }
        }
    }

    for ci in 0..4 {
        let color = Color::from_idx(ci);
        if st.eliminated[ci] {
            continue;
        }
        if let Some((kr, kc)) = king_pos[ci] {
            if attacked(b, &st.eliminated, kr, kc, color) {
                s[ci].check = 1.0;
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
            s[ci].safety = safe as f32;
        }
    }
    s
}

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
    let s = seat_stats(st);
    let mut f = [0.0f32; FEAT_DIM];
    for ci in 0..4 {
        let base = ci * 8;
        for k in 0..6 {
            f[base + k] = s[ci].counts[k] / 8.0;
        }
        f[base + 6] = st.scores[ci] as f32 / 100.0;
        f[base + 7] = if st.eliminated[ci] { 1.0 } else { 0.0 };

        let sbase = 36 + ci * 4;
        f[sbase + 0] = s[ci].pawn_adv / 72.0; // 8 pawns * 9 max
        f[sbase + 1] = s[ci].center / 8.0;
        f[sbase + 2] = s[ci].check;
        f[sbase + 3] = s[ci].safety / 8.0;
    }
    if let Some(c) = st.current {
        f[32 + c.idx()] = 1.0;
    }
    f
}

/// Perspective-relative features: seat k = colour (mover + k) % 4. Only valid
/// for positions with a side to move (callers handle terminal states).
pub fn features_rel(st: &State) -> [f32; FEAT_DIM_REL] {
    let mover = st.current.expect("features_rel needs a side to move").idx();
    let s = seat_stats(st);
    let mut f = [0.0f32; FEAT_DIM_REL];
    for k in 0..4 {
        let ci = (mover + k) % 4;
        let base = k * 12;
        for j in 0..6 {
            f[base + j] = s[ci].counts[j] / 8.0;
        }
        f[base + 6] = st.scores[ci] as f32 / 100.0;
        f[base + 7] = if st.eliminated[ci] { 1.0 } else { 0.0 };
        f[base + 8] = s[ci].pawn_adv / 72.0;
        f[base + 9] = s[ci].center / 8.0;
        f[base + 10] = s[ci].check;
        f[base + 11] = s[ci].safety / 8.0;
    }
    f
}

/// Per-colour tactical stats: [attacked piece value, hanging value (attacked
/// and not defended by own colour), attacked piece count]. Reverse attack
/// probes per piece; the defended probe only runs for attacked pieces.
fn tac_stats(st: &State) -> [[f32; 3]; 4] {
    let b = &st.board;
    let mut out = [[0.0f32; 3]; 4];
    for r in 0..14i32 {
        for c in 0..14i32 {
            let Some(p) = b[r as usize][c as usize] else { continue };
            let ci = p.color.idx();
            if st.eliminated[ci] {
                continue;
            }
            if attacked(b, &st.eliminated, r, c, p.color) {
                let v = value(p.kind) as f32;
                out[ci][0] += v;
                out[ci][2] += 1.0;
                if !defended(b, r, c, p.color) {
                    out[ci][1] += v;
                }
            }
        }
    }
    out
}

/// `features_rel` plus per-seat tactical pressure (see module docs). Same
/// mover-relative seat order and output convention as `features_rel`.
pub fn features_tac(st: &State) -> [f32; FEAT_DIM_TAC] {
    let mover = st.current.expect("features_tac needs a side to move").idx();
    let rel = features_rel(st);
    let t = tac_stats(st);
    let mut f = [0.0f32; FEAT_DIM_TAC];
    f[..FEAT_DIM_REL].copy_from_slice(&rel);
    for k in 0..4 {
        let ci = (mover + k) % 4;
        let base = FEAT_DIM_REL + k * 3;
        f[base + 0] = t[ci][0] / 24.0;
        f[base + 1] = t[ci][1] / 24.0;
        f[base + 2] = t[ci][2] / 8.0;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    /// features_rel must be features re-indexed by seat offset from the mover.
    #[test]
    fn rel_matches_abs_rotation() {
        let mut st = State::new_game();
        let mut rng = 0xFEED_u64;
        let mut checked = 0;
        for _ in 0..200 {
            if st.over {
                break;
            }
            let abs = features(&st);
            let rel = features_rel(&st);
            let mover = st.current.unwrap().idx();
            for k in 0..4 {
                let ci = (mover + k) % 4;
                let (rb, ab, sb) = (k * 12, ci * 8, 36 + ci * 4);
                for j in 0..8 {
                    assert_eq!(rel[rb + j], abs[ab + j], "block k={k} j={j}");
                }
                for j in 0..4 {
                    assert_eq!(rel[rb + 8 + j], abs[sb + j], "spatial k={k} j={j}");
                }
            }
            checked += 1;
            // splitmix-ish step for a random legal move
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mv = st.current_legal[(rng >> 33) as usize % st.current_legal.len()];
            st.make_move(mv);
        }
        assert!(checked > 100, "too few positions checked: {checked}");
    }

    /// features_tac = features_rel prefix + sane tactical block (hanging value
    /// never exceeds attacked value; counts/values move together).
    #[test]
    fn tac_extends_rel() {
        let mut st = State::new_game();
        let mut rng = 0xBEEF_u64;
        let mut saw_attacked = false;
        for _ in 0..200 {
            if st.over {
                break;
            }
            let rel = features_rel(&st);
            let tac = features_tac(&st);
            assert_eq!(&tac[..FEAT_DIM_REL], &rel[..]);
            for k in 0..4 {
                let base = FEAT_DIM_REL + k * 3;
                let (av, hv, ac) = (tac[base], tac[base + 1], tac[base + 2]);
                assert!(hv <= av + 1e-6, "hanging {hv} > attacked {av}");
                assert_eq!(av > 0.0, ac > 0.0, "value/count mismatch");
                if av > 0.0 {
                    saw_attacked = true;
                }
            }
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mv = st.current_legal[(rng >> 33) as usize % st.current_legal.len()];
            st.make_move(mv);
        }
        assert!(saw_attacked, "no attacked pieces in 200 random plies?");
    }
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
