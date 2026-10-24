//! 특징값. 평가함수(가중합)와 설명 생성이 모두 이 값에서만 출발한다.
//!
//! 모든 값은 한쪽 색 기준이다. "다음 수에" 계열 두 값은 둘 차례와 무관하게
//! "그 색이 지금 둔다고 가정"하거나 "상대가 지금 둔다고 가정"해서 계산한다.

use crate::board::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SideFeatures {
    /// 밀어낸 상대 구슬 수
    pub pushed_off: i32,
    /// 내 구슬들의 중앙(E5)까지 육각 거리 합 (작을수록 중앙)
    pub center_distance: i32,
    /// 같은 편끼리 맞닿은 쌍 수
    pub cohesion: i32,
    /// 가장자리(바깥 고리) 위 내 구슬 수
    pub edge: i32,
    /// 상대가 다음 한 수로 판 밖에 밀어낼 수 있는 내 구슬 수
    pub endangered: i32,
    /// 내가 다음 한 수로 판 밖에 밀어낼 수 있는 상대 구슬 수
    pub attackable: i32,
    /// 같은 편 이웃이 하나도 없는 내 구슬 수
    pub isolated: i32,
}

pub const FEATURE_NAMES: [&str; 7] = [
    "pushed_off",
    "center_distance",
    "cohesion",
    "edge",
    "endangered",
    "attackable",
    "isolated",
];

impl SideFeatures {
    pub fn as_array(&self) -> [i32; 7] {
        [
            self.pushed_off,
            self.center_distance,
            self.cohesion,
            self.edge,
            self.endangered,
            self.attackable,
            self.isolated,
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Features {
    pub black: SideFeatures,
    pub white: SideFeatures,
}

impl Features {
    pub fn side(&self, c: Color) -> &SideFeatures {
        match c {
            Color::Black => &self.black,
            Color::White => &self.white,
        }
    }
}

pub fn neighbor_mask(m: u128) -> u128 {
    let mut n = 0;
    for d in 0..6 {
        n |= shift(m, d);
    }
    n & VALID
}

pub fn cohesion_pairs(m: u128) -> i32 {
    // 양의 3방향만 세면 각 쌍을 정확히 한 번 센다
    ((m & shift(m, 0)).count_ones()
        + (m & shift(m, 1)).count_ones()
        + (m & shift(m, 2)).count_ones()) as i32
}

pub fn center_distance_sum(m: u128) -> i32 {
    Bits(m).map(|s| CENTER_DIST[s as usize] as i32).sum()
}

pub fn side_features(pos: &Position, c: Color) -> SideFeatures {
    let own = pos.stones[c.idx()];
    let opp = pos.stones[c.other().idx()];
    SideFeatures {
        pushed_off: pos.pushed_off_by(c) as i32,
        center_distance: center_distance_sum(own),
        cohesion: cohesion_pairs(own),
        edge: (own & EDGE).count_ones() as i32,
        endangered: push_off_targets(opp, own).count_ones() as i32,
        attackable: push_off_targets(own, opp).count_ones() as i32,
        isolated: (own & !neighbor_mask(own)).count_ones() as i32,
    }
}

pub fn compute(pos: &Position) -> Features {
    Features {
        black: side_features(pos, Color::Black),
        white: side_features(pos, Color::White),
    }
}
