// Language shootout — Go. Run: go run bench/main.go rollout [steps]
//                            go run bench/main.go mlp [iters]
// Identical algorithm + LCG to bench.rs/.js/.py => checksum must match.
package main

import (
	"fmt"
	"os"
	"strconv"
	"time"
)

const (
	N     = 14
	BOARD = N * N
	F     = 52
	H     = 128
)

type lcg struct{ s uint64 }

func (l *lcg) nextU32() uint32 {
	l.s = l.s*6364136223846793005 + 1442695040888963407
	return uint32(l.s >> 33)
}
func (l *lcg) unit() float64 {
	return float64(l.nextU32())/4294967296.0*2.0 - 1.0
}

func rollout(steps int) uint64 {
	dirs := [8][2]int{{-1, -1}, {-1, 0}, {-1, 1}, {0, -1}, {0, 1}, {1, -1}, {1, 0}, {1, 1}}
	var board [BOARD]int32
	for i := 0; i < BOARD; i++ {
		if i%5 == 0 {
			board[i] = 1
		}
	}
	rng := lcg{0x12345678}
	moves := make([]int32, 0, 2048)
	var total uint64
	for st := 0; st < steps; st++ {
		moves = moves[:0]
		for idx := 0; idx < BOARD; idx++ {
			if board[idx] == 0 {
				continue
			}
			r := idx / N
			c := idx % N
			for d := 0; d < 8; d++ {
				dr := dirs[d][0]
				dc := dirs[d][1]
				nr := r + dr
				nc := c + dc
				for nr >= 0 && nr < N && nc >= 0 && nc < N {
					ni := nr*N + nc
					if board[ni] != 0 {
						moves = append(moves, int32(idx<<8|ni))
						break
					}
					moves = append(moves, int32(idx<<8|ni))
					nr += dr
					nc += dc
				}
			}
		}
		total += uint64(len(moves))
		if len(moves) > 0 {
			m := moves[int(rng.nextU32())%len(moves)]
			from := int(m >> 8)
			to := int(m & 0xFF)
			board[to], board[from] = board[from], board[to]
		}
	}
	return total
}

func mlp(iters int) float64 {
	rng := lcg{0x9E3779B9}
	w1 := make([]float64, H*F)
	b1 := make([]float64, H)
	w2 := make([]float64, H*H)
	b2 := make([]float64, H)
	w3 := make([]float64, 4*H)
	b3 := make([]float64, 4)
	fill := func(a []float64, scale float64) {
		for i := range a {
			a[i] = rng.unit() * scale
		}
	}
	fill(w1, 0.1)
	fill(b1, 0.1)
	fill(w2, 0.1)
	fill(b2, 0.1)
	fill(w3, 0.1)
	fill(b3, 0.1)
	x := make([]float64, F)
	for i := range x {
		x[i] = rng.unit()
	}
	h1 := make([]float64, H)
	h2 := make([]float64, H)
	acc := 0.0
	for n := 0; n < iters; n++ {
		for o := 0; o < H; o++ {
			s := b1[o]
			row := o * F
			for j := 0; j < F; j++ {
				s += w1[row+j] * x[j]
			}
			if s < 0 {
				s = 0
			}
			h1[o] = s
		}
		for o := 0; o < H; o++ {
			s := b2[o]
			row := o * H
			for j := 0; j < H; j++ {
				s += w2[row+j] * h1[j]
			}
			if s < 0 {
				s = 0
			}
			h2[o] = s
		}
		var out [4]float64
		for o := 0; o < 4; o++ {
			s := b3[o]
			row := o * H
			for j := 0; j < H; j++ {
				s += w3[row+j] * h2[j]
			}
			out[o] = s
		}
		acc += out[0] + out[1] + out[2] + out[3]
		x[n%F] += 1e-6 * (out[0] - out[1])
	}
	return acc
}

func main() {
	which := "rollout"
	if len(os.Args) > 1 {
		which = os.Args[1]
	}
	arg := func(def int) int {
		if len(os.Args) > 2 {
			if v, err := strconv.Atoi(os.Args[2]); err == nil {
				return v
			}
		}
		return def
	}
	t := time.Now()
	switch which {
	case "rollout":
		steps := arg(50000)
		chk := rollout(steps)
		fmt.Fprintf(os.Stderr, "go   rollout  steps=%d checksum=%d time=%.3fs\n", steps, chk, time.Since(t).Seconds())
	case "mlp":
		iters := arg(200000)
		chk := mlp(iters)
		fmt.Fprintf(os.Stderr, "go   mlp      iters=%d checksum=%.6f time=%.3fs\n", iters, chk, time.Since(t).Seconds())
	default:
		fmt.Fprintln(os.Stderr, "usage: main.go rollout|mlp [count]")
	}
}
