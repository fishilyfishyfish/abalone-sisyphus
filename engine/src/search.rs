//! 알파베타 탐색: 네가맥스 PVS, 반복 심화, 조브리스트 치환표, 수 정렬, 밀어내기 정지 탐색.
//!
//! 설명 층을 위한 성질: 루트 점수는 주 변화(PV) 끝 국면의 정적 평가값과 정확히 같다
//! (승패·반복·수 제한 점수가 아닐 때). 그래서 PV 노드에서는 치환표로 잘라내지 않고,
//! 정지 탐색도 PV를 이어서 기록한다.

use crate::board::*;
use crate::eval::{self, Weights, WIN, WIN_BOUND};
use crate::game::Game;

pub const MAX_PLY: usize = 64;
pub const INF: i32 = WIN + 10_000;
/// 정지 탐색에서 연달아 볼 밀어내기 수의 최대 개수.
pub const QS_DEPTH: i32 = 8;
/// 정적 평가 캐시 칸 수 = 2^ECACHE_BITS (칸당 16바이트, 1MB).
const ECACHE_BITS: u32 = 16;
/// 시계를 확인하는 노드 간격.
const CLOCK_EVERY: u64 = 1024;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// 반복 심화 최대 깊이
    pub max_depth: u32,
    /// 제한 시간(ms). 0이면 깊이만 본다. 깊이 1은 시간과 무관하게 끝까지 본다.
    pub time_ms: f64,
    /// 루트의 모든 수를 전체 창으로 탐색해 정확한 점수를 남긴다 (느리지만 수 비교·난이도 조절용).
    pub exact_root: bool,
    /// PV가 아닌 노드에서 널 무브 가지치기와 늦은 수 줄이기(LMR)를 쓴다. 같은 시간에 더 깊이 보지만
    /// 점수가 그 깊이의 미니맥스 값과 같다는 보장은 없어진다(루트 점수 = PV 끝 정적 평가는 그대로).
    pub prune: bool,
}

impl Limits {
    pub fn depth(d: u32) -> Limits {
        Limits {
            max_depth: d,
            time_ms: 0.0,
            exact_root: false,
            prune: false,
        }
    }

    /// 시간 제한 탐색 (깊이 제한 없음, 가지치기 사용). 컴퓨터가 수를 고를 때 쓴다.
    pub fn time(ms: f64) -> Limits {
        Limits {
            max_depth: 64,
            time_ms: ms,
            exact_root: false,
            prune: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    pub best: Option<Move>,
    /// 둘 차례 관점 점수
    pub score: i32,
    /// 끝까지 마친 깊이
    pub depth: u32,
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub elapsed_ms: f64,
    /// 마지막으로 끝낸 깊이의 루트 수별 점수. exact_root가 아니면 최선 수 외에는 상한값이다.
    pub root_scores: Vec<(Move, i32)>,
}

#[derive(Clone, Copy, Default)]
struct Entry {
    key: u64,
    score: i32,
    mv: u16,
    depth: i8,
    flag: u8,
}

/// 수 정렬 점수: 치환표 수.
const TT_KEY: i32 = 1 << 30;
/// 수 정렬 점수: 첫 번째 킬러. 두 번째 킬러는 1 작고, 히스토리는 그보다 작다.
const KILLER_KEY: i32 = 1 << 27;

const EXACT: u8 = 1;
const LOWER: u8 = 2;
const UPPER: u8 = 3;

fn pack(m: Move) -> u16 {
    0x8000
        | m.tail as u16
        | ((m.n as u16 - 1) << 7)
        | ((m.axis as u16) << 9)
        | ((m.dir as u16) << 11)
}

fn unpack(v: u16) -> Option<Move> {
    if v & 0x8000 == 0 {
        return None;
    }
    Some(Move {
        tail: (v & 0x7f) as u8,
        n: ((v >> 7) & 3) as u8 + 1,
        axis: ((v >> 9) & 3) as u8,
        dir: ((v >> 11) & 7) as u8,
    })
}

/// 승패 점수는 치환표에 "그 노드로부터의 거리"로 저장한다.
fn to_tt(score: i32, ply: usize) -> i32 {
    if score >= WIN_BOUND {
        score + ply as i32
    } else if score <= -WIN_BOUND {
        score - ply as i32
    } else {
        score
    }
}

fn from_tt(score: i32, ply: usize) -> i32 {
    if score >= WIN_BOUND {
        score - ply as i32
    } else if score <= -WIN_BOUND {
        score + ply as i32
    } else {
        score
    }
}

pub struct Searcher {
    /// 바꾸면 `clear()`로 평가 캐시를 비워야 한다.
    pub weights: Weights,
    /// 평가에 보정 신경망을 더한다(`eval::evaluate_nn`). 바꾸면 `clear()`로 평가 캐시를 비워야 한다.
    pub nn: bool,
    tt: Vec<Entry>,
    killers: [[u16; 2]; MAX_PLY],
    history: Vec<i32>,
    pv: Box<[[Move; MAX_PLY]; MAX_PLY]>,
    pv_len: [usize; MAX_PLY],
    bufs: Vec<Vec<Move>>,
    /// 정적 평가 캐시 (키, 점수). 같은 국면을 반복 심화·치환으로 여러 번 평가하는 것을 줄인다
    ecache: Vec<(u64, i32)>,
    /// 수 정렬 점수 (bufs와 같은 순서). 할당을 줄이려고 깊이마다 재사용한다
    keys: Vec<Vec<i32>>,
    /// 대국 기록 + 현재 탐색 경로의 해시 (반복 판정)
    path: Vec<u64>,
    base_ply: usize,
    move_limit: u32,
    prune: bool,
    nodes: u64,
    stop: bool,
    can_stop: bool,
    deadline: f64,
    clock: Box<dyn Fn() -> f64>,
}

const NO_MOVE: Move = Move {
    tail: 0,
    n: 1,
    axis: 0,
    dir: 0,
};

impl Searcher {
    /// tt_bits: 치환표 칸 수 = 2^tt_bits (칸당 16바이트). clock: ms 단위 현재 시각.
    pub fn new(tt_bits: u32, weights: Weights, clock: Box<dyn Fn() -> f64>) -> Searcher {
        Searcher {
            weights,
            nn: false,
            tt: vec![Entry::default(); 1 << tt_bits],
            killers: [[0; 2]; MAX_PLY],
            history: vec![0; 121 * 6 * 3],
            pv: Box::new([[NO_MOVE; MAX_PLY]; MAX_PLY]),
            pv_len: [0; MAX_PLY],
            bufs: (0..MAX_PLY).map(|_| Vec::with_capacity(128)).collect(),
            keys: (0..MAX_PLY).map(|_| Vec::with_capacity(128)).collect(),
            ecache: vec![(0, 0); 1 << ECACHE_BITS],
            path: Vec::new(),
            base_ply: 0,
            move_limit: 0,
            prune: false,
            nodes: 0,
            stop: false,
            can_stop: false,
            deadline: f64::INFINITY,
            clock,
        }
    }

    /// 표준 라이브러리 시계를 쓰는 생성자 (wasm32 제외).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_std_clock(tt_bits: u32, weights: Weights) -> Searcher {
        let t0 = std::time::Instant::now();
        Searcher::new(
            tt_bits,
            weights,
            Box::new(move || t0.elapsed().as_secs_f64() * 1000.0),
        )
    }

    /// 정적 평가(둘 차례 관점). 해시는 밀어낸 수를 담지 않으므로 키에 섞는다.
    fn eval(&mut self, pos: &Position) -> i32 {
        let key = pos.hash
            ^ (pos.pushed[0] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (pos.pushed[1] as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            | 1;
        let slot = &mut self.ecache[(key as usize) & ((1 << ECACHE_BITS) - 1)];
        if slot.0 == key {
            return slot.1;
        }
        let v = if self.nn {
            eval::evaluate_nn(pos, &self.weights)
        } else {
            eval::evaluate(pos, &self.weights)
        };
        *slot = (key, v);
        v
    }

    /// 치환표와 수 정렬 기록을 비운다.
    pub fn clear(&mut self) {
        self.ecache.iter_mut().for_each(|e| *e = (0, 0));
        self.tt.iter_mut().for_each(|e| *e = Entry::default());
        self.killers = [[0; 2]; MAX_PLY];
        self.history.iter_mut().for_each(|h| *h = 0);
    }

    pub fn search(&mut self, game: &Game, lim: Limits) -> SearchResult {
        let start = (self.clock)();
        self.deadline = if lim.time_ms > 0.0 {
            start + lim.time_ms
        } else {
            f64::INFINITY
        };
        self.nodes = 0;
        self.stop = false;
        self.path.clear();
        self.path.extend_from_slice(game.hashes());
        self.base_ply = game.ply();
        self.move_limit = game.config.move_limit;
        self.prune = lim.prune;
        self.history.iter_mut().for_each(|h| *h /= 8);

        let mut res = SearchResult::default();
        let mut pos = game.pos;
        let mut root: Vec<Move> = game.legal_moves();
        if root.is_empty() {
            return res;
        }
        for depth in 1..=lim.max_depth.max(1) {
            self.can_stop = depth > 1;
            let (score, scores) = self.root(&mut pos, depth as i32, &mut root, lim.exact_root);
            if self.stop {
                break;
            }
            res.score = score;
            res.depth = depth;
            res.pv = self.pv[0][..self.pv_len[0]].to_vec();
            res.best = res.pv.first().copied();
            res.root_scores = scores;
            if score.abs() >= WIN_BOUND || self.deadline_near(start) {
                break;
            }
        }
        res.nodes = self.nodes;
        res.elapsed_ms = (self.clock)() - start;
        res
    }

    /// 다음 깊이는 보통 지금까지의 몇 배가 걸리므로, 남은 시간이 부족하면 시작하지 않는다.
    fn deadline_near(&self, start: f64) -> bool {
        if !self.deadline.is_finite() {
            return false;
        }
        let now = (self.clock)();
        now - start > (self.deadline - start) * 0.5
    }

    fn root(
        &mut self,
        pos: &mut Position,
        depth: i32,
        moves: &mut [Move],
        exact: bool,
    ) -> (i32, Vec<(Move, i32)>) {
        self.pv_len[0] = 0;
        let mut alpha = -INF;
        let mut best = -INF;
        let mut scores = Vec::with_capacity(moves.len());
        for (i, &m) in moves.iter().enumerate() {
            let u = pos.make_move(m);
            let score = if exact || i == 0 {
                -self.negamax(
                    pos,
                    depth - 1,
                    1,
                    -INF,
                    if exact { INF } else { -alpha },
                    true,
                    true,
                )
            } else {
                let s = -self.negamax(pos, depth - 1, 1, -alpha - 1, -alpha, false, true);
                if s > alpha && !self.stop {
                    -self.negamax(pos, depth - 1, 1, -INF, -alpha, true, true)
                } else {
                    s
                }
            };
            pos.unmake_move(u);
            if self.stop {
                break;
            }
            scores.push((m, score));
            if score > best {
                best = score;
                self.set_pv(0, m);
                if score > alpha {
                    alpha = score;
                }
            }
        }
        // 다음 깊이는 이번 점수 순으로 본다 (최선 수가 맨 앞)
        if !self.stop {
            let mut order: Vec<(Move, i32)> = scores.clone();
            order.sort_by_key(|x| std::cmp::Reverse(x.1));
            for (slot, (m, _)) in moves.iter_mut().zip(order) {
                *slot = m;
            }
            self.store(pos.hash, depth, best, EXACT, Some(moves[0]), 0);
        }
        (best, scores)
    }

    fn set_pv(&mut self, ply: usize, m: Move) {
        self.pv[ply][ply] = m;
        let next = if ply + 1 < MAX_PLY {
            self.pv_len[ply + 1]
        } else {
            ply + 1
        };
        for i in ply + 1..next {
            self.pv[ply][i] = self.pv[ply + 1][i];
        }
        self.pv_len[ply] = next.max(ply + 1);
    }

    fn tick(&mut self) -> bool {
        self.nodes += 1;
        if self.can_stop
            && self.nodes.is_multiple_of(CLOCK_EVERY)
            && (self.clock)() >= self.deadline
        {
            self.stop = true;
        }
        self.stop
    }

    /// 승패·수 제한으로 끝난 국면이면 둘 차례 관점 점수.
    fn terminal(&self, pos: &Position, ply: usize) -> Option<i32> {
        if pos.pushed_off_by(pos.side.other()) >= WIN_PUSHED {
            return Some(-(WIN - ply as i32));
        }
        if self.move_limit > 0 && (self.base_ply + ply) as u32 >= self.move_limit {
            return Some(self.adjudicate(pos, ply));
        }
        None
    }

    /// 수 제한·합법수 없음: 더 많이 밀어낸 쪽 승, 같으면 무승부.
    fn adjudicate(&self, pos: &Position, ply: usize) -> i32 {
        let me = pos.pushed_off_by(pos.side) as i32;
        let them = pos.pushed_off_by(pos.side.other()) as i32;
        match me.cmp(&them) {
            std::cmp::Ordering::Greater => WIN - ply as i32,
            std::cmp::Ordering::Less => -(WIN - ply as i32),
            std::cmp::Ordering::Equal => 0,
        }
    }

    fn is_repetition(&self, h: u64) -> bool {
        // path 마지막은 부모 국면. 같은 차례의 국면만 같을 수 있으니 2칸씩 거슬러 본다.
        let n = self.path.len();
        let mut i = n as isize - 2;
        while i >= 0 {
            if self.path[i as usize] == h {
                return true;
            }
            i -= 2;
        }
        false
    }

    #[allow(clippy::too_many_arguments)] // 탐색 상태를 구조체로 묶으면 재귀 호출마다 더 번거롭다
    fn negamax(
        &mut self,
        pos: &mut Position,
        depth: i32,
        ply: usize,
        mut alpha: i32,
        beta: i32,
        pv_node: bool,
        allow_null: bool,
    ) -> i32 {
        self.pv_len[ply] = ply;
        if let Some(s) = self.terminal(pos, ply) {
            return s;
        }
        if self.is_repetition(pos.hash) {
            // 대국 규칙과 같이 반복은 밀어낸 수로 판정한다 (같으면 무승부)
            return self.adjudicate(pos, ply);
        }
        if depth <= 0 || ply >= MAX_PLY - 1 {
            return self.qsearch(pos, ply, alpha, beta, QS_DEPTH);
        }
        if self.tick() {
            return 0;
        }

        let alpha0 = alpha;
        let e = self.tt[(pos.hash as usize) & (self.tt.len() - 1)];
        let mut tt_move = None;
        if e.key == pos.hash && e.flag != 0 {
            tt_move = unpack(e.mv);
            if !pv_node && e.depth as i32 >= depth {
                let s = from_tt(e.score, ply);
                let cut = match e.flag {
                    EXACT => true,
                    LOWER => s >= beta,
                    _ => s <= alpha,
                };
                if cut {
                    return s;
                }
            }
        }

        // 널 무브: 한 수 쉬어도(상대가 두 번 둬도) beta 이상이면 이 노드는 잘라도 된다.
        // 아발론에는 "두지 않는 게 나은" 국면(추크추방)이 사실상 없어서 안전하다.
        if self.prune
            && !pv_node
            && allow_null
            && depth >= 3
            && beta.abs() < WIN_BOUND
            && self.eval(pos) >= beta
        {
            let r = if depth >= 6 { 3 } else { 2 };
            self.path.push(pos.hash);
            pos.make_null();
            let s = -self.negamax(pos, depth - 1 - r, ply + 1, -beta, -beta + 1, false, false);
            pos.make_null();
            self.path.pop();
            if self.stop {
                return 0;
            }
            if s >= beta {
                return if s >= WIN_BOUND { beta } else { s };
            }
        }

        // 치환표 수가 있으면 그것부터 둬 본다. 거기서 베타 컷이 나면 나머지 수는 만들지도 않는다.
        let mut moves = std::mem::take(&mut self.bufs[ply]);
        let mut keys = std::mem::take(&mut self.keys[ply]);
        moves.clear();
        keys.clear();
        let mut generated = false;
        match tt_move.filter(|&m| pos.is_legal(m)) {
            Some(m) => {
                moves.push(m);
                keys.push(TT_KEY);
            }
            None => {
                pos.legal_moves_into(&mut moves);
                if moves.is_empty() {
                    self.bufs[ply] = moves;
                    self.keys[ply] = keys;
                    return self.adjudicate(pos, ply);
                }
                self.order(pos, &mut moves, &mut keys, 0, None, ply);
                generated = true;
            }
        }

        self.path.push(pos.hash);
        let mut best = -INF;
        let mut best_move = None;
        let mut i = 0;
        loop {
            if i == moves.len() {
                if generated {
                    break;
                }
                // 치환표 수로 컷이 안 났다: 나머지 수를 만들어 뒤에 붙인다
                let tt = moves[0];
                pos.legal_moves_into(&mut moves);
                if let Some(k) = moves[1..].iter().position(|&m| m == tt) {
                    moves.remove(k + 1);
                }
                self.order(pos, &mut moves, &mut keys, 1, None, ply);
                generated = true;
                if i == moves.len() {
                    break;
                }
            }
            pick_next(&mut moves, &mut keys, i);
            let m = moves[i];
            // 늦은 수 줄이기: 정렬상 뒤쪽의 조용한 수는 한두 수 얕게 먼저 보고, alpha를 넘으면 제 깊이로 다시 본다
            let reduce = if self.prune && i >= 3 && depth >= 3 {
                // 정렬 점수가 히스토리 구간이면 밀기가 아니고 치환표 수·킬러도 아니다
                if keys[i] < KILLER_KEY - 1 {
                    if i >= 12 && depth >= 5 {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                }
            } else {
                0
            };
            let u = pos.make_move(m);
            let score = if i == 0 {
                -self.negamax(pos, depth - 1, ply + 1, -beta, -alpha, pv_node, true)
            } else {
                let mut s = -self.negamax(
                    pos,
                    depth - 1 - reduce,
                    ply + 1,
                    -alpha - 1,
                    -alpha,
                    false,
                    true,
                );
                if reduce > 0 && s > alpha && !self.stop {
                    s = -self.negamax(pos, depth - 1, ply + 1, -alpha - 1, -alpha, false, true);
                }
                if s > alpha && s < beta && pv_node && !self.stop {
                    -self.negamax(pos, depth - 1, ply + 1, -beta, -alpha, true, true)
                } else {
                    s
                }
            };
            pos.unmake_move(u);
            if self.stop {
                break;
            }
            if score > best {
                best = score;
                best_move = Some(m);
                if score > alpha {
                    alpha = score;
                    self.set_pv(ply, m);
                    if alpha >= beta {
                        if pos
                            .inline_kind(m)
                            .is_none_or(|k| matches!(k, InlineResult::Step))
                        {
                            let k = pack(m);
                            if self.killers[ply][0] != k {
                                self.killers[ply][1] = self.killers[ply][0];
                                self.killers[ply][0] = k;
                            }
                            self.history[hist_index(m)] += depth * depth;
                        }
                        break;
                    }
                }
            }
            i += 1;
        }
        self.path.pop();
        self.bufs[ply] = moves;
        self.keys[ply] = keys;
        if self.stop {
            return 0;
        }
        let flag = if best >= beta {
            LOWER
        } else if best > alpha0 {
            EXACT
        } else {
            UPPER
        };
        self.store(pos.hash, depth, best, flag, best_move, ply);
        best
    }

    fn qsearch(
        &mut self,
        pos: &mut Position,
        ply: usize,
        mut alpha: i32,
        beta: i32,
        qd: i32,
    ) -> i32 {
        self.pv_len[ply] = ply;
        if self.tick() {
            return 0;
        }
        if let Some(s) = self.terminal(pos, ply) {
            return s;
        }
        let stand = self.eval(pos);
        if stand >= beta || qd == 0 || ply >= MAX_PLY - 1 {
            return stand;
        }
        if stand > alpha {
            alpha = stand;
        }
        let mut best = stand;
        let mut moves = std::mem::take(&mut self.bufs[ply]);
        moves.clear();
        gen_push_offs(pos.own(), pos.opp(), &mut moves);
        for &m in moves.iter() {
            let u = pos.make_move(m);
            let s = -self.qsearch(pos, ply + 1, -beta, -alpha, qd - 1);
            pos.unmake_move(u);
            if self.stop {
                break;
            }
            if s > best {
                best = s;
                if s > alpha {
                    alpha = s;
                    self.set_pv(ply, m);
                    if s >= beta {
                        break;
                    }
                }
            }
        }
        self.bufs[ply] = moves;
        if self.stop {
            0
        } else {
            best
        }
    }

    fn store(&mut self, key: u64, depth: i32, score: i32, flag: u8, m: Option<Move>, ply: usize) {
        let idx = (key as usize) & (self.tt.len() - 1);
        let e = &mut self.tt[idx];
        // 같은 국면이면 더 깊은 정보 우선, 다른 국면이면 항상 덮어쓴다
        if e.key == key && e.depth as i32 > depth && e.flag == EXACT {
            return;
        }
        let mv = m.map(pack).unwrap_or(if e.key == key { e.mv } else { 0 });
        *e = Entry {
            key,
            score: to_tt(score, ply),
            mv,
            depth: depth.min(127) as i8,
            flag,
        };
    }

    /// 수 정렬: 치환표 수 > 밀어내기 > 판 안 밀기(많이 밀수록) > 킬러 > 히스토리.
    /// keys에는 각 수의 점수가 같은 순서로 남는다(LMR이 조용한 수를 가리는 데 쓴다).
    /// moves[start..]의 점수를 keys 뒤에 붙인다(keys 길이는 start여야 한다).
    fn order(
        &self,
        pos: &Position,
        moves: &mut [Move],
        keys: &mut Vec<i32>,
        start: usize,
        tt_move: Option<Move>,
        ply: usize,
    ) {
        let tt = tt_move.map(pack).unwrap_or(0);
        let k = self.killers[ply];
        keys.truncate(start);
        for &m in moves[start..].iter() {
            let p = pack(m);
            keys.push(if p == tt {
                TT_KEY
            } else {
                match pos.inline_kind(m) {
                    Some(InlineResult::PushOff(_)) => 1 << 29,
                    Some(InlineResult::Push(n)) => (1 << 28) + n as i32,
                    _ if p == k[0] => KILLER_KEY,
                    _ if p == k[1] => KILLER_KEY - 1,
                    _ => self.history[hist_index(m)].min(KILLER_KEY - 2),
                }
            });
        }
        // 정렬은 하지 않는다. 탐색이 i번째 수를 볼 때 pick_next가 남은 것 중 최고를 앞으로 꺼낸다
        // (베타 컷이 앞에서 나면 나머지는 정렬할 필요가 없다)
    }

    pub fn nodes(&self) -> u64 {
        self.nodes
    }
}

/// moves[i..] 중 점수가 가장 높은 수(같으면 앞의 것)를 i 자리로 옮긴다. 사이의 수들은 한 칸씩 밀려
/// 원래 순서를 지키므로, 끝까지 다 꺼내면 안정 정렬한 것과 같은 순서가 된다.
fn pick_next(moves: &mut [Move], keys: &mut [i32], i: usize) {
    let mut best = i;
    for j in i + 1..moves.len() {
        if keys[j] > keys[best] {
            best = j;
        }
    }
    if best != i {
        moves[i..=best].rotate_right(1);
        keys[i..=best].rotate_right(1);
    }
}

fn hist_index(m: Move) -> usize {
    (m.tail as usize * 6 + m.dir as usize) * 3 + (m.n as usize - 1)
}

/// 수순을 적용한 국면.
pub fn play_line(pos: &Position, line: &[Move]) -> Position {
    let mut p = *pos;
    for &m in line {
        p.make_move(m);
    }
    p
}
