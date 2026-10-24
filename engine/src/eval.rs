//! 평가함수: 특징값 차이의 가중합.
//!
//! `evaluate(pos) = Σ w[i] × (f[i](둘 차례) − f[i](상대))`. 선형이라 기여도가 정확히 나뉜다.
//! 한 국면의 기여도는 `w[i] × diff[i]`이고, 두 국면(예: 둔 수와 최선 수의 주 변화 끝)의
//! 기여도 차이는 `w[i] × (diff_a[i] − diff_b[i])`다. 합은 평가 차이와 정확히 같다.

use crate::board::{Color, Position};
use crate::features::{self, FEATURE_NAMES};

pub const NUM_FEATURES: usize = 7;

/// 승리 점수. 탐색에서는 `WIN − 수순 길이`로 써서 빠른 승리를 고른다.
pub const WIN: i32 = 1_000_000;
/// |점수|가 이 이상이면 승패가 정해진 점수.
pub const WIN_BOUND: i32 = WIN - 1_000;

/// 특징값 순서(`FEATURE_NAMES`)대로의 가중치.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weights(pub [i32; NUM_FEATURES]);

/// 손으로 정한 초기값. 4단계 자기대국 튜닝에서 바뀐다.
/// - pushed_off 1000: 구슬 하나 = 1000점이 기준 단위
/// - center_distance −20: 중앙에서 한 칸 멀어질 때마다
/// - cohesion +10: 맞닿은 쌍 하나
/// - edge −15: 가장자리 구슬 하나
/// - endangered −75, attackable +75: 두 값은 차이로 보면 부호만 반대(내 attackable = 상대 endangered)라
///   합쳐서 "다음 수 밀어내기 위협 1개당 150점" 한 항목처럼 작동한다. 설명 층은 둘을 한 쌍으로 다룬다
/// - isolated −30: 고립 구슬 하나
pub const DEFAULT_WEIGHTS: Weights = Weights([1000, -20, 10, -15, -75, 75, -30]);

/// 둘 차례 쪽 관점의 평가값 (승패 판정은 포함하지 않음).
pub fn evaluate(pos: &Position, w: &Weights) -> i32 {
    score(&features::diff(pos, pos.side), w)
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
