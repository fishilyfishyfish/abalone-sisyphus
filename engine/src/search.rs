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
}

impl Limits {
    pub fn depth(d: u32) -> Limits {
        Limits {
            max_depth: d,
            time_ms: 0.0,
            exact_root: false,
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
    pub weights: Weights,
    tt: Vec<Entry>,
    killers: [[u16; 2]; MAX_PLY],
    history: Vec<i32>,
    pv: Box<[[Move; MAX_PLY]; MAX_PLY]>,
    pv_len: [usize; MAX_PLY],
    bufs: Vec<Vec<Move>>,
    /// 대국 기록 + 현재 탐색 경로의 해시 (반복 판정)
    path: Vec<u64>,
    base_ply: usize,
    move_limit: u32,
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
            tt: vec![Entry::default(); 1 << tt_bits],
            killers: [[0; 2]; MAX_PLY],
            history: vec![0; 121 * 6 * 3],
            pv: Box::new([[NO_MOVE; MAX_PLY]; MAX_PLY]),
            pv_len: [0; MAX_PLY],
            bufs: (0..MAX_PLY).map(|_| Vec::with_capacity(128)).collect(),
            path: Vec::new(),
            base_ply: 0,
            move_limit: 0,
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

    /// 치환표와 수 정렬 기록을 비운다.
    pub fn clear(&mut self) {
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
                )
            } else {
                let s = -self.negamax(pos, depth - 1, 1, -alpha - 1, -alpha, false);
                if s > alpha && !self.stop {
                    -self.negamax(pos, depth - 1, 1, -INF, -alpha, true)
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

    fn negamax(
        &mut self,
        pos: &mut Position,
        depth: i32,
        ply: usize,
        mut alpha: i32,
        beta: i32,
        pv_node: bool,
    ) -> i32 {
        self.pv_len[ply] = ply;
        if let Some(s) = self.terminal(pos, ply) {
            return s;
        }
        if self.is_repetition(pos.hash) {
            return 0;
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

        let mut moves = std::mem::take(&mut self.bufs[ply]);
        moves.clear();
        pos.legal_moves_into(&mut moves);
        if moves.is_empty() {
            self.bufs[ply] = moves;
            return self.adjudicate(pos, ply);
        }
        self.order(pos, &mut moves, tt_move, ply);

        self.path.push(pos.hash);
        let mut best = -INF;
        let mut best_move = None;
        for (i, &m) in moves.iter().enumerate() {
            let u = pos.make_move(m);
            let score = if i == 0 {
                -self.negamax(pos, depth - 1, ply + 1, -beta, -alpha, pv_node)
            } else {
                let s = -self.negamax(pos, depth - 1, ply + 1, -alpha - 1, -alpha, false);
                if s > alpha && s < beta && pv_node && !self.stop {
                    -self.negamax(pos, depth - 1, ply + 1, -beta, -alpha, true)
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
        }
        self.path.pop();
        self.bufs[ply] = moves;
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
        let stand = eval::evaluate(pos, &self.weights);
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
    fn order(&self, pos: &Position, moves: &mut [Move], tt_move: Option<Move>, ply: usize) {
        let tt = tt_move.map(pack).unwrap_or(0);
        let k = self.killers[ply];
        let mut keyed: Vec<(i32, Move)> = moves
            .iter()
            .map(|&m| {
                let p = pack(m);
                let s = if p == tt {
                    1 << 30
                } else {
                    match pos.inline_kind(m) {
                        Some(InlineResult::PushOff(_)) => 1 << 29,
                        Some(InlineResult::Push(n)) => (1 << 28) + n as i32,
                        _ if p == k[0] => 1 << 27,
                        _ if p == k[1] => (1 << 27) - 1,
                        _ => self.history[hist_index(m)].min((1 << 26) - 1),
                    }
                };
                (s, m)
            })
            .collect();
        keyed.sort_by_key(|x| std::cmp::Reverse(x.0));
        for (slot, (_, m)) in moves.iter_mut().zip(keyed) {
            *slot = m;
        }
    }

    pub fn nodes(&self) -> u64 {
        self.nodes
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
