// Real 4PC engine throughput in C — faithful port of fpc-core. Same splitmix64
// RNG + identical move ordering as throughput.rs, so a correct port reproduces
// Rust's exact positions=499967 finished=1 for `2000 250`.
//   cc -O3 -o bench_engine_c bench/engine.c && ./bench_engine_c 2000 250
//
// cell: 0=empty; else 1 + color*6 + kind. color R0 B1 Y2 G3; kind P0 N1 B2 R3 Q4 K5.

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <time.h>

typedef int8_t Board[14][14];
typedef struct { int fr, fc, tr, tc, promo; } Move;

static inline int8_t enc(int color, int kind) { return (int8_t)(1 + color * 6 + kind); }
static inline int col_of(int8_t c) { return (c - 1) / 6; }
static inline int knd_of(int8_t c) { return (c - 1) % 6; }

static const int VALUE[6] = {1, 3, 5, 5, 9, 20};
static const int ORTH[4][2]   = {{-1,0},{1,0},{0,-1},{0,1}};
static const int DIAG[4][2]   = {{-1,-1},{-1,1},{1,-1},{1,1}};
static const int ALL8[8][2]   = {{-1,0},{1,0},{0,-1},{0,1},{-1,-1},{-1,1},{1,-1},{1,1}};
static const int KNIGHT[8][2] = {{-2,-1},{-2,1},{2,-1},{2,1},{-1,-2},{1,-2},{-1,2},{1,2}};

static void pawn_fwd(int c, int *dr, int *dc) {
    switch (c) { case 0:*dr=-1;*dc=0;break; case 1:*dr=0;*dc=1;break;
                 case 2:*dr=1;*dc=0;break; default:*dr=0;*dc=-1; }
}
static void pawn_caps(int c, int caps[2][2]) {
    switch (c) {
        case 0: caps[0][0]=-1;caps[0][1]=-1;caps[1][0]=-1;caps[1][1]=1;break;
        case 1: caps[0][0]=-1;caps[0][1]=1; caps[1][0]=1; caps[1][1]=1;break;
        case 2: caps[0][0]=1; caps[0][1]=-1;caps[1][0]=1; caps[1][1]=1;break;
        default:caps[0][0]=-1;caps[0][1]=-1;caps[1][0]=1; caps[1][1]=-1;
    }
}
static inline int is_playable(int r, int c) {
    if (r < 0 || r > 13 || c < 0 || c > 13) return 0;
    return !((r < 3 || r > 10) && (c < 3 || c > 10));
}
static inline int pawn_home(int color, int r, int c) {
    return (color==0&&r==12)||(color==2&&r==1)||(color==1&&c==1)||(color==3&&c==12);
}
static inline int pawn_promo(int color, int r, int c) {
    return (color==0&&r==6)||(color==2&&r==7)||(color==1&&c==7)||(color==3&&c==6);
}
static inline int iabs(int x){ return x<0?-x:x; }
static inline int isign(int x){ return x>0?1:(x<0?-1:0); }
static inline int imax(int a,int b){ return a>b?a:b; }

static int clear_path(Board b, int pr, int pc, int tr, int tc) {
    int sr = isign(tr-pr), sc = isign(tc-pc);
    int r = pr+sr, c = pc+sc;
    while (r != tr || c != tc) {
        if (!is_playable(r,c) || b[r][c] != 0) return 0;
        r += sr; c += sc;
    }
    return 1;
}
static int piece_attacks(Board b, int8_t cell, int pr, int pc, int tr, int tc) {
    int dr = tr-pr, dc = tc-pc;
    if (dr==0 && dc==0) return 0;
    int color = col_of(cell), kind = knd_of(cell);
    switch (kind) {
        case 5: return imax(iabs(dr),iabs(dc))==1;
        case 1: return (iabs(dr)==1&&iabs(dc)==2)||(iabs(dr)==2&&iabs(dc)==1);
        case 0: { int caps[2][2]; pawn_caps(color,caps);
                  return (caps[0][0]==dr&&caps[0][1]==dc)||(caps[1][0]==dr&&caps[1][1]==dc); }
        case 2: return iabs(dr)==iabs(dc) && clear_path(b,pr,pc,tr,tc);
        case 3: return (dr==0||dc==0) && clear_path(b,pr,pc,tr,tc);
        case 4: return (dr==0||dc==0||iabs(dr)==iabs(dc)) && clear_path(b,pr,pc,tr,tc);
    }
    return 0;
}
static int attacked(Board b, const int elim[4], int tr, int tc, int defColor) {
    for (int r=0;r<14;r++) for (int c=0;c<14;c++) {
        int8_t cell = b[r][c];
        if (!cell) continue;
        int pc = col_of(cell);
        if (pc==defColor || elim[pc]) continue;
        if (piece_attacks(b,cell,r,c,tr,tc)) return 1;
    }
    return 0;
}
static int find_king(Board b, int color, int *kr, int *kc) {
    for (int r=0;r<14;r++) for (int c=0;c<14;c++) {
        int8_t cell=b[r][c];
        if (cell && col_of(cell)==color && knd_of(cell)==5) { *kr=r;*kc=c; return 1; }
    }
    return 0;
}
static int king_attacked(Board b, const int elim[4], int color) {
    int kr,kc;
    if (!find_king(b,color,&kr,&kc)) return 1;
    return attacked(b,elim,kr,kc,color);
}
static int can_land(Board b, const int elim[4], int color, int r, int c) {
    if (!is_playable(r,c)) return 0;
    int8_t occ=b[r][c];
    if (!occ) return 1;
    int oc=col_of(occ);
    return oc!=color && !(knd_of(occ)==5 && !elim[oc]);
}
static void add_slide(Board b, const int elim[4], int color, int fr, int fc,
                      const int dirs[][2], int ndir, Move *out, int *n) {
    for (int d=0; d<ndir; d++) {
        int r=fr+dirs[d][0], c=fc+dirs[d][1];
        while (is_playable(r,c)) {
            int8_t occ=b[r][c];
            if (!occ) { out[(*n)++] = (Move){fr,fc,r,c,0}; }
            else {
                int oc=col_of(occ);
                if (oc!=color && !(knd_of(occ)==5 && !elim[oc])) out[(*n)++]=(Move){fr,fc,r,c,0};
                break;
            }
            r+=dirs[d][0]; c+=dirs[d][1];
        }
    }
}
static int pseudo_moves(Board b, const int elim[4], int color, Move *out) {
    int n=0;
    for (int fr=0;fr<14;fr++) for (int fc=0;fc<14;fc++) {
        int8_t cell=b[fr][fc];
        if (!cell || col_of(cell)!=color) continue;
        switch (knd_of(cell)) {
            case 0: { // P
                int fdr,fdc; pawn_fwd(color,&fdr,&fdc);
                int r1=fr+fdr,c1=fc+fdc;
                if (is_playable(r1,c1) && b[r1][c1]==0) {
                    out[n++]=(Move){fr,fc,r1,c1,pawn_promo(color,r1,c1)};
                    if (pawn_home(color,fr,fc)) {
                        int r2=fr+2*fdr,c2=fc+2*fdc;
                        if (is_playable(r2,c2) && b[r2][c2]==0) out[n++]=(Move){fr,fc,r2,c2,0};
                    }
                }
                int caps[2][2]; pawn_caps(color,caps);
                for (int k=0;k<2;k++) {
                    int r=fr+caps[k][0],c=fc+caps[k][1];
                    if (!is_playable(r,c)) continue;
                    int8_t occ=b[r][c];
                    if (occ) { int oc=col_of(occ);
                        if (oc!=color && !(knd_of(occ)==5 && !elim[oc]))
                            out[n++]=(Move){fr,fc,r,c,pawn_promo(color,r,c)};
                    }
                }
                break;
            }
            case 1: for (int k=0;k<8;k++){int r=fr+KNIGHT[k][0],c=fc+KNIGHT[k][1];
                        if (can_land(b,elim,color,r,c)) out[n++]=(Move){fr,fc,r,c,0};} break;
            case 5: for (int k=0;k<8;k++){int r=fr+ALL8[k][0],c=fc+ALL8[k][1];
                        if (can_land(b,elim,color,r,c)) out[n++]=(Move){fr,fc,r,c,0};} break;
            case 2: add_slide(b,elim,color,fr,fc,DIAG,4,out,&n); break;
            case 3: add_slide(b,elim,color,fr,fc,ORTH,4,out,&n); break;
            case 4: add_slide(b,elim,color,fr,fc,ALL8,8,out,&n); break;
        }
    }
    return n;
}
static void apply_to(Board b, Move mv) {
    int8_t cell=b[mv.fr][mv.fc];
    b[mv.fr][mv.fc]=0;
    int color=col_of(cell), kind=knd_of(cell);
    if (kind==0 && pawn_promo(color,mv.tr,mv.tc)) b[mv.tr][mv.tc]=enc(color,4);
    else b[mv.tr][mv.tc]=cell;
}
static int legal_moves(Board b, const int elim[4], int color, Move *out) {
    Move pseudo[300];
    int np = pseudo_moves(b,elim,color,pseudo);
    int n=0;
    for (int i=0;i<np;i++) {
        Board nb; memcpy(nb,b,sizeof(Board));
        apply_to(nb,pseudo[i]);
        if (!king_attacked(nb,elim,color)) out[n++]=pseudo[i];
    }
    return n;
}

// ---- threefold repetition: FNV-1a 64-bit of (board,current,elim) -> count ----
#define RT_SIZE 1024
static uint64_t rt_key[RT_SIZE];
static int      rt_cnt[RT_SIZE];
static void rt_reset(void){ memset(rt_key,0,sizeof(rt_key)); memset(rt_cnt,0,sizeof(rt_cnt)); }
static int rt_bump(uint64_t h){ // returns new count
    if (h==0) h=1;
    uint64_t i=h & (RT_SIZE-1);
    while (rt_key[i]!=0 && rt_key[i]!=h) i=(i+1)&(RT_SIZE-1);
    rt_key[i]=h; return ++rt_cnt[i];
}
static uint64_t repeat_hash(Board b, int cur, const int elim[4]) {
    uint64_t h=1469598103934665603ULL;
    for (int r=0;r<14;r++) for (int c=0;c<14;c++){ h^=(uint8_t)b[r][c]; h*=1099511628211ULL; }
    h^=(uint8_t)cur; h*=1099511628211ULL;
    for (int i=0;i<4;i++){ h^=(uint8_t)elim[i]; h*=1099511628211ULL; }
    return h;
}

typedef struct {
    Board board;
    int elim[4];
    int scores[4];
    int idx;
    int current;       // -1 = none
    Move legal[256];
    int nlegal;
    int lastMover;     // -1 = none
    int over;
    int noProgress;
} State;

static int active_count(State *s){ int n=0; for(int i=0;i<4;i++) if(!s->elim[i]) n++; return n; }
static int only_kings_left(State *s){
    for (int r=0;r<14;r++) for (int c=0;c<14;c++){
        int8_t cell=s->board[r][c];
        if (cell && !s->elim[col_of(cell)] && knd_of(cell)!=5) return 0;
    }
    return 1;
}
static int is_draw(State *s, int cur){
    if (s->noProgress>=100) return 1;
    if (only_kings_left(s)) return 1;
    return rt_bump(repeat_hash(s->board,cur,s->elim)) >= 3;
}
static void advance_turn(State *s){
    for (;;) {
        if (active_count(s)<=1){ s->current=-1; s->over=1; return; }
        s->idx=(s->idx+1)%4;
        int c=s->idx;
        if (s->elim[c]) continue;
        int n=legal_moves(s->board,s->elim,c,s->legal);
        if (n==0){
            int kr,kc; (void)kr;(void)kc;
            // checkers: any active enemy attacking c's king (first one credited)
            int credited=-1;
            if (find_king(s->board,c,&kr,&kc)){
                for (int r=0;r<14 && credited<0;r++) for (int cc=0;cc<14;cc++){
                    int8_t cell=s->board[r][cc];
                    if (!cell) continue;
                    int pc=col_of(cell);
                    if (pc==c || s->elim[pc]) continue;
                    if (piece_attacks(s->board,cell,r,cc,kr,kc)){ credited=pc; break; }
                }
            }
            s->elim[c]=1;
            if (credited>=0) s->scores[credited]+=20;
            else if (s->lastMover>=0 && s->lastMover!=c && !s->elim[s->lastMover]) s->scores[s->lastMover]+=20;
            continue;
        }
        s->current=c; s->nlegal=n;
        if (is_draw(s,c)){ s->current=-1; s->over=1; }
        return;
    }
}
static void new_board(Board b){
    memset(b,0,sizeof(Board));
    const int red[8]={3,1,2,4,5,2,1,3};
    const int yellow[8]={3,1,2,5,4,2,1,3};
    for (int i=0;i<8;i++){
        int col=3+i, row=3+i;
        b[13][col]=enc(0,red[i]);   b[12][col]=enc(0,0);
        b[0][col]=enc(2,yellow[i]); b[1][col]=enc(2,0);
        b[row][0]=enc(1,red[i]);    b[row][1]=enc(1,0);     // blue == red layout
        b[row][13]=enc(3,yellow[i]);b[row][12]=enc(3,0);    // green == yellow layout
    }
}
static void new_game(State *s){
    memset(s,0,sizeof(State));
    new_board(s->board);
    s->idx=3; s->current=-1; s->lastMover=-1;
    rt_reset();
    advance_turn(s);
}
static void make_move(State *s, Move mv){
    int8_t cell=s->board[mv.fr][mv.fc];
    int pcolor=col_of(cell), pkind=knd_of(cell);
    int8_t cap=s->board[mv.tr][mv.tc];
    if (cap){ int cc=col_of(cap); if (!s->elim[cc]) s->scores[pcolor]+=VALUE[knd_of(cap)]; }
    s->noProgress = (cap || pkind==0) ? 0 : s->noProgress+1;
    apply_to(s->board,mv);
    s->lastMover=pcolor;
    advance_turn(s);
}

// splitmix64, matching fpc_agents::Rng
typedef struct { uint64_t s; } Rng;
static Rng rng_new(uint64_t seed){ Rng r; r.s=seed+0x9E3779B97F4A7C15ULL; return r; }
static uint64_t rng_next(Rng *r){
    r->s += 0x9E3779B97F4A7C15ULL;
    uint64_t z=r->s;
    z=(z^(z>>30))*0xBF58476D1CE4E5B9ULL;
    z=(z^(z>>27))*0x94D049BB133111EBULL;
    return z^(z>>31);
}
static int rng_below(Rng *r, int n){ return (int)(rng_next(r) % (uint64_t)n); }

int main(int argc, char **argv){
    int games = argc>1 ? atoi(argv[1]) : 2000;
    int maxSteps = argc>2 ? atoi(argv[2]) : 250;
    struct timespec t0,t1; clock_gettime(CLOCK_MONOTONIC,&t0);
    uint64_t positions=0, finished=0;
    State *s = malloc(sizeof(State));
    for (int g=0; g<games; g++){
        Rng rng = rng_new((uint64_t)g * 0x9E3779B97F4A7C15ULL ^ 0xBEEFULL);
        new_game(s);
        int steps=0;
        while (!s->over && steps<maxSteps){
            positions++;
            make_move(s, s->legal[rng_below(&rng, s->nlegal)]);
            steps++;
        }
        if (s->over) finished++;
    }
    free(s);
    clock_gettime(CLOCK_MONOTONIC,&t1);
    double dt=(t1.tv_sec-t0.tv_sec)+(t1.tv_nsec-t0.tv_nsec)/1e9;
    fprintf(stderr,"c    engine  games=%d positions=%llu finished=%llu time=%.3fs  => %.0f pos/s\n",
            games, (unsigned long long)positions, (unsigned long long)finished, dt, positions/dt);
    return 0;
}
