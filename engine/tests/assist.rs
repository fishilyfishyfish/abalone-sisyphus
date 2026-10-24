use abalone_engine::assist::{group_options, single_options, Blocked};
use abalone_engine::board::*;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::{mask_of, Layout};
use abalone_engine::notation::{move_to_string, parse_cell};
use abalone_engine::rng::Rng;

fn pos(black: &[&str], white: &[&str]) -> Position {
    Position::new(mask_of(black), mask_of(white), Color::Black)
}

fn c(name: &str) -> i32 {
    parse_cell(name).unwrap()
}

const E: usize = 0;
const W: usize = 3;
const NE: usize = 1;
const SE: usize = 5;

fn single(p: &Position, cell: &str, d: usize) -> (Option<String>, Option<Blocked>) {
    let o = single_options(p, c(cell))[d];
    (o.mv.map(move_to_string), o.blocked)
}

fn group(p: &Position, cells: &[&str], d: usize) -> (Option<String>, Option<Blocked>) {
    let v: Vec<i32> = cells.iter().map(|s| c(s)).collect();
    let o = group_options(p, &v)[d];
    (o.mv.map(move_to_string), o.blocked)
}

#[test]
fn tapping_rear_marble_moves_the_whole_line_in_front() {
    let p = pos(&["E2", "E3", "E4"], &["A1"]);
    // 맨 뒤 E2를 E로 → 3개가 같이 간다
    assert_eq!(single(&p, "E2", E).0.as_deref(), Some("E2E3"));
    // 가운데 E3을 E로 → E3, E4 두 개
    let o = single_options(&p, c("E3"))[E].mv.unwrap();
    assert_eq!(o.n, 2);
    // 맨 앞 E4를 E로 → 혼자
    assert_eq!(single_options(&p, c("E4"))[E].mv.unwrap().n, 1);
}

#[test]
fn tapping_middle_marble_borrows_marbles_behind_to_push() {
    // 흑 E2 E3 E4, 백 E5 E6: E3 기준 E3+E4(2:2)는 불가 → E2까지 보태 3:2로 민다
    let p = pos(&["E2", "E3", "E4"], &["E5", "E6"]);
    let o = single_options(&p, c("E3"))[E].mv.unwrap();
    assert_eq!(o.n, 3);
    assert_eq!(move_to_string(o), "E2E3");
}

#[test]
fn blocked_reasons_for_single_marble() {
    // 판 밖
    let p = pos(&["E9"], &["A1"]);
    assert_eq!(single(&p, "E9", E), (None, Some(Blocked::OffBoard)));
    // 2:2
    let p = pos(&["E3", "E4"], &["E5", "E6"]);
    assert_eq!(
        single(&p, "E3", E),
        (None, Some(Blocked::NotStronger { mine: 2, theirs: 2 }))
    );
    // 1:1
    let p = pos(&["E4"], &["E5"]);
    assert_eq!(
        single(&p, "E4", E),
        (None, Some(Blocked::NotStronger { mine: 1, theirs: 1 }))
    );
    // 3:3
    let p = pos(&["E1", "E2", "E3"], &["E4", "E5", "E6"]);
    assert_eq!(
        single(&p, "E2", E),
        (None, Some(Blocked::NotStronger { mine: 3, theirs: 3 }))
    );
    // 상대 뒤에 내 구슬
    let p = pos(&["E3", "E4", "E6"], &["E5"]);
    assert_eq!(single(&p, "E3", E), (None, Some(Blocked::OwnBehind)));
    // 4개 줄의 맨 뒤
    let p = pos(&["E2", "E3", "E4", "E5"], &["A1"]);
    assert_eq!(single(&p, "E2", E), (None, Some(Blocked::TooMany)));
    // 4개 줄이 3개를 상대로: 한 번에 3개까지라 3:3, 맨 뒤는 4개라 불가
    let p = pos(&["E1", "E2", "E3", "E4"], &["E5", "E6", "E7"]);
    assert_eq!(
        single(&p, "E2", E).1,
        Some(Blocked::NotStronger { mine: 3, theirs: 3 })
    );
    assert_eq!(single(&p, "E1", E).1, Some(Blocked::TooMany));
    // 상대 구슬이 아님
    let p = pos(&["E3"], &["E5"]);
    assert_eq!(single(&p, "E5", E).1, Some(Blocked::NotOwn));
}

#[test]
fn blocked_reasons_for_group() {
    let p = pos(&["E4", "E5"], &["F6"]);
    // 옆으로 NE: F5 F6 중 F6에 백
    assert_eq!(
        group(&p, &["E4", "E5"], NE),
        (None, Some(Blocked::BroadsideBlocked))
    );
    // 옆으로 SE: D4 D5 비어 있음
    assert_eq!(group(&p, &["E4", "E5"], SE).0.as_deref(), Some("E4E5D4"));
    // 줄이 아님
    let p = pos(&["E4", "E6"], &["A1"]);
    assert_eq!(group(&p, &["E4", "E6"], E).1, Some(Blocked::NotLine));
    // 앞에 고르지 않은 내 구슬
    let p = pos(&["E3", "E4", "E5"], &["A1"]);
    assert_eq!(group(&p, &["E3", "E4"], E).1, Some(Blocked::OwnInWay));
    // 판 밖 옆걸음
    let p = pos(&["I6", "I7"], &["A1"]);
    assert_eq!(group(&p, &["I6", "I7"], NE).1, Some(Blocked::OffBoard));
    // 고른 2개로는 2:2 (뒤의 E2는 고르지 않음)
    let p = pos(&["E2", "E3", "E4"], &["E5", "E6"]);
    assert_eq!(
        group(&p, &["E3", "E4"], E).1,
        Some(Blocked::NotStronger { mine: 2, theirs: 2 })
    );
    // 같은 국면에서 E3만 눌러 밀면 뒤의 E2까지 보태 3:2로 민다
    assert_eq!(single(&p, "E3", E).0.as_deref(), Some("E2E3"));
    let p = pos(&["E3", "E4"], &["E5", "E6"]);
    assert_eq!(
        group(&p, &["E3", "E4"], E).1,
        Some(Blocked::NotStronger { mine: 2, theirs: 2 })
    );
    assert_eq!(group(&p, &["E3", "E4"], W).0.as_deref(), Some("E4E3"));
}

#[test]
fn options_agree_with_legal_moves_in_random_games() {
    let mut rng = Rng::new(99);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    for gi in 0..150 {
        let mut g = Game::new(GameConfig {
            layout: layouts[gi % 3],
            ..GameConfig::default()
        });
        while !g.is_over() {
            let p = g.pos;
            let legal = p.legal_moves();
            for x in Bits(p.own()) {
                let opts = single_options(&p, x);
                for (d, o) in opts.iter().enumerate() {
                    assert_eq!(o.dir as usize, d);
                    let cands: Vec<Move> = legal
                        .iter()
                        .copied()
                        .filter(|m| {
                            m.dir as usize == d && m.is_inline() && m.cells().any(|s| s == x)
                        })
                        .collect();
                    match o.mv {
                        Some(m) => {
                            assert!(cands.contains(&m));
                            assert!(o.blocked.is_none());
                            if cands.iter().any(|m| m.head_and_rear().1 == x) {
                                assert_eq!(m.head_and_rear().1, x, "prefer rear = tapped marble");
                            }
                        }
                        None => {
                            assert!(cands.is_empty(), "missed a move for {x} dir {d}");
                            assert!(o.blocked.is_some());
                        }
                    }
                }
            }
            // 모든 합법수는 그 구슬 집합으로 고르면 그 방향 선택지에 그대로 나온다
            for &m in &legal {
                let cells: Vec<i32> = m.cells().collect();
                let o = group_options(&p, &cells)[m.dir as usize];
                assert_eq!(o.mv, Some(m));
            }
            // 둘 수 없는 방향에는 항상 이유가 있다
            for &m in legal.iter().filter(|m| m.n >= 2) {
                let cells: Vec<i32> = m.cells().collect();
                for o in group_options(&p, &cells) {
                    assert_eq!(o.mv.is_some(), o.blocked.is_none());
                }
            }
            g.play_unchecked(legal[rng.below(legal.len())]);
        }
    }
}
