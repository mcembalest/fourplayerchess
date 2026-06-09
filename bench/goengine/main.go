// Real 4PC engine throughput in Go — faithful port of fpc-core (validated
// move-for-move against rules.js). Same splitmix64 RNG + identical move ordering
// as crates/fpc-train/src/bin/throughput.rs, so a correct port reproduces Rust's
// exact positions=499967 finished=1 for `2000 250` (gold-standard validation).
//
//   go build -o bench_goengine ./bench/goengine && ./bench_goengine 2000 250
package main

import (
	"fmt"
	"os"
	"strconv"
	"time"
)

// cell encoding: 0 = empty; else 1 + color*6 + kind. color R0 B1 Y2 G3; kind P0 N1 B2 R3 Q4 K5.
type Board [14][14]uint8

func enc(color, kind int) uint8 { return uint8(1 + color*6 + kind) }
func col(c uint8) int           { return int((c - 1) / 6) }
func knd(c uint8) int           { return int((c - 1) % 6) }

var value = [6]int{1, 3, 5, 5, 9, 20}

var orth = [4][2]int{{-1, 0}, {1, 0}, {0, -1}, {0, 1}}
var diag = [4][2]int{{-1, -1}, {-1, 1}, {1, -1}, {1, 1}}
var all8 = [8][2]int{{-1, 0}, {1, 0}, {0, -1}, {0, 1}, {-1, -1}, {-1, 1}, {1, -1}, {1, 1}}
var knight = [8][2]int{{-2, -1}, {-2, 1}, {2, -1}, {2, 1}, {-1, -2}, {1, -2}, {-1, 2}, {1, 2}}

func pawnFwd(c int) (int, int) {
	switch c {
	case 0:
		return -1, 0 // R
	case 1:
		return 0, 1 // B
	case 2:
		return 1, 0 // Y
	default:
		return 0, -1 // G
	}
}
func pawnCaps(c int) [2][2]int {
	switch c {
	case 0:
		return [2][2]int{{-1, -1}, {-1, 1}}
	case 1:
		return [2][2]int{{-1, 1}, {1, 1}}
	case 2:
		return [2][2]int{{1, -1}, {1, 1}}
	default:
		return [2][2]int{{-1, -1}, {1, -1}}
	}
}
func isPlayable(r, c int) bool {
	if r < 0 || r > 13 || c < 0 || c > 13 {
		return false
	}
	return !((r < 3 || r > 10) && (c < 3 || c > 10))
}
func pawnHome(color, r, c int) bool {
	return (color == 0 && r == 12) || (color == 2 && r == 1) || (color == 1 && c == 1) || (color == 3 && c == 12)
}
func pawnPromo(color, r, c int) bool {
	return (color == 0 && r == 6) || (color == 2 && r == 7) || (color == 1 && c == 7) || (color == 3 && c == 6)
}

type Move struct {
	fr, fc, tr, tc int
	promo          bool
}

func abs(x int) int {
	if x < 0 {
		return -x
	}
	return x
}
func sign(x int) int {
	if x > 0 {
		return 1
	}
	if x < 0 {
		return -1
	}
	return 0
}
func clearPath(b *Board, pr, pc, tr, tc int) bool {
	sr, sc := sign(tr-pr), sign(tc-pc)
	r, c := pr+sr, pc+sc
	for r != tr || c != tc {
		if !isPlayable(r, c) || b[r][c] != 0 {
			return false
		}
		r += sr
		c += sc
	}
	return true
}
func pieceAttacks(b *Board, cell uint8, pr, pc, tr, tc int) bool {
	dr, dc := tr-pr, tc-pc
	if dr == 0 && dc == 0 {
		return false
	}
	color, kind := col(cell), knd(cell)
	switch kind {
	case 5: // K
		return max2(abs(dr), abs(dc)) == 1
	case 1: // N
		return (abs(dr) == 1 && abs(dc) == 2) || (abs(dr) == 2 && abs(dc) == 1)
	case 0: // P
		for _, o := range pawnCaps(color) {
			if o[0] == dr && o[1] == dc {
				return true
			}
		}
		return false
	case 2: // B
		return abs(dr) == abs(dc) && clearPath(b, pr, pc, tr, tc)
	case 3: // R
		return (dr == 0 || dc == 0) && clearPath(b, pr, pc, tr, tc)
	case 4: // Q
		return (dr == 0 || dc == 0 || abs(dr) == abs(dc)) && clearPath(b, pr, pc, tr, tc)
	}
	return false
}
func max2(a, b int) int {
	if a > b {
		return a
	}
	return b
}
func attacked(b *Board, elim *[4]bool, tr, tc, defColor int) bool {
	for r := 0; r < 14; r++ {
		for c := 0; c < 14; c++ {
			cell := b[r][c]
			if cell == 0 {
				continue
			}
			pc := col(cell)
			if pc == defColor || elim[pc] {
				continue
			}
			if pieceAttacks(b, cell, r, c, tr, tc) {
				return true
			}
		}
	}
	return false
}
func findKing(b *Board, color int) (int, int, bool) {
	for r := 0; r < 14; r++ {
		for c := 0; c < 14; c++ {
			cell := b[r][c]
			if cell != 0 && col(cell) == color && knd(cell) == 5 {
				return r, c, true
			}
		}
	}
	return 0, 0, false
}
func kingAttacked(b *Board, elim *[4]bool, color int) bool {
	kr, kc, ok := findKing(b, color)
	if !ok {
		return true
	}
	return attacked(b, elim, kr, kc, color)
}
func checkers(b *Board, elim *[4]bool, color int) []int {
	kr, kc, ok := findKing(b, color)
	if !ok {
		return nil
	}
	var out []int
	for r := 0; r < 14; r++ {
		for c := 0; c < 14; c++ {
			cell := b[r][c]
			if cell == 0 {
				continue
			}
			pc := col(cell)
			if pc == color || elim[pc] {
				continue
			}
			if pieceAttacks(b, cell, r, c, kr, kc) {
				seen := false
				for _, x := range out {
					if x == pc {
						seen = true
					}
				}
				if !seen {
					out = append(out, pc)
				}
			}
		}
	}
	return out
}
func canLand(b *Board, elim *[4]bool, color, r, c int) bool {
	if !isPlayable(r, c) {
		return false
	}
	occ := b[r][c]
	if occ == 0 {
		return true
	}
	oc := col(occ)
	return oc != color && !(knd(occ) == 5 && !elim[oc])
}
func addSlide(b *Board, elim *[4]bool, color, fr, fc int, dirs [][2]int, out *[]Move) {
	for _, d := range dirs {
		r, c := fr+d[0], fc+d[1]
		for isPlayable(r, c) {
			occ := b[r][c]
			if occ == 0 {
				*out = append(*out, Move{fr, fc, r, c, false})
			} else {
				oc := col(occ)
				if oc != color && !(knd(occ) == 5 && !elim[oc]) {
					*out = append(*out, Move{fr, fc, r, c, false})
				}
				break
			}
			r += d[0]
			c += d[1]
		}
	}
}
func pseudoMoves(b *Board, elim *[4]bool, color int) []Move {
	out := make([]Move, 0, 64)
	for fr := 0; fr < 14; fr++ {
		for fc := 0; fc < 14; fc++ {
			cell := b[fr][fc]
			if cell == 0 || col(cell) != color {
				continue
			}
			switch knd(cell) {
			case 0: // P
				fdr, fdc := pawnFwd(color)
				r1, c1 := fr+fdr, fc+fdc
				if isPlayable(r1, c1) && b[r1][c1] == 0 {
					out = append(out, Move{fr, fc, r1, c1, pawnPromo(color, r1, c1)})
					if pawnHome(color, fr, fc) {
						r2, c2 := fr+2*fdr, fc+2*fdc
						if isPlayable(r2, c2) && b[r2][c2] == 0 {
							out = append(out, Move{fr, fc, r2, c2, false})
						}
					}
				}
				for _, cc := range pawnCaps(color) {
					r, c := fr+cc[0], fc+cc[1]
					if !isPlayable(r, c) {
						continue
					}
					occ := b[r][c]
					if occ != 0 {
						oc := col(occ)
						if oc != color && !(knd(occ) == 5 && !elim[oc]) {
							out = append(out, Move{fr, fc, r, c, pawnPromo(color, r, c)})
						}
					}
				}
			case 1: // N
				for _, d := range knight {
					r, c := fr+d[0], fc+d[1]
					if canLand(b, elim, color, r, c) {
						out = append(out, Move{fr, fc, r, c, false})
					}
				}
			case 5: // K
				for _, d := range all8 {
					r, c := fr+d[0], fc+d[1]
					if canLand(b, elim, color, r, c) {
						out = append(out, Move{fr, fc, r, c, false})
					}
				}
			case 2: // B
				addSlide(b, elim, color, fr, fc, diag[:], &out)
			case 3: // R
				addSlide(b, elim, color, fr, fc, orth[:], &out)
			case 4: // Q
				addSlide(b, elim, color, fr, fc, all8[:], &out)
			}
		}
	}
	return out
}
func applyTo(b *Board, mv Move) {
	cell := b[mv.fr][mv.fc]
	b[mv.fr][mv.fc] = 0
	color, kind := col(cell), knd(cell)
	if kind == 0 && pawnPromo(color, mv.tr, mv.tc) {
		b[mv.tr][mv.tc] = enc(color, 4) // Q
	} else {
		b[mv.tr][mv.tc] = cell
	}
}
func legalMoves(b *Board, elim *[4]bool, color int) []Move {
	out := make([]Move, 0, 48)
	for _, mv := range pseudoMoves(b, elim, color) {
		nb := *b
		applyTo(&nb, mv)
		if !kingAttacked(&nb, elim, color) {
			out = append(out, mv)
		}
	}
	return out
}

type State struct {
	board        Board
	eliminated   [4]bool
	scores       [4]int
	idx          int
	current      int // -1 = none
	currentLegal []Move
	lastMover    int // -1 = none
	over         bool
	noProgress   int
	repeats      map[string]int
}

func newBoard() Board {
	red := [8]int{3, 1, 2, 4, 5, 2, 1, 3}
	yellow := [8]int{3, 1, 2, 5, 4, 2, 1, 3}
	blue := red
	green := yellow
	var b Board
	for i := 0; i < 8; i++ {
		col := 3 + i
		row := 3 + i
		b[13][col] = enc(0, red[i])
		b[12][col] = enc(0, 0)
		b[0][col] = enc(2, yellow[i])
		b[1][col] = enc(2, 0)
		b[row][0] = enc(1, blue[i])
		b[row][1] = enc(1, 0)
		b[row][13] = enc(3, green[i])
		b[row][12] = enc(3, 0)
	}
	return b
}
func newGame() *State {
	s := &State{board: newBoard(), idx: 3, current: -1, lastMover: -1, repeats: map[string]int{}}
	s.advanceTurn()
	return s
}
func (s *State) activeCount() int {
	n := 0
	for i := 0; i < 4; i++ {
		if !s.eliminated[i] {
			n++
		}
	}
	return n
}
func (s *State) onlyKingsLeft() bool {
	for r := 0; r < 14; r++ {
		for c := 0; c < 14; c++ {
			cell := s.board[r][c]
			if cell != 0 && !s.eliminated[col(cell)] && knd(cell) != 5 {
				return false
			}
		}
	}
	return true
}
func (s *State) repeatKey(cur int) string {
	buf := make([]byte, 0, 196+8)
	for r := 0; r < 14; r++ {
		for c := 0; c < 14; c++ {
			buf = append(buf, s.board[r][c])
		}
	}
	buf = append(buf, byte(cur))
	for i := 0; i < 4; i++ {
		if s.eliminated[i] {
			buf = append(buf, 1)
		} else {
			buf = append(buf, 0)
		}
	}
	return string(buf)
}
func (s *State) isDraw(cur int) bool {
	if s.noProgress >= 100 {
		return true
	}
	if s.onlyKingsLeft() {
		return true
	}
	k := s.repeatKey(cur)
	s.repeats[k]++
	return s.repeats[k] >= 3
}
func (s *State) advanceTurn() {
	for {
		if s.activeCount() <= 1 {
			s.current = -1
			s.over = true
			return
		}
		s.idx = (s.idx + 1) % 4
		c := s.idx
		if s.eliminated[c] {
			continue
		}
		legal := legalMoves(&s.board, &s.eliminated, c)
		if len(legal) == 0 {
			chk := checkers(&s.board, &s.eliminated, c)
			s.eliminated[c] = true
			if len(chk) > 0 {
				s.scores[chk[0]] += 20
			} else if s.lastMover >= 0 && s.lastMover != c && !s.eliminated[s.lastMover] {
				s.scores[s.lastMover] += 20
			}
			continue
		}
		s.current = c
		s.currentLegal = legal
		if s.isDraw(c) {
			s.current = -1
			s.over = true
		}
		return
	}
}
func (s *State) makeMove(mv Move) {
	cell := s.board[mv.fr][mv.fc]
	pcolor, pkind := col(cell), knd(cell)
	cap := s.board[mv.tr][mv.tc]
	if cap != 0 {
		cc := col(cap)
		if !s.eliminated[cc] {
			s.scores[pcolor] += value[knd(cap)]
		}
	}
	if cap != 0 || pkind == 0 {
		s.noProgress = 0
	} else {
		s.noProgress++
	}
	applyTo(&s.board, mv)
	s.lastMover = pcolor
	s.advanceTurn()
}

type Rng struct{ s uint64 }

func newRng(seed uint64) Rng { return Rng{seed + 0x9E3779B97F4A7C15} }
func (r *Rng) next() uint64 {
	r.s += 0x9E3779B97F4A7C15
	z := r.s
	z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
	z = (z ^ (z >> 27)) * 0x94D049BB133111EB
	return z ^ (z >> 31)
}
func (r *Rng) below(n int) int { return int(r.next() % uint64(n)) }

func main() {
	games := 2000
	maxSteps := 250
	if len(os.Args) > 1 {
		if v, err := strconv.Atoi(os.Args[1]); err == nil {
			games = v
		}
	}
	if len(os.Args) > 2 {
		if v, err := strconv.Atoi(os.Args[2]); err == nil {
			maxSteps = v
		}
	}
	t := time.Now()
	var positions, finished uint64
	for g := 0; g < games; g++ {
		rng := newRng(uint64(g)*0x9E3779B97F4A7C15 ^ 0xBEEF)
		st := newGame()
		steps := 0
		for !st.over && steps < maxSteps {
			positions++
			m := st.currentLegal
			st.makeMove(m[rng.below(len(m))])
			steps++
		}
		if st.over {
			finished++
		}
	}
	dt := time.Since(t).Seconds()
	fmt.Fprintf(os.Stderr, "go   engine  games=%d positions=%d finished=%d time=%.3fs  => %.0f pos/s\n",
		games, positions, finished, dt, float64(positions)/dt)
}
