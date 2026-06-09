//! Move generation, attack detection, and legality. Faithful port of game.js.

use crate::types::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Move {
    pub fr: i32,
    pub fc: i32,
    pub tr: i32,
    pub tc: i32,
    pub promo: bool,
}

#[inline]
fn at(b: &Board, r: i32, c: i32) -> Cell {
    b[r as usize][c as usize]
}

/// True if every square strictly between (pr,pc) and (tr,tc) is empty & playable.
pub fn clear_path(b: &Board, pr: i32, pc: i32, tr: i32, tc: i32) -> bool {
    let sr = (tr - pr).signum();
    let sc = (tc - pc).signum();
    let mut r = pr + sr;
    let mut c = pc + sc;
    while r != tr || c != tc {
        if !is_playable(r, c) {
            return false;
        }
        if at(b, r, c).is_some() {
            return false;
        }
        r += sr;
        c += sc;
    }
    true
}

/// Can piece `p` at (pr,pc) attack square (tr,tc)? Ignores turn/elimination.
pub fn piece_attacks(b: &Board, p: Piece, pr: i32, pc: i32, tr: i32, tc: i32) -> bool {
    let dr = tr - pr;
    let dc = tc - pc;
    if dr == 0 && dc == 0 {
        return false;
    }
    match p.kind {
        Kind::K => dr.abs().max(dc.abs()) == 1,
        Kind::N => (dr.abs() == 1 && dc.abs() == 2) || (dr.abs() == 2 && dc.abs() == 1),
        Kind::P => pawn_caps(p.color).iter().any(|o| o.0 == dr && o.1 == dc),
        Kind::B => dr.abs() == dc.abs() && clear_path(b, pr, pc, tr, tc),
        Kind::R => (dr == 0 || dc == 0) && clear_path(b, pr, pc, tr, tc),
        Kind::Q => {
            (dr == 0 || dc == 0 || dr.abs() == dc.abs()) && clear_path(b, pr, pc, tr, tc)
        }
    }
}

/// Is (tr,tc) attacked by any active piece not of `def_color`?
pub fn attacked(b: &Board, elim: &[bool; 4], tr: i32, tc: i32, def_color: Color) -> bool {
    for r in 0..N as i32 {
        for c in 0..N as i32 {
            if let Some(p) = at(b, r, c) {
                if p.color == def_color || elim[p.color.idx()] {
                    continue;
                }
                if piece_attacks(b, p, r, c, tr, tc) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn find_king(b: &Board, color: Color) -> Option<(i32, i32)> {
    for r in 0..N as i32 {
        for c in 0..N as i32 {
            if let Some(p) = at(b, r, c) {
                if p.color == color && p.kind == Kind::K {
                    return Some((r, c));
                }
            }
        }
    }
    None
}

pub fn king_attacked(b: &Board, elim: &[bool; 4], color: Color) -> bool {
    match find_king(b, color) {
        None => true,
        Some((kr, kc)) => attacked(b, elim, kr, kc, color),
    }
}

/// Active colours whose piece attacks `color`'s king, in board scan order.
pub fn checkers(b: &Board, elim: &[bool; 4], color: Color) -> Vec<Color> {
    let k = match find_king(b, color) {
        None => return Vec::new(),
        Some(k) => k,
    };
    let mut out: Vec<Color> = Vec::new();
    for r in 0..N as i32 {
        for c in 0..N as i32 {
            if let Some(p) = at(b, r, c) {
                if p.color == color || elim[p.color.idx()] {
                    continue;
                }
                if piece_attacks(b, p, r, c, k.0, k.1) && !out.contains(&p.color) {
                    out.push(p.color);
                }
            }
        }
    }
    out
}

/// Can a step piece of `color` land on (r,c)? (own piece / live king block it)
#[inline]
fn can_land(b: &Board, elim: &[bool; 4], color: Color, r: i32, c: i32) -> bool {
    if !is_playable(r, c) {
        return false;
    }
    match at(b, r, c) {
        None => true,
        Some(occ) => occ.color != color && !(occ.kind == Kind::K && !elim[occ.color.idx()]),
    }
}

fn add_slide(
    b: &Board,
    elim: &[bool; 4],
    color: Color,
    fr: i32,
    fc: i32,
    dirs: &[(i32, i32)],
    out: &mut Vec<Move>,
) {
    for &(dr, dc) in dirs {
        let mut r = fr + dr;
        let mut c = fc + dc;
        while is_playable(r, c) {
            match at(b, r, c) {
                None => out.push(Move { fr, fc, tr: r, tc: c, promo: false }),
                Some(occ) => {
                    if occ.color != color && !(occ.kind == Kind::K && !elim[occ.color.idx()]) {
                        out.push(Move { fr, fc, tr: r, tc: c, promo: false });
                    }
                    break;
                }
            }
            r += dr;
            c += dc;
        }
    }
}

pub fn pseudo_moves(b: &Board, elim: &[bool; 4], color: Color) -> Vec<Move> {
    let mut out: Vec<Move> = Vec::new();
    for fr in 0..N as i32 {
        for fc in 0..N as i32 {
            let p = match at(b, fr, fc) {
                Some(p) if p.color == color => p,
                _ => continue,
            };
            match p.kind {
                Kind::P => {
                    let (fdr, fdc) = pawn_fwd(color);
                    let (r1, c1) = (fr + fdr, fc + fdc);
                    if is_playable(r1, c1) && at(b, r1, c1).is_none() {
                        out.push(Move {
                            fr,
                            fc,
                            tr: r1,
                            tc: c1,
                            promo: pawn_promo(color, r1, c1),
                        });
                        if pawn_home(color, fr, fc) {
                            let (r2, c2) = (fr + 2 * fdr, fc + 2 * fdc);
                            if is_playable(r2, c2) && at(b, r2, c2).is_none() {
                                out.push(Move { fr, fc, tr: r2, tc: c2, promo: false });
                            }
                        }
                    }
                    for &(cdr, cdc) in pawn_caps(color).iter() {
                        let (r, c) = (fr + cdr, fc + cdc);
                        if !is_playable(r, c) {
                            continue;
                        }
                        if let Some(occ) = at(b, r, c) {
                            if occ.color != color
                                && !(occ.kind == Kind::K && !elim[occ.color.idx()])
                            {
                                out.push(Move {
                                    fr,
                                    fc,
                                    tr: r,
                                    tc: c,
                                    promo: pawn_promo(color, r, c),
                                });
                            }
                        }
                    }
                }
                Kind::N => {
                    for &(dr, dc) in KNIGHT.iter() {
                        let (r, c) = (fr + dr, fc + dc);
                        if can_land(b, elim, color, r, c) {
                            out.push(Move { fr, fc, tr: r, tc: c, promo: false });
                        }
                    }
                }
                Kind::K => {
                    for &(dr, dc) in ALL8.iter() {
                        let (r, c) = (fr + dr, fc + dc);
                        if can_land(b, elim, color, r, c) {
                            out.push(Move { fr, fc, tr: r, tc: c, promo: false });
                        }
                    }
                }
                Kind::B => add_slide(b, elim, color, fr, fc, &DIAG, &mut out),
                Kind::R => add_slide(b, elim, color, fr, fc, &ORTH, &mut out),
                Kind::Q => add_slide(b, elim, color, fr, fc, &ALL8, &mut out),
            }
        }
    }
    out
}

/// Apply a move in place (with auto-queen promotion). Caller handles scoring.
pub fn apply_to(b: &mut Board, mv: Move) {
    let p = b[mv.fr as usize][mv.fc as usize].take().expect("no piece at source");
    let np = if p.kind == Kind::P && pawn_promo(p.color, mv.tr, mv.tc) {
        Piece { color: p.color, kind: Kind::Q }
    } else {
        p
    };
    b[mv.tr as usize][mv.tc as usize] = Some(np);
}

pub fn legal_moves(b: &Board, elim: &[bool; 4], color: Color) -> Vec<Move> {
    let mut out: Vec<Move> = Vec::new();
    for mv in pseudo_moves(b, elim, color) {
        let mut nb = *b;
        apply_to(&mut nb, mv);
        if !king_attacked(&nb, elim, color) {
            out.push(mv);
        }
    }
    out
}
