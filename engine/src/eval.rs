//! 평가함수: 특징값 차이의 가중합.
//!
//! `evaluate(pos) = Σ w[i] × (f[i](둘 차례) − f[i](상대))`. 선형이라 기여도가 정확히 나뉜다.
//! 한 국면의 기여도는 `w[i] × diff[i]`이고, 두 국면(예: 둔 수와 최선 수의 주 변화 끝)의
//! 기여도 차이는 `w[i] × (diff_a[i] − diff_b[i])`다. 합은 평가 차이와 정확히 같다.

use crate::board::{Color, Position};
pub use crate::features::NUM_FEATURES;
use crate::features::{self, FEATURE_NAMES};

/// 승리 점수. 탐색에서는 `WIN − 수순 길이`로 써서 빠른 승리를 고른다.
pub const WIN: i32 = 1_000_000;
/// |점수|가 이 이상이면 승패가 정해진 점수.
pub const WIN_BOUND: i32 = WIN - 1_000;

/// 특징값 순서(`FEATURE_NAMES`)대로의 가중치.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weights(pub [i32; NUM_FEATURES]);

/// v0: 3단계에서 손으로 정한 초기값.
/// - pushed_off 1000: 구슬 하나 = 1000점이 기준 단위
/// - center_distance −20: 중앙에서 한 칸 멀어질 때마다
/// - cohesion +10: 맞닿은 쌍 하나
/// - edge −15: 가장자리 구슬 하나
/// - endangered −75, attackable +75: 두 값은 차이로 보면 부호만 반대(내 attackable = 상대 endangered)라
///   합쳐서 "다음 수 밀어내기 위협 1개당 150점" 한 항목처럼 작동한다. 설명 층은 둘을 한 쌍으로 다룬다
/// - isolated −30: 고립 구슬 하나
pub const WEIGHTS_V0: Weights = Weights([1000, -20, 10, -15, -75, 75, -30, 0, 0, 0, 0]);

/// v1: v0 자기대국(깊이 3, 3000판, 조용한 국면 33.6만 개) 텍셀 튜닝, 부호 제약(가장자리 0으로 묶임).
/// center_distance는 밀려난 구슬을 거리 5로 치는 정의 기준이다.
pub const WEIGHTS_V1: Weights = Weights([1000, -141, 106, 0, -77, 77, -33, 0, 0, 0, 0]);

/// v2: 특징 11개(밀기 기회·3개 줄·덩어리 수·움직임 추가). v1 자기대국(깊이 3, 3000판,
/// 조용한 국면 45만 개) 텍셀 튜닝, 부호 제약(뭉침·고립·움직임이 0으로 묶임).
pub const WEIGHTS_V2: Weights = Weights([1000, -123, 0, -17, -28, 28, 0, 39, 55, -15, 0]);

/// 엔진이 쓰는 가중치.
pub const DEFAULT_WEIGHTS: Weights = WEIGHTS_V2;

/// 이름("v0", "default")이나 "w:" 뒤 쉼표 목록으로 가중치를 고른다. 측정·튜닝 도구용.
/// 목록이 특징 수보다 짧으면 나머지는 0이다 (예: 예전 7개짜리 목록).
pub fn weights_by_name(name: &str) -> Option<Weights> {
    match name {
        "v0" => Some(WEIGHTS_V0),
        "v1" => Some(WEIGHTS_V1),
        "v2" => Some(WEIGHTS_V2),
        "default" => Some(DEFAULT_WEIGHTS),
        _ => {
            let list = name.strip_prefix("w:")?;
            let v: Vec<i32> = list
                .split(',')
                .map(|x| x.trim().parse().ok())
                .collect::<Option<_>>()?;
            if v.len() > NUM_FEATURES {
                return None;
            }
            let mut w = [0; NUM_FEATURES];
            w[..v.len()].copy_from_slice(&v);
            Some(Weights(w))
        }
    }
}

/// 둘 차례 쪽 관점의 평가값 (승패 판정은 포함하지 않음).
///
/// 탐색에서 가장 많이 부르는 함수라, `score(diff)`와 정확히 같은 값을 특징값 구조체 없이 바로 센다.
/// 가중치가 0인 특징은 세지 않는다. 같은 값인지는 `tests/search.rs`가 무작위 국면·여러 가중치로 확인한다.
pub fn evaluate(pos: &Position, w: &Weights) -> i32 {
    use crate::board::{count_moves, EDGE};
    use features::*;
    let w = &w.0;
    let me = pos.side.idx();
    let them = 1 - me;
    let (own, opp) = (pos.stones[me], pos.stones[them]);
    let (pm, pt) = (pos.pushed[me] as i32, pos.pushed[them] as i32);
    let cnt = |m: u128| m.count_ones() as i32;
    let mut s = w[0] * (pm - pt);
    if w[1] != 0 {
        // 밀려난 구슬은 거리 OFF_BOARD_DISTANCE로 친다: 내 쪽은 상대가 밀어낸 수(pt)만큼
        s += w[1]
            * (center_distance_sum(own) + OFF_BOARD_DISTANCE * pt
                - center_distance_sum(opp)
                - OFF_BOARD_DISTANCE * pm);
    }
    if w[2] != 0 {
        s += w[2] * (cohesion_pairs(own) - cohesion_pairs(opp));
    }
    if w[3] != 0 {
        s += w[3] * (cnt(own & EDGE) - cnt(opp & EDGE));
    }
    if w[4] != 0 || w[5] != 0 || w[7] != 0 {
        let (my_pushes, my_targets) = push_stats(own, opp);
        let (their_pushes, their_targets) = push_stats(opp, own);
        // endangered(나) = 상대가 밀어낼 수 있는 내 구슬, attackable(나) = 내가 밀어낼 수 있는 상대 구슬
        let threat = cnt(their_targets) - cnt(my_targets);
        s += w[4] * threat - w[5] * threat + w[7] * (my_pushes - their_pushes);
    }
    if w[6] != 0 {
        s += w[6] * (cnt(own & !neighbor_mask(own)) - cnt(opp & !neighbor_mask(opp)));
    }
    if w[8] != 0 {
        s += w[8] * (triples(own) - triples(opp));
    }
    if w[9] != 0 {
        s += w[9] * (groups(own) - groups(opp));
    }
    if w[10] != 0 {
        s += w[10] * (count_moves(own, opp).0 - count_moves(opp, own).0);
    }
    s
}

/// 보정 신경망을 더한 평가값(둘 차례 관점) = 선형 가중합 + `nn::residual`.
pub fn evaluate_nn(pos: &Position, w: &Weights) -> i32 {
    let d = features::diff_opts(pos, pos.side, w.0[features::MOBILITY] != 0);
    score(&d, w) + crate::nn::residual(pos, &d)
}

/// c 관점 평가값.
pub fn evaluate_for(pos: &Position, c: Color, w: &Weights) -> i32 {
    score(&features::diff(pos, c), w)
}

pub fn score(diff: &[i32; NUM_FEATURES], w: &Weights) -> i32 {
    diff.iter().zip(w.0.iter()).map(|(d, w)| d * w).sum()
}

/// c 관점 기여도: `w[i] × diff[i]`. 합 = `evaluate_for(pos, c)`.
pub fn contributions(pos: &Position, c: Color, w: &Weights) -> [i32; NUM_FEATURES] {
    let d = features::diff(pos, c);
    std::array::from_fn(|i| w.0[i] * d[i])
}

/// 두 국면의 c 관점 기여도 차이 (a − b). 합 = `evaluate_for(a, c) − evaluate_for(b, c)`.
pub fn contribution_delta(
    a: &Position,
    b: &Position,
    c: Color,
    w: &Weights,
) -> [i32; NUM_FEATURES] {
    let (ca, cb) = (contributions(a, c, w), contributions(b, c, w));
    std::array::from_fn(|i| ca[i] - cb[i])
}

pub fn feature_name(i: usize) -> &'static str {
    FEATURE_NAMES[i]
}
