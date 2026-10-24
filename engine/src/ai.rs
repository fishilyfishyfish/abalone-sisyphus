//! 컴퓨터 상대의 난이도별 수 선택.

use crate::board::Move;
use crate::game::Game;
use crate::rng::Rng;
use crate::search::{Limits, SearchResult, Searcher};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// 합법수 중 무작위
    Random,
    /// 깊이 1(+밀어내기 정지 탐색)의 정확한 점수에서, 최선보다 EASY_MARGIN 이내인 수 중 무작위
    Easy,
    /// 깊이 3까지, 0.5초
    Normal,
    /// 깊이 제한 없이 1.5초
    Hard,
}

/// 쉬움 난이도에서 "그럭저럭 괜찮은 수"로 보는 점수 폭. 기본 가중치(v2)에서 처음 쉬움(v0, 폭 150)과
/// 세기가 비슷하다. 넓히면 급격히 약해진다(구슬을 공짜로 내주는 수까지 고름).
pub const EASY_MARGIN: i32 = 450;

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "random" => Some(Level::Random),
            "easy" => Some(Level::Easy),
            "normal" => Some(Level::Normal),
            "hard" => Some(Level::Hard),
            _ => None,
        }
    }

    pub fn limits(self) -> Limits {
        match self {
            Level::Random => Limits::depth(0),
            Level::Easy => Limits {
                max_depth: 1,
                time_ms: 0.0,
                exact_root: true,
            },
            Level::Normal => Limits {
                max_depth: 3,
                time_ms: 500.0,
                exact_root: false,
            },
            Level::Hard => Limits {
                max_depth: 30,
                time_ms: 1500.0,
                exact_root: false,
            },
        }
    }
}

/// 난이도에 맞는 수와 (탐색했다면) 탐색 결과.
pub fn choose(
    game: &Game,
    level: Level,
    searcher: &mut Searcher,
    rng: &mut Rng,
) -> Option<(Move, Option<SearchResult>)> {
    let moves = game.legal_moves();
    if moves.is_empty() {
        return None;
    }
    match level {
        Level::Random => Some((moves[rng.below(moves.len())], None)),
        Level::Easy => {
            let res = searcher.search(game, level.limits());
            let best = res.score;
            let ok: Vec<Move> = res
                .root_scores
                .iter()
                .filter(|(_, s)| *s >= best - EASY_MARGIN)
                .map(|(m, _)| *m)
                .collect();
            let m = if ok.is_empty() {
                res.best?
            } else {
                ok[rng.below(ok.len())]
            };
            Some((m, Some(res)))
        }
        Level::Normal | Level::Hard => {
            let res = searcher.search(game, level.limits());
            Some((res.best?, Some(res)))
        }
    }
}

/// 깊이 1(+정지 탐색)의 정확한 점수에서 최선보다 margin 이내인 수 중 무작위.
pub fn choose_easy(
    game: &Game,
    searcher: &mut Searcher,
    rng: &mut Rng,
    margin: i32,
) -> Option<(Move, Option<SearchResult>)> {
    let res = searcher.search(game, Level::Easy.limits());
    let best = res.score;
    let ok: Vec<Move> = res
        .root_scores
        .iter()
        .filter(|(_, s)| *s >= best - margin)
        .map(|(m, _)| *m)
        .collect();
    let m = if ok.is_empty() {
        res.best?
    } else {
        ok[rng.below(ok.len())]
    };
    Some((m, Some(res)))
}
