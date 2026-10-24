//! 웹에 넘길 JSON 직렬화 (의존성 없이 손으로 작성).

use crate::assist::{self, Blocked, DirOption};
use crate::board::*;
use crate::features::{Features, SideFeatures, FEATURE_NAMES};
use crate::game::{Game, Outcome};
use crate::notation::move_to_string;

pub fn color_name(c: Color) -> &'static str {
    match c {
        Color::Black => "black",
        Color::White => "white",
    }
}

fn cells_json(m: Move) -> String {
    let v: Vec<String> = m
        .cells()
        .map(|s| CELL_INDEX[s as usize].to_string())
        .collect();
    format!("[{}]", v.join(","))
}

pub fn move_json(pos: &Position, m: Move) -> String {
    format!(
        "{{\"cells\":{},\"dir\":{},\"text\":\"{}\",\"inline\":{},\"push_off\":{}}}",
        cells_json(m),
        m.dir,
        move_to_string(m),
        m.is_inline(),
        pos.is_push_off(m)
    )
}

pub fn legal_moves_json(g: &Game) -> String {
    let v: Vec<String> = g
        .legal_moves()
        .into_iter()
        .map(|m| move_json(&g.pos, m))
        .collect();
    format!("[{}]", v.join(","))
}

pub fn outcome_json(o: Outcome) -> String {
    match o {
        Outcome::Ongoing => "{\"status\":\"ongoing\"}".to_string(),
        Outcome::Win(c, r) => format!(
            "{{\"status\":\"win\",\"winner\":\"{}\",\"reason\":\"{}\"}}",
            color_name(c),
            r.name()
        ),
        Outcome::Draw(r) => format!("{{\"status\":\"draw\",\"reason\":\"{}\"}}", r.name()),
    }
}

pub fn state_json(g: &Game) -> String {
    let board: String = CELLS
        .iter()
        .map(|&s| match g.pos.at(s as i32) {
            Some(Color::Black) => 'b',
            Some(Color::White) => 'w',
            None => '.',
        })
        .collect();
    let history: Vec<String> = g
        .history
        .iter()
        .map(|(m, _)| format!("\"{}\"", move_to_string(*m)))
        .collect();
    let push_offs: Vec<&str> = g
        .history
        .iter()
        .map(|(_, u)| if u.pushed_off() { "true" } else { "false" })
        .collect();
    let last = match g.history.last() {
        Some(&(m, u)) => format!(
            "{{\"cells\":{},\"dir\":{},\"push_off\":{}}}",
            cells_json(m),
            m.dir,
            u.pushed_off()
        ),
        None => "null".to_string(),
    };
    format!(
        "{{\"board\":\"{}\",\"side\":\"{}\",\"ply\":{},\"move_limit\":{},\"layout\":\"{}\",\
         \"pushed\":{{\"black\":{},\"white\":{}}},\"outcome\":{},\"history\":[{}],\"push_offs\":[{}],\"last\":{},\
         \"repetition\":{}}}",
        board,
        color_name(g.pos.side),
        g.ply(),
        g.config.move_limit,
        g.config.layout.name(),
        g.pos.pushed_off_by(Color::Black),
        g.pos.pushed_off_by(Color::White),
        outcome_json(g.outcome()),
        history.join(","),
        push_offs.join(","),
        last,
        g.repetition_count()
    )
}

fn side_json(f: &SideFeatures) -> String {
    let parts: Vec<String> = FEATURE_NAMES
        .iter()
        .zip(f.as_array())
        .map(|(n, v)| format!("\"{n}\":{v}"))
        .collect();
    format!("{{{}}}", parts.join(","))
}

pub fn features_json(f: &Features) -> String {
    format!(
        "{{\"black\":{},\"white\":{}}}",
        side_json(&f.black),
        side_json(&f.white)
    )
}

fn blocked_json(b: Blocked) -> String {
    match b {
        Blocked::NotStronger { mine, theirs } => format!(
            "{{\"code\":\"{}\",\"mine\":{mine},\"theirs\":{theirs}}}",
            b.code()
        ),
        _ => format!("{{\"code\":\"{}\"}}", b.code()),
    }
}

pub fn options_json(pos: &Position, opts: &[DirOption; 6]) -> String {
    let v: Vec<String> = opts
        .iter()
        .map(|o| {
            format!(
                "{{\"dir\":{},\"move\":{},\"blocked\":{}}}",
                o.dir,
                o.mv.map_or("null".to_string(), |m| move_json(pos, m)),
                o.blocked.map_or("null".to_string(), blocked_json)
            )
        })
        .collect();
    format!("[{}]", v.join(","))
}

/// 둘 차례인 쪽 구슬마다 방향별 선택지. 키는 정규 칸 번호. 종국이면 빈 객체.
pub fn single_options_json(g: &Game) -> String {
    if g.is_over() {
        return "{}".to_string();
    }
    let v: Vec<String> = Bits(g.pos.own())
        .map(|s| {
            format!(
                "\"{}\":{}",
                CELL_INDEX[s as usize],
                options_json(&g.pos, &assist::single_options(&g.pos, s))
            )
        })
        .collect();
    format!("{{{}}}", v.join(","))
}

/// 고른 칸들(정규 번호)을 그대로 움직이는 방향별 선택지.
pub fn group_options_json(g: &Game, cells: &[u8]) -> String {
    let squares: Vec<i32> = cells
        .iter()
        .filter(|&&c| (c as usize) < NUM_CELLS)
        .map(|&c| CELLS[c as usize] as i32)
        .collect();
    let opts = if g.is_over() {
        let mut o = assist::group_options(&g.pos, &[]);
        o.iter_mut().for_each(|x| x.mv = None);
        o
    } else {
        assist::group_options(&g.pos, &squares)
    };
    options_json(&g.pos, &opts)
}
