//! 웹에 넘길 JSON 직렬화 (의존성 없이 손으로 작성).

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
