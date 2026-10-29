//! 설명 층: 둔 수와 최선 수를 비교해 실수 등급, 기여도, 이유 문장을 만든다.
//!
//! 원칙: 모든 문장은 여기서 계산한 값(점수, 주 변화 끝 국면의 특징값, 가중치 × 특징값 차이)에서만 나온다.
//! - 두 수의 점수는 같은 깊이의 미니맥스 값이다(루트 탐색의 최선 수 점수, 둔 수는 한 수 둔 뒤 깊이 − 1 탐색).
//! - 탐색의 PV 성질 덕분에 점수 = 주 변화 끝 국면의 정적 평가이므로,
//!   두 끝 국면의 기여도 차이 합 = 손실이 정확히 성립한다(승패 점수가 아닐 때).

use crate::board::*;
use crate::eval::{self, Weights, NUM_FEATURES, WIN, WIN_BOUND};
use crate::features::{self, SideFeatures, FEATURE_NAMES};
use crate::game::{Game, Outcome};
use crate::notation::move_to_string;
use crate::search::{play_line, Limits, Searcher};

/// 실수 등급.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Grade {
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
}

/// 등급 경계(손실 점수, 밀어낸 구슬 1000점 기준. v2에서 구슬 하나의 실제 가치는 위치에 따라 1123~1615점).
/// 이 값 미만이면 그 등급. 큰 실수 = 구슬 하나에 가까운 손해.
pub const GOOD_BELOW: i32 = 100;
pub const INACCURACY_BELOW: i32 = 400;
pub const MISTAKE_BELOW: i32 = 900;

impl Grade {
    pub fn code(self) -> &'static str {
        match self {
            Grade::Best => "best",
            Grade::Good => "good",
            Grade::Inaccuracy => "inaccuracy",
            Grade::Mistake => "mistake",
            Grade::Blunder => "blunder",
        }
    }

    pub fn korean(self) -> &'static str {
        match self {
            Grade::Best => "최선",
            Grade::Good => "좋은 수",
            Grade::Inaccuracy => "아쉬운 수",
            Grade::Mistake => "실수",
            Grade::Blunder => "큰 실수",
        }
    }

    fn from_loss(loss: i32) -> Grade {
        if loss < GOOD_BELOW {
            Grade::Good
        } else if loss < INACCURACY_BELOW {
            Grade::Inaccuracy
        } else if loss < MISTAKE_BELOW {
            Grade::Mistake
        } else {
            Grade::Blunder
        }
    }
}

/// 한 수와 그 주 변화.
#[derive(Clone, Debug)]
pub struct Line {
    pub first: Move,
    /// 수를 두는 쪽 관점 점수
    pub score: i32,
    /// 첫 수부터 시작하는 주 변화
    pub pv: Vec<Move>,
    /// 주 변화를 다 둔 국면
    pub end: Position,
    /// 끝 국면의 수 두는 쪽 / 상대 특징값
    pub mine: SideFeatures,
    pub theirs: SideFeatures,
}

/// 설명에 쓰는 이유 하나: 특징 하나(또는 위협 한 쌍)에서 최선 수가 더 나은 만큼과 근거 값.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    /// 특징 키 또는 "threat"(endangered/attackable 한 쌍)
    pub key: &'static str,
    /// 기여도 차이(최선 − 둔 수), 수 두는 쪽 관점, 양수
    pub delta: i32,
    /// true면 내 구슬 쪽 변화가 주된 이유, false면 상대 쪽
    pub mine_side: bool,
    /// 근거 값: (둔 수 끝, 최선 수 끝)
    pub played_value: i32,
    pub best_value: i32,
}

#[derive(Clone, Debug)]
pub struct MoveAnalysis {
    pub mover: Color,
    pub depth: u32,
    pub best: Line,
    pub played: Line,
    /// 최선 점수 − 둔 수 점수 (0 이상. 승패 점수가 끼면 ±3000으로 눌러서 계산)
    pub loss: i32,
    pub grade: Grade,
    /// 특징별 기여도 차이(최선 끝 − 둔 수 끝, 수 두는 쪽 관점). 승패 점수가 끼면 None.
    pub contributions: Option<[i32; NUM_FEATURES]>,
    /// 보정 신경망 값의 차이(최선 끝 − 둔 수 끝, 수 두는 쪽 관점). 신경망을 쓰는 탐색일 때만 있다.
    /// 있으면 `contributions` 합 + 이 값 = 손실.
    pub residual: Option<i32>,
    pub reasons: Vec<Reason>,
    /// 반대 방향(둔 수가 최선 수보다 나은 쪽)의 기여도 중 큰 것 최대 2개. 구슬을 내준 대가로 얻은 것.
    /// 여기서 `played_value`는 최선 수 끝, `best_value`는 둔 수 끝 값이다(역할이 뒤바뀜).
    pub compensations: Vec<Reason>,
    pub sentences: Vec<String>,
    /// 주 변화 끝의 구슬 차이(밀어낸 수 − 잃은 수): 최선 수 − 둔 수. 양수면 둔 수가 구슬을 더 내준다.
    pub material_diff: i32,
    /// `material_diff`의 0 이상 부분 (친절 모드 경고에 쓴다)
    pub material_drop: i32,
}

fn clamp_score(s: i32) -> i32 {
    s.clamp(-3000, 3000)
}

fn line_from(root: &Position, mover: Color, pv: Vec<Move>, score: i32) -> Line {
    let end = play_line(root, &pv);
    let f = features::compute(&end);
    Line {
        first: pv[0],
        score,
        mine: *f.side(mover),
        theirs: *f.side(mover.other()),
        pv,
        end,
    }
}

/// 둔 수 하나를 분석한다. `game`은 수를 두기 전 대국.
pub fn analyze(
    game: &Game,
    played: Move,
    searcher: &mut Searcher,
    lim: Limits,
) -> Option<MoveAnalysis> {
    if game.is_over() || !game.legal_moves().contains(&played) {
        return None;
    }
    let mover = game.pos.side;
    let mut root = searcher.search(game, lim);
    if root.depth < 2 {
        root = searcher.search(game, Limits::depth(2));
    }
    let depth = root.depth;
    let best = line_from(&game.pos, mover, root.pv.clone(), root.score);

    let played_line = if root.best == Some(played) {
        best.clone()
    } else {
        let mut child = game.clone();
        child.play_unchecked(played);
        if child.is_over() {
            let score = match child.outcome() {
                Outcome::Win(c, _) if c == mover => WIN - 1,
                Outcome::Win(..) => -(WIN - 1),
                _ => 0,
            };
            line_from(&game.pos, mover, vec![played], score)
        } else {
            let r = searcher.search(&child, Limits::depth(depth - 1));
            let mut pv = vec![played];
            pv.extend(r.pv);
            line_from(&game.pos, mover, pv, -r.score)
        }
    };

    let (bs, ps) = (best.score, played_line.score);
    let material = |l: &Line| l.mine.pushed_off - l.theirs.pushed_off;
    let material_diff = material(&best) - material(&played_line);
    let material_drop = material_diff.max(0);
    let mate = bs.abs() >= WIN_BOUND || ps.abs() >= WIN_BOUND;
    // 승패 점수가 끼면 ±3000(구슬 3개)으로 눌러서 비교하고, 아니면 점수 차이 그대로
    let loss = if mate {
        (clamp_score(bs) - clamp_score(ps)).max(0)
    } else {
        (bs - ps).max(0)
    };
    let grade = if played == best.first || ps >= bs {
        Grade::Best
    } else if (bs >= WIN_BOUND && ps < WIN_BOUND) || (ps <= -WIN_BOUND && bs > -WIN_BOUND) {
        Grade::Blunder // 이기는 수순을 놓치거나 지는 수순으로 들어감
    } else if mate {
        Grade::Good // 둘 다 이기거나 둘 다 지는 수순
    } else {
        // 등급은 엔진의 손실로만 매긴다. 구슬을 내줘도 그만큼 자리(중앙, 줄 등)를 얻으면 손실이 작다.
        // 그런 수는 등급을 바꾸지 않고, 문장에서 "구슬을 내주지만 그 대신 …"으로 대가를 설명한다.
        Grade::from_loss(loss)
    };

    let w = searcher.weights;
    let contributions = if mate || grade == Grade::Best {
        None
    } else {
        Some(eval::contribution_delta(
            &best.end,
            &played_line.end,
            mover,
            &w,
        ))
    };
    let residual = contributions.filter(|_| searcher.nn).map(|_| {
        eval::residual_for(&best.end, mover) - eval::residual_for(&played_line.end, mover)
    });
    let reasons = contributions
        .map(|c| pick_reasons(&c, &best, &played_line, &w, loss))
        .unwrap_or_default();
    // 대가: 기여도 부호를 뒤집고 두 수의 역할을 바꿔 같은 방식으로 고른다 (구슬 자체는 빼고)
    let compensations = contributions
        .map(|c| {
            let neg: [i32; NUM_FEATURES] = std::array::from_fn(|i| -c[i]);
            let mut v = pick_reasons(&neg, &played_line, &best, &w, 0);
            v.retain(|r| r.key != "pushed_off");
            v.truncate(2);
            v
        })
        .unwrap_or_default();
    let mut a = MoveAnalysis {
        mover,
        depth,
        best,
        played: played_line,
        loss,
        grade,
        contributions,
        residual,
        reasons,
        compensations,
        sentences: Vec::new(),
        material_diff,
        material_drop,
    };
    a.sentences = sentences(&a, &game.pos);
    Some(a)
}

/// 특징 i의 기여도 차이를 내 쪽 변화분과 상대 쪽 변화분으로 나눈다.
fn split(i: usize, wi: i32, best: &Line, played: &Line) -> (i32, i32, (i32, i32), (i32, i32)) {
    let (mb, mp) = (best.mine.as_array()[i], played.mine.as_array()[i]);
    let (tb, tp) = (best.theirs.as_array()[i], played.theirs.as_array()[i]);
    (wi * (mb - mp), -wi * (tb - tp), (mp, mb), (tp, tb))
}

/// 손실과 같은 방향(최선 수가 더 나은 쪽)의 기여도를 큰 순서로 최대 3개.
fn pick_reasons(
    c: &[i32; NUM_FEATURES],
    best: &Line,
    played: &Line,
    w: &Weights,
    loss: i32,
) -> Vec<Reason> {
    let threshold = 20.max(loss * 15 / 100);
    let mut out: Vec<Reason> = Vec::new();
    for i in 0..NUM_FEATURES {
        if i == 5 {
            continue; // attackable은 endangered와 한 쌍으로 아래에서 처리
        }
        let (delta, key, wi) = if i == 4 {
            // 위협 한 쌍: 내 endangered와 상대 endangered(= 내 attackable)로 나눈다
            (c[4] + c[5], "threat", w.0[4] - w.0[5])
        } else {
            (c[i], FEATURE_NAMES[i], w.0[i])
        };
        if delta < threshold || wi == 0 {
            continue;
        }
        let (mine_part, their_part, mine_vals, their_vals) = split(i, wi, best, played);
        let mine_side = mine_part >= their_part;
        let (played_value, best_value) = if mine_side { mine_vals } else { their_vals };
        out.push(Reason {
            key,
            delta,
            mine_side,
            played_value,
            best_value,
        });
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.delta));
    out.truncate(3);
    out
}

fn reason_sentence(r: &Reason, now: &SideFeatures, now_theirs: &SideFeatures) -> String {
    let (p, b) = (r.played_value, r.best_value);
    match (r.key, r.mine_side) {
        ("pushed_off", true) => {
            let (wp, wb) = (p - now.pushed_off, b - now.pushed_off);
            if wp == 0 {
                format!("최선 수로는 상대 구슬을 {wb}개 밀어낼 수 있는데, 이 수로는 하나도 못 밀어내요.")
            } else {
                format!("최선 수로는 상대 구슬을 {wb}개 밀어낼 수 있는데, 이 수로는 {wp}개예요.")
            }
        }
        ("pushed_off", false) => {
            let (lp, lb) = (p - now_theirs.pushed_off, b - now_theirs.pushed_off);
            if lb == 0 {
                format!("이 수대로 가면 내 구슬을 {lp}개 잃어요. 최선 수로는 잃지 않아요.")
            } else {
                format!("이 수대로 가면 내 구슬을 {lp}개 잃어요. 최선 수로는 {lb}개만 잃어요.")
            }
        }
        ("center_distance", true) => {
            format!("이 수는 내 구슬이 중앙에서 멀어져요(중앙까지 거리 합 {p}, 최선 수는 {b}).")
        }
        ("center_distance", false) => {
            format!("이 수는 상대 구슬이 중앙으로 들어오게 둬요(상대 거리 합 {p}, 최선 수는 {b}).")
        }
        ("edge", true) => {
            format!("이 수대로면 가장자리에 내 구슬이 {p}개 남아요(최선 수는 {b}개).")
        }
        ("edge", false) => format!("최선 수는 상대 구슬 {b}개를 가장자리로 몰아요(이 수는 {p}개)."),
        ("threat", true) => {
            format!("이 수대로 가면 상대가 다음에 내 구슬 {p}개를 밀어낼 수 있게 돼요(최선 수는 {b}개).")
        }
        ("threat", false) => {
            format!("최선 수는 다음에 상대 구슬 {b}개를 밀어낼 수 있게 해요(이 수는 {p}개).")
        }
        ("push_moves", true) => {
            format!("최선 수는 상대를 밀 수 있는 수가 {b}개로 더 많아요(이 수는 {p}개).")
        }
        ("push_moves", false) => {
            format!("이 수는 상대에게 나를 밀 수 있는 수를 {p}개 줘요(최선 수는 {b}개).")
        }
        ("triples", true) => {
            format!("최선 수는 내 구슬 3개 줄이 {b}개라 미는 힘이 세요(이 수는 {p}개).")
        }
        ("triples", false) => {
            format!("이 수는 상대의 3개 줄을 {p}개로 만들어 줘요(최선 수는 {b}개).")
        }
        ("groups", true) => {
            format!("이 수대로면 내 구슬이 {p}덩어리로 갈라져요(최선 수는 {b}덩어리).")
        }
        ("groups", false) => {
            format!("최선 수는 상대 구슬을 {b}덩어리로 갈라 놓아요(이 수는 {p}덩어리).")
        }
        ("cohesion", true) => {
            format!("이 수는 맞닿은 내 구슬 쌍이 {p}개로 줄어요(최선 수는 {b}개).")
        }
        ("cohesion", false) => {
            format!("이 수는 상대 구슬끼리 {p}쌍이나 붙게 둬요(최선 수는 {b}쌍).")
        }
        ("isolated", true) => {
            format!("이 수대로면 혼자 떨어진 내 구슬이 {p}개예요(최선 수는 {b}개).")
        }
        ("isolated", false) => {
            format!("최선 수는 상대 구슬 {b}개를 혼자 떨어뜨려요(이 수는 {p}개).")
        }
        ("mobility", true) => {
            format!("이 수대로면 내가 둘 수 있는 수가 {p}개로 줄어요(최선 수는 {b}개).")
        }
        ("mobility", false) => {
            format!("이 수는 상대가 둘 수 있는 수를 {p}개로 늘려 줘요(최선 수는 {b}개).")
        }
        _ => String::new(),
    }
}

/// 등급 머리말 + 이유 문장들.
fn sentences(a: &MoveAnalysis, root: &Position) -> Vec<String> {
    let mut out = Vec::new();
    let (bs, ps) = (a.best.score, a.played.score);
    let plies = |s: i32| WIN - s.abs();
    match a.grade {
        Grade::Best => out.push("가장 좋은 수예요!".to_string()),
        _ if bs >= WIN_BOUND && ps < WIN_BOUND => out.push(format!(
            "큰 실수예요. 최선 수({})로 두면 {}수 안에 이길 수 있었어요.",
            move_to_string(a.best.first),
            plies(bs)
        )),
        _ if ps <= -WIN_BOUND && bs > -WIN_BOUND => out.push(format!(
            "큰 실수예요. 이 수대로 가면 상대가 {}수 안에 이길 수 있어요.",
            plies(ps)
        )),
        Grade::Good if bs.abs() >= WIN_BOUND => {
            out.push("좋은 수예요. 결과는 최선 수와 같아요.".to_string())
        }
        Grade::Good => out.push(format!(
            "좋은 수예요. 최선 수({})와 거의 차이가 없어요.",
            move_to_string(a.best.first)
        )),
        g => out.push(format!(
            "{}예요. 최선 수({})보다 구슬 {:.1}개만큼 손해예요.",
            g.korean(),
            move_to_string(a.best.first),
            a.loss as f64 / 1000.0
        )),
    }
    if a.contributions.is_some() && a.material_diff <= -1 {
        out.push(format!(
            "최선 수는 구슬을 {}개 내주지만 그 대신 더 좋은 자리를 잡아요.",
            -a.material_diff
        ));
    }
    let f = features::compute(root);
    let (now, now_theirs) = (f.side(a.mover), f.side(a.mover.other()));
    for r in &a.reasons {
        let s = reason_sentence(r, now, now_theirs);
        if !s.is_empty() {
            out.push(s);
        }
    }
    if a.material_diff >= 1 {
        if let Some(c) = a.compensations.first() {
            let phrase = compensation_phrase(c);
            if !phrase.is_empty() {
                out.push(format!(
                    "이 수는 구슬을 {}개 내주지만 그 대신 {phrase}.",
                    a.material_diff
                ));
            }
        }
    }
    // 신경망 몫: 특징으로 나눌 수 없는 형세 판단이라 한 덩어리로만 말한다
    let nn_part = a
        .residual
        .filter(|&r| r > 0 && r >= (a.loss * 15 / 100).max(20));
    if let Some(r) = nn_part {
        out.push(format!(
            "정해진 특징 말고도, 엔진이 익힌 형세 판단으로 보면 최선 수가 구슬 {:.1}개만큼 더 좋아요.",
            r as f64 / 1000.0
        ));
    }
    if a.grade != Grade::Best
        && a.reasons.is_empty()
        && nn_part.is_none()
        && a.contributions.is_some()
    {
        out.push("어느 한 가지 이유보다 여러 작은 차이가 모인 결과예요.".to_string());
    }
    out
}

/// 구슬을 내준 대가 문장 조각. `best_value` = 둔 수 끝, `played_value` = 최선 수 끝 (`compensations` 참고).
fn compensation_phrase(r: &Reason) -> String {
    let (p, b) = (r.best_value, r.played_value);
    match (r.key, r.mine_side) {
        ("center_distance", true) => {
            format!("내 구슬이 중앙에 더 모여요(거리 합 {p}, 최선 수는 {b})")
        }
        ("center_distance", false) => {
            format!("상대 구슬을 중앙에서 더 밀어내요(상대 거리 합 {p}, 최선 수는 {b})")
        }
        ("edge", true) => format!("가장자리의 내 구슬이 적어요({p}개, 최선 수는 {b}개)"),
        ("edge", false) => format!("상대 구슬을 가장자리로 더 몰아요({p}개, 최선 수는 {b}개)"),
        ("threat", true) => format!("밀려날 위험이 있는 내 구슬이 적어요({p}개, 최선 수는 {b}개)"),
        ("threat", false) => {
            format!("다음에 밀어낼 수 있는 상대 구슬이 많아요({p}개, 최선 수는 {b}개)")
        }
        ("push_moves", true) => format!("상대를 밀 수 있는 수가 많아요({p}개, 최선 수는 {b}개)"),
        ("push_moves", false) => {
            format!("상대가 나를 밀 수 있는 수가 적어요({p}개, 최선 수는 {b}개)")
        }
        ("triples", true) => format!("3개 줄이 많아요({p}개, 최선 수는 {b}개)"),
        ("triples", false) => format!("상대의 3개 줄이 적어요({p}개, 최선 수는 {b}개)"),
        ("groups", true) => format!("내 구슬이 덜 갈라져요({p}덩어리, 최선 수는 {b}덩어리)"),
        ("groups", false) => format!("상대 구슬을 더 갈라 놓아요({p}덩어리, 최선 수는 {b}덩어리)"),
        ("cohesion", true) => format!("내 구슬이 더 뭉쳐요(맞닿은 쌍 {p}, 최선 수는 {b})"),
        ("isolated", true) => format!("혼자 떨어진 내 구슬이 적어요({p}개, 최선 수는 {b}개)"),
        ("mobility", true) => format!("둘 수 있는 수가 많아요({p}개, 최선 수는 {b}개)"),
        _ => String::new(),
    }
}

/// 힌트: 최선 수와, 지금 국면에서 그 주 변화 끝까지 좋아지는 점.
#[derive(Clone, Debug)]
pub struct Hint {
    pub depth: u32,
    pub best: Line,
    pub reasons: Vec<Reason>,
    pub sentences: Vec<String>,
}

pub fn hint(game: &Game, searcher: &mut Searcher, lim: Limits) -> Option<Hint> {
    if game.is_over() {
        return None;
    }
    let mover = game.pos.side;
    let r = searcher.search(game, lim);
    let best = line_from(&game.pos, mover, r.pv.clone(), r.score);
    let mut sentences = vec![format!(
        "추천 수는 {}예요(초록 화살표).",
        move_to_string(best.first)
    )];
    let mut reasons = Vec::new();
    if r.score >= WIN_BOUND {
        sentences.push(format!("이 수로 {}수 안에 이길 수 있어요!", WIN - r.score));
    } else if r.score > -WIN_BOUND {
        // 지금 국면을 "둔 수 끝", 주 변화 끝을 "최선 끝"으로 놓고 좋아지는 점을 고른다
        let f = features::compute(&game.pos);
        let now_line = Line {
            first: best.first,
            score: eval::evaluate_for(&game.pos, mover, &searcher.weights)
                + if searcher.nn {
                    eval::residual_for(&game.pos, mover)
                } else {
                    0
                },
            pv: Vec::new(),
            end: game.pos,
            mine: *f.side(mover),
            theirs: *f.side(mover.other()),
        };
        let c = eval::contribution_delta(&best.end, &game.pos, mover, &searcher.weights);
        let gain = (best.score - now_line.score).max(0);
        reasons = pick_reasons(&c, &best, &now_line, &searcher.weights, gain);
        reasons.truncate(2);
        for r in &reasons {
            let s = hint_sentence(r);
            if !s.is_empty() {
                sentences.push(s);
            }
        }
    }
    Some(Hint {
        depth: r.depth,
        best,
        reasons,
        sentences,
    })
}

fn hint_sentence(r: &Reason) -> String {
    let (now, end) = (r.played_value, r.best_value);
    match (r.key, r.mine_side) {
        ("pushed_off", true) => format!("상대 구슬을 {}개 밀어낼 수 있어요.", end - now),
        ("pushed_off", false) => "내 구슬을 지킬 수 있어요.".to_string(),
        ("center_distance", true) => format!("내 구슬이 중앙에 가까워져요(거리 합 {now} → {end})."),
        ("center_distance", false) => {
            format!("상대 구슬을 중앙에서 밀어내요(상대 거리 합 {now} → {end}).")
        }
        ("edge", true) => format!("가장자리의 내 구슬이 줄어요({now}개 → {end}개)."),
        ("edge", false) => format!("상대 구슬을 가장자리로 몰아요({now}개 → {end}개)."),
        ("threat", true) => format!("밀려날 위험이 있는 내 구슬이 줄어요({now}개 → {end}개)."),
        ("threat", false) => {
            format!("다음에 밀어낼 수 있는 상대 구슬이 생겨요({now}개 → {end}개).")
        }
        ("push_moves", true) => format!("상대를 밀 수 있는 수가 늘어요({now}개 → {end}개)."),
        ("push_moves", false) => format!("상대가 나를 밀 수 있는 수가 줄어요({now}개 → {end}개)."),
        ("triples", true) => format!("3개 줄이 늘어 미는 힘이 세져요({now}개 → {end}개)."),
        ("triples", false) => format!("상대의 3개 줄이 줄어요({now}개 → {end}개)."),
        ("groups", true) => format!("내 구슬 덩어리가 합쳐져요({now}덩어리 → {end}덩어리)."),
        ("groups", false) => format!("상대 구슬을 갈라 놓아요({now}덩어리 → {end}덩어리)."),
        ("cohesion", true) => format!("내 구슬이 더 뭉쳐요(맞닿은 쌍 {now} → {end})."),
        ("isolated", true) => format!("혼자 떨어진 내 구슬이 줄어요({now}개 → {end}개)."),
        ("mobility", true) => format!("둘 수 있는 수가 늘어요({now}개 → {end}개)."),
        _ => String::new(),
    }
}
