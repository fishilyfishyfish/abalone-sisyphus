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
/// mobility의 특징 번호
pub const MOBILITY: usize = 10;

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

/// own 쪽이 지금 둔다고 가정할 때 상대 구슬을 미는 수의 개수와, 판 밖으로 밀어낼 수 있는 상대 구슬 집합.
/// 미는 수는 (줄 맨 앞 구슬, 방향, 길이)로 하나씩 정해진다. 방향마다 "맨 앞 구슬" 집합을 비트 연산으로
/// 한꺼번에 구한다(2:1, 3:1, 3:2 세 경우). 각 경우의 집합 비트 하나가 미는 수 하나다.
/// 줄을 이루는 칸이 모두 판 위라 한 칸 앞은 늘 패딩 격자 안(0..=120)이어서, 시프트로 잃는 비트가 없다.
pub fn push_stats(own: u128, opp: u128) -> (i32, u128) {
    let free = !(own | opp); // 빈칸이거나 판 밖
    let off = !VALID;
    // 시프트 양을 상수로 두려고 방향마다 따로 펼친다 (u128 가변 시프트는 느리다)
    let r = [
        push_dir::<1>(own, opp, free, off),
        push_dir::<12>(own, opp, free, off),
        push_dir::<11>(own, opp, free, off),
        push_dir::<{ -1 }>(own, opp, free, off),
        push_dir::<{ -12 }>(own, opp, free, off),
        push_dir::<{ -11 }>(own, opp, free, off),
    ];
    r.iter().fold((0, 0), |(c, t), &(c2, t2)| (c + c2, t | t2))
}

/// 패딩 칸 번호 오프셋 O 방향으로 미는 수 (`push_stats` 참고).
#[inline(always)]
fn push_dir<const O: i32>(own: u128, opp: u128, free: u128, off: u128) -> (i32, u128) {
    // fwd(m): 각 비트를 O만큼 옮긴다. ahead(m): 한 칸 앞(O 방향)이 m인 칸들
    let fwd = |m: u128| if O > 0 { m << O } else { m >> (-O) };
    let ahead = |m: u128| if O > 0 { m >> O } else { m << (-O) };
    let own2 = own & fwd(own); // 맨 앞과 그 뒤 1칸이 내 구슬
    let own3 = own2 & fwd(fwd(own)); // 뒤 2칸까지
    let o1 = ahead(opp); // 바로 앞이 상대
    let o2 = o1 & ahead(ahead(opp)); // 앞 2칸이 상대
    let f2 = ahead(ahead(free));
    let x2 = ahead(ahead(off));
    // 상대 1개(그 뒤가 비거나 판 밖): 내 줄 2개 또는 3개
    let one = o1 & f2;
    let p21 = own2 & one;
    let p31 = own3 & one;
    // 상대 2개: 내 줄 3개
    let p32 = own3 & o2 & ahead(f2);
    let count = (p21.count_ones() + p31.count_ones() + p32.count_ones()) as i32;
    // 판 밖으로 밀리는 구슬: 상대 줄의 마지막 구슬
    let targets = fwd((p21 | p31) & x2) | fwd(fwd(p32 & ahead(x2)));
    (count, targets)
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
        let mut comp = 1u128 << rest.trailing_zeros();
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
    // 거리별 고리 마스크로 센다 (칸마다 표를 찾는 것보다 빠르다)
    (1..5)
        .map(|k| k * (m & RINGS[k as usize]).count_ones() as i32)
        .sum()
}

/// 한쪽 특징값. `pushes`/`my_targets` = c의 밀기 수와 밀어낼 수 있는 상대 구슬,
/// `their_targets` = 상대가 밀어낼 수 있는 c의 구슬. `with_mobility`가 거짓이면 mobility는 0으로 둔다
/// (가중치가 0일 때 평가 속도를 위해서. 표시·설명용 `compute`는 항상 센다).
fn side_from(
    pos: &Position,
    c: Color,
    pushes: i32,
    my_targets: u128,
    their_targets: u128,
    with_mobility: bool,
) -> SideFeatures {
    let own = pos.stones[c.idx()];
    let opp = pos.stones[c.other().idx()];
    SideFeatures {
        pushed_off: pos.pushed_off_by(c) as i32,
        center_distance: center_distance_sum(own)
            + OFF_BOARD_DISTANCE * pos.pushed_off_by(c.other()) as i32,
        cohesion: cohesion_pairs(own),
        edge: (own & EDGE).count_ones() as i32,
        endangered: their_targets.count_ones() as i32,
        attackable: my_targets.count_ones() as i32,
        isolated: (own & !neighbor_mask(own)).count_ones() as i32,
        push_moves: pushes,
        triples: triples(own),
        groups: groups(own),
        mobility: if with_mobility {
            count_moves(own, opp).0
        } else {
            0
        },
    }
}

pub fn side_features(pos: &Position, c: Color) -> SideFeatures {
    let f = compute(pos);
    *f.side(c)
}

pub fn compute(pos: &Position) -> Features {
    compute_opts(pos, true)
}

/// `with_mobility`가 거짓이면 두 쪽 mobility를 0으로 둔다 (평가함수 빠른 경로).
pub fn compute_opts(pos: &Position, with_mobility: bool) -> Features {
    let (b, w) = (pos.stones[0], pos.stones[1]);
    let (bp, bt) = push_stats(b, w);
    let (wp, wt) = push_stats(w, b);
    Features {
        black: side_from(pos, Color::Black, bp, bt, wt, with_mobility),
        white: side_from(pos, Color::White, wp, wt, bt, with_mobility),
    }
}

/// c 기준 특징값 차이 (c의 값 − 상대 값). 평가함수와 기여도 계산의 입력.
pub fn diff(pos: &Position, c: Color) -> [i32; NUM_FEATURES] {
    diff_opts(pos, c, true)
}

/// mobility 계산을 고를 수 있는 `diff` (`compute_opts` 참고).
pub fn diff_opts(pos: &Position, c: Color, with_mobility: bool) -> [i32; NUM_FEATURES] {
    let f = compute_opts(pos, with_mobility);
    let (me, them) = match c {
        Color::Black => (f.black.as_array(), f.white.as_array()),
        Color::White => (f.white.as_array(), f.black.as_array()),
    };
    std::array::from_fn(|i| me[i] - them[i])
}
