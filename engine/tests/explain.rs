use abalone_engine::board::*;
use abalone_engine::eval::{self, DEFAULT_WEIGHTS, WIN_BOUND};
use abalone_engine::explain::{self, Grade};
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::{mask_of, Layout};
use abalone_engine::notation::{move_to_string, parse_move};
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

fn searcher() -> Searcher {
    Searcher::with_std_clock(16, DEFAULT_WEIGHTS)
}

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
            for _ in 0..rng.below(max_plies + 1) {
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

#[test]
fn contributions_sum_to_loss_and_scores_match_pv_ends() {
    let mut s = searcher();
    let mut rng = Rng::new(9);
    let mut checked = 0;
    let mut grades = std::collections::BTreeMap::new();
    for g in random_games(41, 120, 80) {
        let moves = g.legal_moves();
        let played = moves[rng.below(moves.len())];
        let a = explain::analyze(&g, played, &mut s, Limits::depth(3)).unwrap();
        *grades.entry(a.grade).or_insert(0) += 1;
        assert_eq!(a.played.first, played);
        assert_eq!(a.played.pv[0], played);
        assert!(a.loss >= 0);
        if a.best.score.abs() < WIN_BOUND {
            assert_eq!(
                eval::evaluate_for(&a.best.end, a.mover, &DEFAULT_WEIGHTS),
                a.best.score
            );
        }
        if let Some(c) = a.contributions {
            // 승패 점수가 아니면 두 끝 국면의 평가 = 각 점수, 기여도 차이 합 = 손실
            assert_eq!(
                eval::evaluate_for(&a.played.end, a.mover, &DEFAULT_WEIGHTS),
                a.played.score
            );
            assert_eq!(
                c.iter().sum::<i32>(),
                a.loss,
                "sum of contribution deltas must equal the loss"
            );
            assert_eq!(a.loss, a.best.score - a.played.score);
            checked += 1;
        }
        // 이유: 양수, 큰 순서, 최대 3개, 문장이 하나씩
        assert!(a.reasons.len() <= 3);
        for w in a.reasons.windows(2) {
            assert!(w[0].delta >= w[1].delta);
        }
        assert!(a.reasons.iter().all(|r| r.delta > 0));
        assert!(!a.sentences.is_empty());
        assert!(
            a.sentences.len() > a.reasons.len(),
            "one header plus one sentence per reason"
        );
        // 등급과 손실의 관계 (구슬 차이 보정이 없을 때)
        let material = |l: &explain::Line| l.mine.pushed_off - l.theirs.pushed_off;
        if material(&a.best) != material(&a.played) {
            assert!(a.grade >= Grade::Mistake);
            continue;
        }
        match a.grade {
            Grade::Best => {}
            Grade::Good if a.contributions.is_some() => assert!(a.loss < explain::GOOD_BELOW),
            Grade::Inaccuracy => {
                assert!(a.loss >= explain::GOOD_BELOW && a.loss < explain::INACCURACY_BELOW)
            }
            Grade::Mistake => {
                assert!(a.loss >= explain::INACCURACY_BELOW && a.loss < explain::MISTAKE_BELOW)
            }
            _ => {}
        }
    }
    eprintln!("grades {grades:?}, contribution checks {checked}");
    assert!(checked > 60, "checked {checked}");
    assert!(grades.len() >= 3, "expected a spread of grades: {grades:?}");
}

#[test]
fn best_move_is_graded_best() {
    let mut s = searcher();
    for g in random_games(42, 20, 60) {
        let best = s.search(&g, Limits::depth(3)).best.unwrap();
        let a = explain::analyze(&g, best, &mut s, Limits::depth(3)).unwrap();
        assert_eq!(a.grade, Grade::Best);
        assert_eq!(a.loss, 0);
        assert_eq!(a.sentences[0], "가장 좋은 수예요!");
    }
}

fn pos(black: &[&str], white: &[&str], side: Color) -> Position {
    Position::new(mask_of(black), mask_of(white), side)
}

#[test]
fn missing_a_win_is_a_blunder_with_a_counted_reason() {
    // 흑이 5개 밀어낸 상태, E7E8로 6번째를 밀어내면 이긴다
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
    let played = parse_move(&g.pos, "A1A2").unwrap_or(g.legal_moves()[0]);
    let a = explain::analyze(&g, played, &mut s, Limits::depth(3)).unwrap();
    assert_eq!(a.grade, Grade::Blunder);
    assert_eq!(move_to_string(a.best.first), "E7E8");
    assert!(
        a.sentences[0].contains("E7E8") && a.sentences[0].contains("1수 안에 이길"),
        "{:?}",
        a.sentences
    );
}

#[test]
fn hanging_a_marble_is_explained_by_the_threat_or_the_lost_marble() {
    // 흑 E9가 백 E7 E8에게 밀려날 수 있다. 흑이 E9를 그대로 두면 구슬을 잃는다.
    let g = Game::from_position(
        pos(
            &["E9", "B2", "B3", "C2", "C3", "C4", "D3", "D4", "A1", "A2"],
            &["E7", "E8", "H4", "H5", "H6", "I5", "I6", "G4", "G5", "I7"],
            Color::Black,
        ),
        GameConfig {
            move_limit: 0,
            ..GameConfig::default()
        },
    );
    let mut s = searcher();
    // 수비와 상관없는 수
    let played = g
        .legal_moves()
        .into_iter()
        .find(|&m| {
            !m.cells()
                .any(|c| c == abalone_engine::notation::parse_cell("E9").unwrap())
                && move_to_string(m).starts_with('A')
        })
        .unwrap();
    let a = explain::analyze(&g, played, &mut s, Limits::depth(4)).unwrap();
    assert!(a.grade >= Grade::Mistake, "{:?} loss {}", a.grade, a.loss);
    let keys: Vec<&str> = a.reasons.iter().map(|r| r.key).collect();
    assert!(
        keys.contains(&"pushed_off") || keys.contains(&"threat"),
        "{keys:?} {:?}",
        a.sentences
    );
    // 문장에는 근거 숫자가 들어간다
    let r = &a.reasons[0];
    assert!(
        a.sentences[1].contains(&r.played_value.to_string())
            || a.sentences[1].contains(&r.best_value.to_string())
    );
}

#[test]
fn hint_suggests_the_search_best_move() {
    let mut s = searcher();
    for g in random_games(43, 15, 50) {
        let h = explain::hint(&g, &mut s, Limits::depth(3)).unwrap();
        let best = s.search(&g, Limits::depth(3)).best.unwrap();
        assert_eq!(h.best.first, best);
        assert!(h.sentences[0].contains(&move_to_string(best)));
        assert!(h.reasons.len() <= 2);
    }
}
