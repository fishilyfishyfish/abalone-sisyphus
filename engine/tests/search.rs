use abalone_engine::ai::{self, Level, EASY_MARGIN};
use abalone_engine::board::*;
use abalone_engine::eval::{self, DEFAULT_WEIGHTS, WIN, WIN_BOUND};
use abalone_engine::features;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::{mask_of, Layout};
use abalone_engine::notation::{move_to_string, parse_move};
use abalone_engine::rng::Rng;
use abalone_engine::search::{play_line, Limits, Searcher, QS_DEPTH};

const W: eval::Weights = DEFAULT_WEIGHTS;

fn searcher() -> Searcher {
    Searcher::with_std_clock(16, W)
}

/// 무작위로 진행한 대국들 (수 제한 없음)
fn random_games(seed: u64, count: usize, max_plies: usize) -> Vec<Game> {
    let mut rng = Rng::new(seed);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    (0..count)
        .map(|i| {
            let cfg = GameConfig {
                layout: layouts[i % 3],
                move_limit: 0,
                ..GameConfig::default()
            };
            let mut g = Game::new(cfg);
            let plies = rng.below(max_plies + 1);
            for _ in 0..plies {
                if g.is_over() {
                    break;
                }
                let ms = g.legal_moves();
                g.play_unchecked(ms[rng.below(ms.len())]);
            }
            g
        })
        .filter(|g| !g.is_over())
        .collect()
}

// ------------------------------------------------------------- 단순 미니맥스 (가지치기·치환표·정렬 없음)

fn repeated(path: &[u64], h: u64) -> bool {
    let n = path.len() as isize;
    let mut i = n - 2;
    while i >= 0 {
        if path[i as usize] == h {
            return true;
        }
        i -= 2;
    }
    false
}

fn ref_q(pos: &Position, ply: i32, qd: i32) -> i32 {
    if pos.pushed_off_by(pos.side.other()) >= WIN_PUSHED {
        return -(WIN - ply);
    }
    let stand = eval::evaluate(pos, &W);
    if qd == 0 {
        return stand;
    }
    let mut best = stand;
    for m in pos.legal_moves() {
        if pos.is_push_off(m) {
            let mut c = *pos;
            c.make_move(m);
            best = best.max(-ref_q(&c, ply + 1, qd - 1));
        }
    }
    best
}

fn ref_negamax(pos: &Position, depth: i32, ply: i32, path: &mut Vec<u64>) -> i32 {
    if pos.pushed_off_by(pos.side.other()) >= WIN_PUSHED {
        return -(WIN - ply);
    }
    if ply > 0 && repeated(path, pos.hash) {
        let (me, them) = (
            pos.pushed_off_by(pos.side),
            pos.pushed_off_by(pos.side.other()),
        );
        return match me.cmp(&them) {
            std::cmp::Ordering::Greater => WIN - ply,
            std::cmp::Ordering::Less => -(WIN - ply),
            std::cmp::Ordering::Equal => 0,
        };
    }
    if depth <= 0 {
        return ref_q(pos, ply, QS_DEPTH);
    }
    let moves = pos.legal_moves();
    assert!(!moves.is_empty());
    path.push(pos.hash);
    let mut best = i32::MIN;
    for m in moves {
        let mut c = *pos;
        c.make_move(m);
        best = best.max(-ref_negamax(&c, depth - 1, ply + 1, path));
    }
    path.pop();
    best
}

#[test]
fn contributions_sum_to_evaluation() {
    for g in random_games(1, 200, 80) {
        let p = g.pos;
        for c in [Color::Black, Color::White] {
            let contrib = eval::contributions(&p, c, &W);
            assert_eq!(contrib.iter().sum::<i32>(), eval::evaluate_for(&p, c, &W));
        }
        assert_eq!(
            eval::evaluate_for(&p, Color::Black, &W),
            -eval::evaluate_for(&p, Color::White, &W)
        );
        assert_eq!(eval::evaluate(&p, &W), eval::evaluate_for(&p, p.side, &W));
        // 특징값 diff는 표시용 특징값과 같은 값에서 나온다
        let f = features::compute(&p);
        let d = features::diff(&p, Color::Black);
        let (b, w) = (f.black.as_array(), f.white.as_array());
        assert_eq!(d, std::array::from_fn(|i| b[i] - w[i]));
    }
    // 두 국면의 기여도 차이 합 = 평가 차이
    let gs = random_games(2, 20, 60);
    for pair in gs.windows(2) {
        let (a, b) = (pair[0].pos, pair[1].pos);
        let delta = eval::contribution_delta(&a, &b, Color::Black, &W);
        assert_eq!(
            delta.iter().sum::<i32>(),
            eval::evaluate_for(&a, Color::Black, &W) - eval::evaluate_for(&b, Color::Black, &W)
        );
    }
}

#[test]
fn search_score_matches_plain_minimax() {
    let mut s = searcher();
    let mut checked = 0;
    for (i, g) in random_games(3, 60, 70).into_iter().enumerate() {
        let depths: &[u32] = if i < 6 { &[1, 2, 3] } else { &[1, 2] };
        for &d in depths {
            s.clear();
            let res = s.search(&g, Limits::depth(d));
            let mut path = g.hashes()[..g.hashes().len() - 1].to_vec();
            let expect = ref_negamax(&g.pos, d as i32, 0, &mut path);
            assert_eq!(res.score, expect, "game {i} depth {d}");
            assert_eq!(res.depth, d);
            checked += 1;
        }
        // 치환표를 비우지 않고 이어서 탐색해도 같은 값
        let res = s.search(&g, Limits::depth(2));
        let mut path = g.hashes()[..g.hashes().len() - 1].to_vec();
        assert_eq!(res.score, ref_negamax(&g.pos, 2, 0, &mut path));
    }
    assert!(checked > 100);
}

#[test]
fn exact_root_scores_match_minimax_per_move() {
    let mut s = searcher();
    for g in random_games(4, 15, 60) {
        let res = s.search(
            &g,
            Limits {
                max_depth: 2,
                time_ms: 0.0,
                exact_root: true,
                prune: false,
            },
        );
        assert_eq!(res.root_scores.len(), g.legal_moves().len());
        for &(m, score) in &res.root_scores {
            let mut c = g.pos;
            c.make_move(m);
            let mut path = g.hashes().to_vec();
            assert_eq!(
                score,
                -ref_negamax(&c, 1, 1, &mut path),
                "{}",
                move_to_string(m)
            );
        }
        let best = res.root_scores.iter().map(|x| x.1).max().unwrap();
        assert_eq!(res.score, best);
    }
}

#[test]
fn pv_end_evaluation_equals_root_score() {
    let mut s = searcher();
    let mut checked = 0;
    for g in random_games(5, 80, 90) {
        for d in [1, 2, 3] {
            let res = s.search(&g, Limits::depth(d));
            if res.score.abs() >= WIN_BOUND || res.score == 0 {
                continue;
            }
            // PV는 합법수로 이어진다
            let mut p = g.pos;
            for &m in &res.pv {
                assert!(p.legal_moves().contains(&m), "illegal PV move");
                p.make_move(m);
            }
            assert!(res.pv.len() >= d as usize, "PV shorter than depth");
            let end = play_line(&g.pos, &res.pv);
            let sign = if res.pv.len().is_multiple_of(2) {
                1
            } else {
                -1
            };
            assert_eq!(sign * eval::evaluate(&end, &W), res.score, "depth {d}");
            // 같은 값을 루트 쪽 색 관점으로
            assert_eq!(eval::evaluate_for(&end, g.pos.side, &W), res.score);
            checked += 1;
        }
    }
    assert!(checked > 150, "checked {checked}");
}

fn pos(black: &[&str], white: &[&str], side: Color) -> Position {
    Position::new(mask_of(black), mask_of(white), side)
}

#[test]
fn finds_winning_push_off() {
    let white = ["E9", "I5", "I6", "I7", "I8", "H4", "H5", "H6", "H7"];
    let black = ["E7", "E8", "A1", "A2", "B1", "B2"];
    let g = Game::from_position(
        pos(&black, &white, Color::Black).with_pushed(5, 0),
        GameConfig {
            move_limit: 0,
            ..GameConfig::default()
        },
    );
    let mut s = searcher();
    for d in 1..=4 {
        let res = s.search(&g, Limits::depth(d));
        assert_eq!(move_to_string(res.best.unwrap()), "E7E8");
        assert_eq!(res.score, WIN - 1);
        assert_eq!(res.depth, 1, "stops deepening once a forced win is found");
    }
}

#[test]
fn defends_threatened_marble() {
    // 백 E7 E8이 흑 E9를 밀어낼 수 있다. 흑 차례면 피해야 한다.
    // 튜닝된 가중치(v1)는 가장자리 구슬을 밀어낸 쪽 구슬도 가장자리로 나가 약해진다고 보기 때문에
    // 깊이 3~4에서는 지키는 수와 다른 수의 차이를 작게 본다. 깊이 5에서는 지킨다.
    let g = Game::from_position(
        pos(
            &["E9", "A1", "A2", "A3"],
            &["E7", "E8", "I5", "I6", "I7"],
            Color::Black,
        ),
        GameConfig {
            move_limit: 0,
            ..GameConfig::default()
        },
    );
    let mut s = searcher();
    let res = s.search(&g, Limits::depth(5));
    let mut p = g.pos;
    p.make_move(res.best.unwrap());
    // 둔 뒤 백이 바로 밀어낼 수 있는 흑 구슬이 없다
    assert_eq!(
        features::compute(&p).black.endangered,
        0,
        "{}",
        move_to_string(res.best.unwrap())
    );
    assert!(res.score > -500);
}

#[test]
fn move_limit_adjudication_inside_search() {
    // 흑이 하나 더 밀어낸 상태에서 수 제한 1수 전: 아무 수나 둬도 흑 승
    let black = [
        "A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3", "B4", "B5", "B6", "C3", "C4", "C5",
    ];
    let white = [
        "I5", "I6", "I7", "I8", "I9", "H4", "H5", "H6", "H7", "H8", "H9", "G5", "G6",
    ];
    let mut g = Game::from_position(
        pos(&black, &white, Color::Black).with_pushed(1, 0),
        GameConfig {
            move_limit: 3,
            ..GameConfig::default()
        },
    );
    g.play(parse_move(&g.pos, "A1A2").unwrap_or(g.legal_moves()[0]));
    g.play(g.legal_moves()[0]);
    assert!(!g.is_over());
    let mut s = searcher();
    let res = s.search(&g, Limits::depth(3));
    assert_eq!(res.score, WIN - 1);
}

#[test]
fn repetition_is_judged_by_pushed_count() {
    // 흑이 하나 뒤진 상태에서 같은 국면으로 돌아가는 수순은 흑의 패배로 본다.
    // 흑 A1, 백 I9만 남은 국면에서 둘이 왔다 갔다 하면 반복이 된다.
    let g0 = Game::from_position(
        pos(&["A1", "C3"], &["I9", "G7"], Color::Black).with_pushed(0, 1),
        GameConfig {
            move_limit: 0,
            ..GameConfig::default()
        },
    );
    let mut g = g0.clone();
    for t in ["A1A2", "I9I8", "A2A1"] {
        let m = parse_move(&g.pos, t).unwrap();
        g.play(m);
    }
    // 백 차례: I8I9로 돌아가면 시작 국면 반복 → 백(앞선 쪽) 승리로 판정
    let mut s = searcher();
    let res = s.search(&g, Limits::depth(1));
    assert_eq!(move_to_string(res.best.unwrap()), "I8I9");
    assert_eq!(res.score, WIN - 1);
}

#[test]
fn respects_time_limit() {
    let g = Game::new(GameConfig::default());
    let mut s = searcher();
    let res = s.search(
        &g,
        Limits {
            max_depth: 30,
            time_ms: 100.0,
            exact_root: false,
            prune: false,
        },
    );
    assert!(res.best.is_some());
    assert!(res.depth >= 1);
    assert!(res.elapsed_ms < 400.0, "took {} ms", res.elapsed_ms);
}

#[test]
fn levels_pick_legal_moves() {
    let mut s = searcher();
    let mut rng = Rng::new(11);
    for g in random_games(6, 10, 40) {
        let legal = g.legal_moves();
        for level in [Level::Random, Level::Easy, Level::Normal] {
            let (m, res) = ai::choose(&g, level, &mut s, &mut rng).unwrap();
            assert!(legal.contains(&m));
            if level == Level::Easy {
                let res = res.unwrap();
                let sc = res.root_scores.iter().find(|x| x.0 == m).unwrap().1;
                assert!(sc >= res.score - EASY_MARGIN);
            }
        }
    }
}

#[test]
fn search_beats_random_player() {
    // 보통(깊이 3 고정, 시간 제한 없음)이 랜덤을 상대로 지지 않는지 짧게 확인
    let mut s = searcher();
    let mut rng = Rng::new(21);
    let mut wins = 0;
    let games = 6;
    for gi in 0..games {
        let mut g = Game::new(GameConfig {
            move_limit: 120,
            ..GameConfig::default()
        });
        let engine = if gi % 2 == 0 {
            Color::Black
        } else {
            Color::White
        };
        while !g.is_over() {
            let m = if g.pos.side == engine {
                s.search(&g, Limits::depth(2)).best.unwrap()
            } else {
                let ms = g.legal_moves();
                ms[rng.below(ms.len())]
            };
            g.play_unchecked(m);
        }
        if g.outcome() == abalone_engine::Outcome::Win(engine, abalone_engine::EndReason::PushedOff)
        {
            wins += 1;
        }
    }
    assert_eq!(
        wins, games,
        "engine should push off 6 against random every time"
    );
}
