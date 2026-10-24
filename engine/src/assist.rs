//! 조작 보조: 구슬 하나(또는 고른 줄)에서 방향별로 둘 수 있는 수와, 둘 수 없으면 그 이유.
//!
//! UI는 규칙을 판단하지 않고 이 결과만 보여준다. 이유 문장도 여기서 계산한 코드와 숫자로만 만든다.

use crate::board::*;

/// 그 방향으로 못 가는 이유.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blocked {
    /// 내 구슬이 아님
    NotOwn,
    /// 고른 구슬이 한 줄로 붙어 있지 않음
    NotLine,
    /// 한 번에 움직여야 할 내 구슬이 4개 이상
    TooMany,
    /// 내 구슬이 판 밖으로 나가게 됨
    OffBoard,
    /// 미는 쪽(mine)이 밀리는 쪽(theirs)보다 많지 않음
    NotStronger { mine: u8, theirs: u8 },
    /// 상대 줄 바로 뒤에 내 구슬
    OwnBehind,
    /// 옆으로 나란히 갈 칸이 비어 있지 않음
    BroadsideBlocked,
    /// (고른 줄 기준) 바로 앞에 고르지 않은 내 구슬
    OwnInWay,
}

impl Blocked {
    pub fn code(self) -> &'static str {
        match self {
            Blocked::NotOwn => "not_own",
            Blocked::NotLine => "not_line",
            Blocked::TooMany => "too_many",
            Blocked::OffBoard => "off_board",
            Blocked::NotStronger { .. } => "not_stronger",
            Blocked::OwnBehind => "own_behind",
            Blocked::BroadsideBlocked => "broadside_blocked",
            Blocked::OwnInWay => "own_in_way",
        }
    }
}

/// 한 방향의 선택지: 둘 수 있으면 `mv`, 없으면 `blocked`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirOption {
    pub dir: u8,
    pub mv: Option<Move>,
    pub blocked: Option<Blocked>,
}

/// start부터 offset d로 m에 속한 칸이 몇 개 이어지는지 (start 포함).
fn run(m: u128, start: i32, d: i32) -> i32 {
    let mut n = 0;
    while has(m, start + n * d) {
        n += 1;
    }
    n
}

/// 구슬 x 하나를 눌러 방향 d로 밀 때 쓸 수.
///
/// 후보는 x를 포함하는 d 방향 인라인 합법수. x가 맨 뒤인 수(x와 그 앞 구슬들이 함께 감)를
/// 먼저 고르고, 없으면 가장 많은 구슬을 움직이는 수(뒤의 구슬까지 보태서 미는 수)를 고른다.
pub fn single_options(pos: &Position, x: i32) -> [DirOption; 6] {
    let own = pos.own();
    let opp = pos.opp();
    let legal = pos.legal_moves();
    let mut out = [DirOption {
        dir: 0,
        mv: None,
        blocked: Some(Blocked::NotOwn),
    }; 6];
    for (d, o) in out.iter_mut().enumerate() {
        o.dir = d as u8;
        if !has(own, x) {
            continue;
        }
        let mut best: Option<Move> = None;
        for &m in &legal {
            if m.dir as usize != d || !m.is_inline() || !m.cells().any(|s| s == x) {
                continue;
            }
            let rear_is_x = m.head_and_rear().1 == x;
            best = match best {
                None => Some(m),
                Some(b) if b.head_and_rear().1 == x => Some(b),
                Some(_) if rear_is_x => Some(m),
                Some(b) if m.n > b.n => Some(m),
                keep => keep,
            };
        }
        o.mv = best;
        o.blocked = if best.is_some() {
            None
        } else {
            Some(single_block(own, opp, x, DIRS[d]))
        };
    }
    out
}

fn single_block(own: u128, opp: u128, x: i32, dd: i32) -> Blocked {
    let fwd = run(own, x, dd);
    let total = fwd + run(own, x, -dd) - 1;
    if fwd > 3 {
        return Blocked::TooMany;
    }
    let f = x + fwd * dd;
    if !has(VALID, f) {
        return Blocked::OffBoard;
    }
    debug_assert!(
        has(opp, f),
        "front must be an opponent marble when no move exists"
    );
    let k = run(opp, f, dd);
    let mine = total.min(3);
    if k >= mine {
        return Blocked::NotStronger {
            mine: mine as u8,
            theirs: k as u8,
        };
    }
    // 앞의 판정이 모두 통과하면 상대 줄 뒤에 내 구슬이 있는 경우뿐이다
    debug_assert!(has(own, f + k * dd));
    Blocked::OwnBehind
}

/// 고른 구슬 집합(1~3개)을 그대로 움직이는 방향별 선택지.
pub fn group_options(pos: &Position, cells: &[i32]) -> [DirOption; 6] {
    let own = pos.own();
    let opp = pos.opp();
    let mut out = [DirOption {
        dir: 0,
        mv: None,
        blocked: None,
    }; 6];
    for (d, o) in out.iter_mut().enumerate() {
        o.dir = d as u8;
    }
    let fail = |mut out: [DirOption; 6], b: Blocked| {
        for o in out.iter_mut() {
            o.blocked = Some(b);
        }
        out
    };
    if cells.is_empty() || cells.iter().any(|&s| !has(own, s)) {
        return fail(out, Blocked::NotOwn);
    }
    if cells.len() > 3 {
        return fail(out, Blocked::TooMany);
    }
    let mask = cells.iter().fold(0u128, |m, &s| m | bit(s));
    if cells.len() > 1 && line_axis(mask).is_none() {
        return fail(out, Blocked::NotLine);
    }
    let axis = line_axis(mask);
    let n = cells.len() as i32;
    let legal = pos.legal_moves();
    for o in out.iter_mut() {
        let d = o.dir as usize;
        let dd = DIRS[d];
        o.mv = legal
            .iter()
            .copied()
            .find(|m| m.dir as usize == d && m.mask() == mask);
        if o.mv.is_some() {
            continue;
        }
        let inline = axis.is_none_or(|a| d % 3 == a);
        o.blocked = Some(if inline {
            let head = *cells.iter().find(|&&s| !has(mask, s + dd)).unwrap();
            let f = head + dd;
            if !has(VALID, f) {
                Blocked::OffBoard
            } else if has(own, f) {
                if n + run(own, f, dd) > 3 {
                    Blocked::TooMany
                } else {
                    Blocked::OwnInWay
                }
            } else {
                let k = run(opp, f, dd);
                if k >= n {
                    Blocked::NotStronger {
                        mine: n as u8,
                        theirs: k as u8,
                    }
                } else {
                    Blocked::OwnBehind
                }
            }
        } else if cells.iter().any(|&s| !has(VALID, s + dd)) {
            Blocked::OffBoard
        } else {
            Blocked::BroadsideBlocked
        });
    }
    out
}

/// 마스크가 2~3칸의 붙은 일직선이면 그 축(0..3).
fn line_axis(mask: u128) -> Option<usize> {
    let n = mask.count_ones() as i32;
    if !(2..=3).contains(&n) {
        return None;
    }
    let tail = mask.trailing_zeros() as i32;
    (0..3).find(|&a| (0..n).all(|i| has(mask, tail + i * DIRS[a])))
}
