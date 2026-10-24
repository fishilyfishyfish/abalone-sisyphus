//! 특징값. 평가함수(가중합)와 설명 생성이 모두 이 값에서만 출발한다.
//!
//! 모든 값은 한쪽 색 기준이다. "다음 수에" 계열 두 값은 둘 차례와 무관하게
//! "그 색이 지금 둔다고 가정"하거나 "상대가 지금 둔다고 가정"해서 계산한다.

use crate::board::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SideFeatures {
    /// 밀어낸 상대 구슬 수
    pub pushed_off: i32,
    /// 내 구슬들의 중앙(E5)까지 육각 거리 합 (작을수록 중앙).
    /// 밀려난 구슬은 거리 `OFF_BOARD_DISTANCE`(판 밖)로 쳐서 더한다
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
    /// 내가 지금 둔다고 가정할 때 상대 구슬을 미는 합법수 개수 (판 밖 밀어내기 포함)
    pub push_moves: i32,
    /// 내 구슬이 일렬로 3개 붙은 곳의 개수 (줄 방향·위치마다 하나씩)
    pub triples: i32,
    /// 서로 이어진 내 구슬 덩어리 개수
    pub groups: i32,
    /// 내가 지금 둔다고 가정할 때 합법수 개수
    pub mobility: i32,
}

pub const NUM_FEATURES: usize = 11;

pub const FEATURE_NAMES: [&str; NUM_FEATURES] = [
    "pushed_off",
    "center_distance",
    "cohesion",
    "edge",
    "endangered",
    "attackable",
    "isolated",
    "push_moves",
    "triples",
    "groups",
    "mobility",
];

impl SideFeatures {
    pub fn as_array(&self) -> [i32; NUM_FEATURES] {
        [
            self.pushed_off,
            self.center_distance,
            self.cohesion,
            self.edge,
            self.endangered,
            self.attackable,
            self.isolated,
            self.push_moves,
            self.triples,
            self.groups,
            self.mobility,
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

/// 일렬로 3개 붙은 곳의 개수. 양의 3방향에서 "s, s−d, s−2d가 모두 내 구슬"인 s를 센다.
pub fn triples(m: u128) -> i32 {
    (0..3)
        .map(|a| {
            let one = shift(m, a);
            (m & one & shift(one, a)).count_ones() as i32
        })
        .sum()
}

/// 서로 이어진 덩어리 개수 (육각 이웃 기준 연결 성분).
pub fn groups(m: u128) -> i32 {
    let mut rest = m;
    let mut n = 0;
    while rest != 0 {
        let mut comp = rest & rest.wrapping_neg();
        loop {
            let next = (comp | neighbor_mask(comp)) & m;
            if next == comp {
                break;
            }
            comp = next;
        }
        rest &= !comp;
        n += 1;
    }
    n
}

/// 밀려난 구슬의 중앙 거리. 판 위 최대 거리(4)보다 한 칸 멀다.
/// 이 값 없이 판 위 구슬만 더하면 가장자리 구슬을 잃을 때 거리 합이 줄어 "좋아진" 것처럼 보인다.
pub const OFF_BOARD_DISTANCE: i32 = 5;

pub fn center_distance_sum(m: u128) -> i32 {
    Bits(m).map(|s| CENTER_DIST[s as usize] as i32).sum()
}

/// 밀어내기 대상 집합을 미리 구해 둔 상태에서 한쪽 특징값을 만든다.
/// `my_targets` = c가 밀어낼 수 있는 상대 구슬, `their_targets` = 상대가 밀어낼 수 있는 c의 구슬.
fn side_from(pos: &Position, c: Color, my_targets: u128, their_targets: u128) -> SideFeatures {
    let own = pos.stones[c.idx()];
    let opp = pos.stones[c.other().idx()];
    let (mobility, push_moves) = count_moves(own, opp);
    SideFeatures {
        pushed_off: pos.pushed_off_by(c) as i32,
        center_distance: center_distance_sum(own)
            + OFF_BOARD_DISTANCE * pos.pushed_off_by(c.other()) as i32,
        cohesion: cohesion_pairs(own),
        edge: (own & EDGE).count_ones() as i32,
        endangered: their_targets.count_ones() as i32,
        attackable: my_targets.count_ones() as i32,
        isolated: (own & !neighbor_mask(own)).count_ones() as i32,
        push_moves,
        triples: triples(own),
        groups: groups(own),
        mobility,
    }
}

pub fn side_features(pos: &Position, c: Color) -> SideFeatures {
    let own = pos.stones[c.idx()];
    let opp = pos.stones[c.other().idx()];
    side_from(
        pos,
        c,
        push_off_targets(own, opp),
        push_off_targets(opp, own),
    )
}

pub fn compute(pos: &Position) -> Features {
    let (b, w) = (pos.stones[0], pos.stones[1]);
    let black_targets = push_off_targets(b, w);
    let white_targets = push_off_targets(w, b);
    Features {
        black: side_from(pos, Color::Black, black_targets, white_targets),
        white: side_from(pos, Color::White, white_targets, black_targets),
    }
}

/// c 기준 특징값 차이 (c의 값 − 상대 값). 평가함수와 기여도 계산의 입력.
pub fn diff(pos: &Position, c: Color) -> [i32; NUM_FEATURES] {
    let f = compute(pos);
    let (me, them) = match c {
        Color::Black => (f.black.as_array(), f.white.as_array()),
        Color::White => (f.white.as_array(), f.black.as_array()),
    };
    std::array::from_fn(|i| me[i] - them[i])
}
