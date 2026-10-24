//! 판 표현, 합법수 생성, 수 적용/되돌리기.
//!
//! 좌표계: 행 r = 0..8 (A..I, A가 흑 쪽 아래), 사선 c = 0..8 (1..9).
//! 칸 (r, c)는 |r - c| <= 4일 때 판 위에 있다 (61칸).
//! 11×11 패딩 격자에서 칸 번호는 `(r + 1) * 11 + (c + 1)`이고,
//! 판 밖 칸은 비트가 절대 켜지지 않으므로 한 칸 이동 결과가 판 밖이면 `VALID` 검사로 걸러진다.

pub const GRID: i32 = 11;
pub const NUM_CELLS: usize = 61;
pub const WIN_PUSHED: u32 = 6;

/// 6방향. 반대 방향은 `(d + 3) % 6`. 0..3이 줄 방향(축)으로 쓰는 양의 방향이다.
/// 0 E (0,+1), 1 NE (+1,+1), 2 NW (+1,0), 3 W (0,-1), 4 SW (-1,-1), 5 SE (-1,0)
pub const DIRS: [i32; 6] = [1, 12, 11, -1, -12, -11];
pub const DIR_RC: [(i32, i32); 6] = [(0, 1), (1, 1), (1, 0), (0, -1), (-1, -1), (-1, 0)];
pub const DIR_NAMES: [&str; 6] = ["E", "NE", "NW", "W", "SW", "SE"];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    Black = 0,
    White = 1,
}

impl Color {
    #[inline]
    pub fn other(self) -> Color {
        match self {
            Color::Black => Color::White,
            Color::White => Color::Black,
        }
    }
    #[inline]
    pub fn idx(self) -> usize {
        self as usize
    }
}

#[inline]
pub const fn sq(r: i32, c: i32) -> i32 {
    (r + 1) * GRID + (c + 1)
}

#[inline]
pub const fn rc(s: i32) -> (i32, i32) {
    (s / GRID - 1, s % GRID - 1)
}

pub const fn on_board_rc(r: i32, c: i32) -> bool {
    r >= 0 && r <= 8 && c >= 0 && c <= 8 && r - c <= 4 && c - r <= 4
}

const fn build_valid() -> u128 {
    let mut m = 0u128;
    let mut r = 0;
    while r < 9 {
        let mut c = 0;
        while c < 9 {
            if on_board_rc(r, c) {
                m |= 1u128 << sq(r, c);
            }
            c += 1;
        }
        r += 1;
    }
    m
}

/// 판 위 61칸 마스크.
pub const VALID: u128 = build_valid();

/// 정규 순서(행 오름차순, 사선 오름차순)의 칸 번호 → 패딩 칸 번호.
pub const CELLS: [u8; NUM_CELLS] = {
    let mut out = [0u8; NUM_CELLS];
    let mut i = 0;
    let mut r = 0;
    while r < 9 {
        let mut c = 0;
        while c < 9 {
            if on_board_rc(r, c) {
                out[i] = sq(r, c) as u8;
                i += 1;
            }
            c += 1;
        }
        r += 1;
    }
    out
};

/// 패딩 칸 번호 → 정규 칸 번호 (판 밖은 255).
pub const CELL_INDEX: [u8; 121] = {
    let mut out = [255u8; 121];
    let mut i = 0;
    while i < NUM_CELLS {
        out[CELLS[i] as usize] = i as u8;
        i += 1;
    }
    out
};

/// 중앙(E5)까지 육각 거리. 판 밖은 0.
pub const CENTER_DIST: [u8; 121] = {
    let mut out = [0u8; 121];
    let mut i = 0;
    while i < NUM_CELLS {
        let s = CELLS[i] as i32;
        let (r, c) = rc(s);
        let dr = r - 4;
        let dc = c - 4;
        let a = if dr < 0 { -dr } else { dr };
        let b = if dc < 0 { -dc } else { dc };
        let e = if dr - dc < 0 { dc - dr } else { dr - dc };
        let mut m = a;
        if b > m {
            m = b;
        }
        if e > m {
            m = e;
        }
        out[s as usize] = m as u8;
        i += 1;
    }
    out
};

/// 가장자리(중앙 거리 4) 칸 마스크.
pub const EDGE: u128 = {
    let mut m = 0u128;
    let mut i = 0;
    while i < NUM_CELLS {
        let s = CELLS[i] as usize;
        if CENTER_DIST[s] == 4 {
            m |= 1u128 << s;
        }
        i += 1;
    }
    m
};

#[inline]
pub fn bit(s: i32) -> u128 {
    1u128 << s
}

#[inline]
pub fn has(m: u128, s: i32) -> bool {
    (m >> s) & 1 == 1
}

/// 비트 집합을 칸 번호로 순회.
pub struct Bits(pub u128);

impl Iterator for Bits {
    type Item = i32;
    #[inline]
    fn next(&mut self) -> Option<i32> {
        if self.0 == 0 {
            None
        } else {
            let s = self.0.trailing_zeros() as i32;
            self.0 &= self.0 - 1;
            Some(s)
        }
    }
}

/// 방향 d로 마스크 전체를 한 칸 민다 (판 밖 비트는 그대로 남을 수 있으니 필요하면 VALID와 AND).
#[inline]
pub fn shift(m: u128, d: usize) -> u128 {
    let o = DIRS[d];
    if o > 0 {
        m << o
    } else {
        m >> (-o)
    }
}

// ---------------------------------------------------------------------------
// 조브리스트 해시 (컴파일 타임 splitmix64)

const fn splitmix(state: u64) -> (u64, u64) {
    let s = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (s, z ^ (z >> 31))
}

pub const ZOBRIST: [[u64; 121]; 2] = {
    let mut t = [[0u64; 121]; 2];
    let mut st = 0xABA1_0E5E_ED00_0001u64;
    let mut c = 0;
    while c < 2 {
        let mut s = 0;
        while s < 121 {
            let (ns, v) = splitmix(st);
            st = ns;
            t[c][s] = v;
            s += 1;
        }
        c += 1;
    }
    t
};

pub const ZOBRIST_SIDE: u64 = splitmix(0x5151_5151_5151_5151).1;

fn hash_mask(m: u128, c: usize) -> u64 {
    let mut h = 0;
    for s in Bits(m) {
        h ^= ZOBRIST[c][s as usize];
    }
    h
}

// ---------------------------------------------------------------------------
// 수

/// 한 수. 줄은 `tail`에서 축 `axis` 방향으로 `n`칸 (n = 1..=3), 이동 방향은 `dir`.
/// n = 1이면 axis = 0으로 정규화한다. 같은 수는 표현이 하나뿐이다.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Move {
    pub tail: u8,
    pub n: u8,
    pub axis: u8,
    pub dir: u8,
}

impl Move {
    #[inline]
    pub fn is_inline(&self) -> bool {
        self.n == 1 || self.dir % 3 == self.axis
    }

    /// 움직이는 내 구슬 칸들 (tail부터 축 방향 순서).
    pub fn cells(&self) -> impl Iterator<Item = i32> {
        let t = self.tail as i32;
        let a = DIRS[self.axis as usize];
        (0..self.n as i32).map(move |i| t + i * a)
    }

    pub fn mask(&self) -> u128 {
        self.cells().fold(0, |m, s| m | bit(s))
    }

    /// 인라인 수에서 이동 방향으로 가장 앞 구슬 / 가장 뒤 구슬.
    pub fn head_and_rear(&self) -> (i32, i32) {
        let t = self.tail as i32;
        let last = t + (self.n as i32 - 1) * DIRS[self.axis as usize];
        if self.n == 1 || self.dir == self.axis {
            (last, t)
        } else {
            (t, last)
        }
    }
}

/// 되돌리기 정보: 두 색의 XOR 마스크와 이전 해시.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Undo {
    pub own_xor: u128,
    pub opp_xor: u128,
    pub hash: u64,
}

impl Undo {
    /// 판 안 밀기는 상대 비트 2개, 밀어내기는 1개를 바꾼다.
    #[inline]
    pub fn pushed_off(&self) -> bool {
        self.opp_xor.count_ones() == 1
    }
}

/// 인라인 이동 결과.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlineResult {
    /// 앞이 빈칸
    Step,
    /// 상대 구슬 k개를 판 안에서 민다
    Push(u8),
    /// 상대 구슬 k개를 밀어 맨 끝 하나가 판 밖으로
    PushOff(u8),
}

/// `head`가 앞장선 내 구슬 n개의 줄이 방향 offset `d`로 움직일 수 있는지.
#[inline]
pub fn inline_result(own: u128, opp: u128, head: i32, d: i32, n: i32) -> Option<InlineResult> {
    let f = head + d;
    if !has(VALID, f) || has(own, f) {
        return None;
    }
    if !has(opp, f) {
        return Some(InlineResult::Step);
    }
    // 상대 줄 길이 k (k >= n이면 불가)
    let mut k = 1;
    loop {
        if k >= n {
            return None;
        }
        let c = f + k * d;
        if has(opp, c) {
            k += 1;
        } else {
            break;
        }
    }
    let b = f + k * d;
    if has(own, b) {
        None
    } else if has(VALID, b) {
        Some(InlineResult::Push(k as u8))
    } else {
        Some(InlineResult::PushOff(k as u8))
    }
}

/// own 쪽 합법수 전부를 out에 추가한다.
pub fn gen_moves(own: u128, opp: u128, out: &mut Vec<Move>) {
    let empty = VALID & !(own | opp);
    for s in Bits(own) {
        for d in 0..6u8 {
            if has(empty, s + DIRS[d as usize]) {
                out.push(Move {
                    tail: s as u8,
                    n: 1,
                    axis: 0,
                    dir: d,
                });
            }
        }
        for a in 0..3u8 {
            let da = DIRS[a as usize];
            if !has(own, s + da) {
                continue;
            }
            for n in 2..=3i32 {
                if n == 3 && !has(own, s + 2 * da) {
                    break;
                }
                let last = s + (n - 1) * da;
                if inline_result(own, opp, last, da, n).is_some() {
                    out.push(Move {
                        tail: s as u8,
                        n: n as u8,
                        axis: a,
                        dir: a,
                    });
                }
                if inline_result(own, opp, s, -da, n).is_some() {
                    out.push(Move {
                        tail: s as u8,
                        n: n as u8,
                        axis: a,
                        dir: a + 3,
                    });
                }
                for d in 0..6u8 {
                    if d % 3 == a {
                        continue;
                    }
                    let dd = DIRS[d as usize];
                    if (0..n).all(|i| has(empty, s + i * da + dd)) {
                        out.push(Move {
                            tail: s as u8,
                            n: n as u8,
                            axis: a,
                            dir: d,
                        });
                    }
                }
            }
        }
    }
}

/// own 쪽이 지금 둘 수 있다고 가정할 때, 한 수로 판 밖에 밀어낼 수 있는 opp 구슬 집합.
pub fn push_off_targets(own: u128, opp: u128) -> u128 {
    let mut targets = 0u128;
    for s in Bits(own) {
        for dd in DIRS {
            // s가 줄의 맨 앞(head)인 경우만 본다: 뒤로 2, 3개 줄
            for n in 2..=3i32 {
                if !(1..n).all(|i| has(own, s - i * dd)) {
                    break;
                }
                if let Some(InlineResult::PushOff(k)) = inline_result(own, opp, s, dd, n) {
                    targets |= bit(s + k as i32 * dd);
                }
            }
        }
    }
    targets
}

/// own 쪽 합법수 개수와, 그중 상대 구슬을 미는 수(판 밖 밀어내기 포함) 개수.
/// `gen_moves`와 같은 순회를 하되 수를 만들지 않고 센다 (평가함수용).
pub fn count_moves(own: u128, opp: u128) -> (i32, i32) {
    let empty = VALID & !(own | opp);
    let (mut total, mut pushes) = (0, 0);
    for s in Bits(own) {
        for dd in DIRS {
            if has(empty, s + dd) {
                total += 1;
            }
        }
        for (a, &da) in DIRS[..3].iter().enumerate() {
            if !has(own, s + da) {
                continue;
            }
            for n in 2..=3i32 {
                if n == 3 && !has(own, s + 2 * da) {
                    break;
                }
                for (head, dd) in [(s + (n - 1) * da, da), (s, -da)] {
                    match inline_result(own, opp, head, dd, n) {
                        Some(InlineResult::Step) => total += 1,
                        Some(_) => {
                            total += 1;
                            pushes += 1;
                        }
                        None => {}
                    }
                }
                for (d, &dd) in DIRS.iter().enumerate() {
                    if d % 3 != a && (0..n).all(|i| has(empty, s + i * da + dd)) {
                        total += 1;
                    }
                }
            }
        }
    }
    (total, pushes)
}

/// own 쪽 합법수 중 상대 구슬을 판 밖으로 밀어내는 수만 out에 추가한다 (정지 탐색용).
pub fn gen_push_offs(own: u128, opp: u128, out: &mut Vec<Move>) {
    for s in Bits(own) {
        for d in 0..6u8 {
            let dd = DIRS[d as usize];
            // s가 줄의 맨 앞(head)
            for n in 2..=3i32 {
                if !(1..n).all(|i| has(own, s - i * dd)) {
                    break;
                }
                if let Some(InlineResult::PushOff(_)) = inline_result(own, opp, s, dd, n) {
                    let (axis, tail) = if d < 3 {
                        (d, s - (n - 1) * dd)
                    } else {
                        (d - 3, s)
                    };
                    out.push(Move {
                        tail: tail as u8,
                        n: n as u8,
                        axis,
                        dir: d,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 국면

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Position {
    pub stones: [u128; 2],
    pub side: Color,
    /// pushed[c] = c가 판 밖으로 밀어낸 상대 구슬 수
    pub pushed: [u8; 2],
    pub hash: u64,
}

impl Position {
    pub fn new(black: u128, white: u128, side: Color) -> Position {
        assert_eq!(black & white, 0, "overlapping stones");
        assert_eq!((black | white) & !VALID, 0, "stone off board");
        let mut p = Position {
            stones: [black, white],
            side,
            pushed: [0, 0],
            hash: 0,
        };
        p.hash = p.compute_hash();
        p
    }

    pub fn compute_hash(&self) -> u64 {
        let mut h = hash_mask(self.stones[0], 0) ^ hash_mask(self.stones[1], 1);
        if self.side == Color::White {
            h ^= ZOBRIST_SIDE;
        }
        h
    }

    #[inline]
    pub fn own(&self) -> u128 {
        self.stones[self.side.idx()]
    }

    #[inline]
    pub fn opp(&self) -> u128 {
        self.stones[self.side.other().idx()]
    }

    #[inline]
    pub fn count(&self, c: Color) -> u32 {
        self.stones[c.idx()].count_ones()
    }

    /// 밀어낸 수를 지정한 국면 (테스트, 기보 중간 국면용).
    pub fn with_pushed(mut self, black: u8, white: u8) -> Position {
        self.pushed = [black, white];
        self
    }

    /// c가 지금까지 판 밖으로 밀어낸 상대 구슬 수.
    #[inline]
    pub fn pushed_off_by(&self, c: Color) -> u32 {
        self.pushed[c.idx()] as u32
    }

    pub fn at(&self, s: i32) -> Option<Color> {
        if has(self.stones[0], s) {
            Some(Color::Black)
        } else if has(self.stones[1], s) {
            Some(Color::White)
        } else {
            None
        }
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        let mut v = Vec::with_capacity(128);
        gen_moves(self.own(), self.opp(), &mut v);
        v
    }

    pub fn legal_moves_into(&self, out: &mut Vec<Move>) {
        gen_moves(self.own(), self.opp(), out);
    }

    /// 수 적용. 합법성은 검사하지 않는다 (합법수 목록에서 고른 수만 넣을 것).
    pub fn make_move(&mut self, m: Move) -> Undo {
        let me = self.side.idx();
        let them = 1 - me;
        let own = self.stones[me];
        let opp = self.stones[them];
        let d = DIRS[m.dir as usize];
        let mut own_xor = 0u128;
        let mut opp_xor = 0u128;
        if m.is_inline() {
            let (head, rear) = m.head_and_rear();
            own_xor = bit(rear) | bit(head + d);
            match inline_result(own, opp, head, d, m.n as i32) {
                Some(InlineResult::Step) => {}
                Some(InlineResult::Push(k)) => {
                    opp_xor = bit(head + d) | bit(head + d + k as i32 * d);
                }
                Some(InlineResult::PushOff(_)) => {
                    opp_xor = bit(head + d);
                }
                None => debug_assert!(false, "illegal move {:?}", m),
            }
        } else {
            for s in m.cells() {
                own_xor |= bit(s) | bit(s + d);
            }
            // 줄 칸과 도착 칸은 서로 겹치지 않는다 (브로드사이드)
        }
        let undo = Undo {
            own_xor,
            opp_xor,
            hash: self.hash,
        };
        self.stones[me] ^= own_xor;
        self.stones[them] ^= opp_xor;
        if undo.pushed_off() {
            self.pushed[me] += 1;
        }
        self.hash ^= hash_mask(own_xor, me) ^ hash_mask(opp_xor, them) ^ ZOBRIST_SIDE;
        self.side = self.side.other();
        undo
    }

    pub fn unmake_move(&mut self, u: Undo) {
        self.side = self.side.other();
        let me = self.side.idx();
        self.stones[me] ^= u.own_xor;
        self.stones[1 - me] ^= u.opp_xor;
        if u.pushed_off() {
            self.pushed[me] -= 1;
        }
        self.hash = u.hash;
    }

    /// 인라인 2~3개 수의 밀기 결과 (그 외의 수는 None).
    pub fn inline_kind(&self, m: Move) -> Option<InlineResult> {
        if !m.is_inline() || m.n == 1 {
            return None;
        }
        let (head, _) = m.head_and_rear();
        inline_result(
            self.own(),
            self.opp(),
            head,
            DIRS[m.dir as usize],
            m.n as i32,
        )
    }

    /// 이 수가 상대 구슬을 판 밖으로 밀어내는지.
    pub fn is_push_off(&self, m: Move) -> bool {
        if !m.is_inline() || m.n == 1 {
            return false;
        }
        let (head, _) = m.head_and_rear();
        matches!(
            inline_result(
                self.own(),
                self.opp(),
                head,
                DIRS[m.dir as usize],
                m.n as i32
            ),
            Some(InlineResult::PushOff(_))
        )
    }
}
