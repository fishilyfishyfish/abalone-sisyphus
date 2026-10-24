//! 공식 좌표 표기 (행 A~I, 사선 1~9)와 수 표기.
//!
//! 수 표기 규칙 (이 프로젝트 기준):
//! - 인라인(1개 이동 포함): `<맨 뒤 구슬><그 구슬의 도착 칸>` 예: `A1B2`.
//!   줄 길이는 맨 뒤 구슬에서 이동 방향으로 이어진 내 구슬 수로 정해지므로 모호하지 않다.
//! - 브로드사이드: `<줄 한쪽 끝><다른 끝><첫 칸의 도착 칸>` 예: `A1A3B1`.
//!   끝 두 칸은 패딩 칸 번호 오름차순.

use crate::board::*;

pub fn cell_name(s: i32) -> String {
    let (r, c) = rc(s);
    format!("{}{}", (b'A' + r as u8) as char, c + 1)
}

pub fn parse_cell(t: &str) -> Option<i32> {
    let b = t.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let r = (b[0].to_ascii_uppercase() as i32) - ('A' as i32);
    let c = (b[1] as i32) - ('1' as i32);
    if on_board_rc(r, c) {
        Some(sq(r, c))
    } else {
        None
    }
}

pub fn move_to_string(m: Move) -> String {
    let d = DIRS[m.dir as usize];
    if m.is_inline() {
        let (_, rear) = m.head_and_rear();
        format!("{}{}", cell_name(rear), cell_name(rear + d))
    } else {
        let t = m.tail as i32;
        let last = t + (m.n as i32 - 1) * DIRS[m.axis as usize];
        format!("{}{}{}", cell_name(t), cell_name(last), cell_name(t + d))
    }
}

/// 문자열을 현재 국면의 합법수 중 하나로 해석한다.
pub fn parse_move(pos: &Position, text: &str) -> Option<Move> {
    let t = text.trim().to_ascii_uppercase();
    pos.legal_moves()
        .into_iter()
        .find(|&m| move_to_string(m) == t)
}
