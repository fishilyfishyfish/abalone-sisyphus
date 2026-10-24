//! 비트보드 엔진과 별개로, 규칙 문장을 그대로 옮긴 단순 구현(좌표 집합 기반)을 만들고
//! 무작위 대국 수천 판에서 합법수 목록, 수 적용 결과, 특징값, 무르기, 해시가 일치하는지 본다.
//!
//! 판 수는 환경 변수 CROSS_GAMES로 바꿀 수 있다 (기본 3000).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use abalone_engine::board::*;
use abalone_engine::features;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;

type Cell = (i32, i32);

const V: [Cell; 6] = [(0, 1), (1, 1), (1, 0), (0, -1), (-1, -1), (-1, 0)];

fn on(c: Cell) -> bool {
    (0..9).contains(&c.0) && (0..9).contains(&c.1) && (c.0 - c.1).abs() <= 4
}

fn add(a: Cell, b: Cell) -> Cell {
    (a.0 + b.0, a.1 + b.1)
}

fn neg(a: Cell) -> Cell {
    (-a.0, -a.1)
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Naive {
    /// 1 = 흑, 2 = 백
    cells: BTreeMap<Cell, u8>,
    side: u8,
    pushed: [u32; 3],
}

/// 수 = (움직이는 구슬 칸 집합, 방향 벡터)
type NMove = (Vec<Cell>, Cell);

impl Naive {
    fn from_pos(p: &Position) -> Naive {
        let mut cells = BTreeMap::new();
        for r in 0..9 {
            for c in 0..9 {
                if on((r, c)) {
                    match p.at(sq(r, c)) {
                        Some(Color::Black) => {
                            cells.insert((r, c), 1);
                        }
                        Some(Color::White) => {
                            cells.insert((r, c), 2);
                        }
                        None => {}
                    }
                }
            }
        }
        Naive {
            cells,
            side: if p.side == Color::Black { 1 } else { 2 },
            pushed: [0, p.pushed[0] as u32, p.pushed[1] as u32],
        }
    }

    fn get(&self, c: Cell) -> u8 {
        *self.cells.get(&c).unwrap_or(&0)
    }

    /// side 쪽이 둔다고 가정한 합법수와, 각 수가 밀어내는 칸 (없으면 None)
    fn moves_for(&self, side: u8) -> Vec<(NMove, Option<Cell>)> {
        let opp = 3 - side;
        let mut groups: BTreeSet<(Vec<Cell>, Option<Cell>)> = BTreeSet::new();
        for (&a, &col) in &self.cells {
            if col != side {
                continue;
            }
            groups.insert((vec![a], None));
            for v in V {
                let b = add(a, v);
                if self.get(b) == side {
                    let mut g = vec![a, b];
                    g.sort();
                    let axis = if g[0] == a { v } else { neg(v) };
                    groups.insert((g, Some(axis)));
                    let c = add(b, v);
                    if self.get(c) == side {
                        let mut g = vec![a, b, c];
                        g.sort();
                        let axis = if g[0] == a { v } else { neg(v) };
                        groups.insert((g, Some(axis)));
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (g, axis) in groups {
            for d in V {
                let inline = match axis {
                    None => true,
                    Some(v) => d == v || d == neg(v),
                };
                let key = (g.clone(), d);
                if !seen.insert(key.clone()) {
                    continue;
                }
                if inline {
                    let leader = *g.iter().find(|&&x| !g.contains(&add(x, d))).unwrap();
                    let mut p = add(leader, d);
                    let mut opp_count = 0usize;
                    let mut ok = None;
                    loop {
                        if !on(p) {
                            if opp_count > 0 {
                                ok = Some(Some(add(p, neg(d))));
                            }
                            break;
                        }
                        let x = self.get(p);
                        if x == 0 {
                            ok = Some(None);
                            break;
                        }
                        if x == side {
                            break;
                        }
                        debug_assert_eq!(x, opp);
                        opp_count += 1;
                        if opp_count >= g.len() {
                            break;
                        }
                        p = add(p, d);
                    }
                    if let Some(cap) = ok {
                        out.push((key, cap));
                    }
                } else if g.iter().all(|&x| on(add(x, d)) && self.get(add(x, d)) == 0) {
                    out.push((key, None));
                }
            }
        }
        out
    }

    fn moves(&self) -> Vec<NMove> {
        let mut v: Vec<NMove> = self.moves_for(self.side).into_iter().map(|x| x.0).collect();
        v.sort();
        v
    }

    fn apply(&self, m: &NMove) -> Naive {
        let (g, d) = m;
        let mut moving: Vec<Cell> = g.clone();
        // 밀리는 상대 구슬 줄
        let leader = *g.iter().find(|&&x| !g.contains(&add(x, *d))).unwrap();
        let inline = g.len() == 1 || g.contains(&add(g[0], *d)) || g.contains(&add(g[0], neg(*d)));
        if inline {
            let mut p = add(leader, *d);
            while on(p) && self.get(p) == 3 - self.side {
                moving.push(p);
                p = add(p, *d);
            }
        }
        let mut n = self.clone();
        let vals: Vec<(Cell, u8)> = moving.iter().map(|&c| (c, self.get(c))).collect();
        for &(c, _) in &vals {
            n.cells.remove(&c);
        }
        for &(c, col) in &vals {
            let t = add(c, *d);
            if on(t) {
                n.cells.insert(t, col);
            } else {
                n.pushed[(3 - col) as usize] += 1;
            }
        }
        n.side = 3 - self.side;
        n
    }

    fn dist(c: Cell) -> i32 {
        let (dr, dc) = (c.0 - 4, c.1 - 4);
        // 큐브 좌표 (x, y, z) = (dc, -dr, dr - dc)
        let (x, y, z) = (dc, -dr, dr - dc);
        (x.abs() + y.abs() + z.abs()) / 2
    }

    fn features(&self, side: u8) -> [i32; 7] {
        let own: Vec<Cell> = self
            .cells
            .iter()
            .filter(|x| *x.1 == side)
            .map(|x| *x.0)
            .collect();
        let mut pairs = 0;
        for i in 0..own.len() {
            for j in i + 1..own.len() {
                if V.iter().any(|&v| add(own[i], v) == own[j]) {
                    pairs += 1;
                }
            }
        }
        let isolated = own
            .iter()
            .filter(|&&c| V.iter().all(|&v| self.get(add(c, v)) != side))
            .count();
        let targets = |who: u8| -> i32 {
            let s: BTreeSet<Cell> = self
                .moves_for(who)
                .into_iter()
                .filter_map(|x| x.1)
                .collect();
            s.len() as i32
        };
        [
            self.pushed[side as usize] as i32,
            own.iter().map(|&c| Naive::dist(c)).sum(),
            pairs,
            own.iter().filter(|&&c| Naive::dist(c) == 4).count() as i32,
            targets(3 - side),
            targets(side),
            isolated as i32,
        ]
    }
}

fn engine_move_key(m: Move) -> NMove {
    let mut cells: Vec<Cell> = m.cells().map(rc).collect();
    cells.sort();
    (cells, DIR_RC[m.dir as usize])
}

#[test]
fn random_games_match_naive_implementation() {
    let games: usize = std::env::var("CROSS_GAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    let mut rng = Rng::new(20261024);
    let mut total_positions = 0usize;
    let mut total_moves = 0usize;
    let mut push_offs = 0usize;
    let mut decisive = 0usize;
    let mut feature_checks = 0usize;
    for gi in 0..games {
        let layout = layouts[gi % 3];
        let mut game = Game::new(GameConfig {
            layout,
            ..GameConfig::default()
        });
        let mut naive = Naive::from_pos(&game.pos);
        let mut history_positions = vec![game.pos];
        while !game.is_over() {
            let moves = game.legal_moves();
            let mut ek: Vec<NMove> = moves.iter().map(|&m| engine_move_key(m)).collect();
            ek.sort();
            let nk = naive.moves();
            assert_eq!(ek.len(), moves.len(), "duplicate engine moves");
            assert_eq!(ek, nk, "move list mismatch in game {gi} ply {}", game.ply());
            total_positions += 1;
            total_moves += moves.len();

            // 특징값 일치 (단순 구현이 느려서 4수마다)
            if game.ply() % 4 == 1 {
                let f = features::compute(&game.pos);
                assert_eq!(
                    f.black.as_array(),
                    naive.features(1),
                    "black features g{gi}"
                );
                assert_eq!(
                    f.white.as_array(),
                    naive.features(2),
                    "white features g{gi}"
                );
                feature_checks += 1;
            }

            // 일부 국면에서는 모든 수의 적용 결과와 무르기 비교
            let check_all = game.ply().is_multiple_of(8);
            let pick = rng.below(moves.len());
            for (i, &m) in moves.iter().enumerate() {
                if !check_all && i != pick {
                    continue;
                }
                let mut p = game.pos;
                let before = p;
                let u = p.make_move(m);
                assert_eq!(p.hash, p.compute_hash());
                let nn = naive.apply(&engine_move_key(m));
                assert_eq!(Naive::from_pos(&p), nn, "apply mismatch {:?}", m);
                assert_eq!(game.pos.is_push_off(m), p.pushed != before.pushed);
                p.unmake_move(u);
                assert_eq!(p, before);
            }

            let m = moves[pick];
            if game.pos.is_push_off(m) {
                push_offs += 1;
            }
            naive = naive.apply(&engine_move_key(m));
            game.play_unchecked(m);
            history_positions.push(game.pos);
        }
        if matches!(game.outcome(), abalone_engine::Outcome::Win(..)) {
            decisive += 1;
        }
        // 전부 무르면 시작 국면과 중간 국면을 정확히 되짚는다
        while game.undo().is_some() {
            history_positions.pop();
            assert_eq!(Some(&game.pos), history_positions.last());
        }
        assert_eq!(game.pos, layout.position());
    }
    eprintln!(
        "cross-check: {games} games, {total_positions} positions, {total_moves} moves, \
         {push_offs} push-offs, {decisive} decisive, {feature_checks} feature checks"
    );
    assert!(push_offs > 0);
}

#[test]
fn hash_counts_repetition_by_position_and_side() {
    // 해시가 서로 다른 국면 사이에서 충돌하지 않는지 짧게 확인
    let mut rng = Rng::new(7);
    let mut seen: HashMap<u64, Position> = HashMap::new();
    for _ in 0..200 {
        let mut g = Game::new(GameConfig::default());
        while !g.is_over() && g.ply() < 60 {
            let ms = g.legal_moves();
            g.play_unchecked(ms[rng.below(ms.len())]);
            let p = g.pos;
            if let Some(q) = seen.insert(p.hash, p) {
                assert_eq!((q.stones, q.side), (p.stones, p.side));
            }
        }
    }
}
