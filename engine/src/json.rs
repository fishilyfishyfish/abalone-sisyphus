//! 웹에 넘길 JSON 직렬화 (의존성 없이 손으로 작성).

use crate::assist::{self, Blocked, DirOption};
use crate::board::*;
use crate::eval::{self, Weights};
use crate::explain::{Hint, Line, MoveAnalysis};
use crate::features::{Features, SideFeatures, FEATURE_NAMES};
use crate::game::{Game, Outcome};
use crate::notation::move_to_string;
use crate::record::{Record, RecordError};
use crate::search::SearchResult;

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

/// 탐색 결과. score는 둘 차례 관점, score_black은 흑 관점.
pub fn search_json(pos: &Position, r: &SearchResult) -> String {
    let pv: Vec<String> =
        r.pv.iter()
            .map(|&m| format!("\"{}\"", move_to_string(m)))
            .collect();
    let black = if pos.side == Color::Black {
        r.score
    } else {
        -r.score
    };
    format!(
        "{{\"best\":\"{}\",\"depth\":{},\"score\":{},\"score_black\":{},\"nodes\":{},\"ms\":{:.0},\"pv\":[{}]}}",
        r.best.map(move_to_string).unwrap_or_default(),
        r.depth,
        r.score,
        black,
        r.nodes,
        r.elapsed_ms,
        pv.join(",")
    )
}

/// 현재 국면의 흑 관점 정적 평가와 특징별 기여도 (가중치 × 흑−백 차이).
/// nn이면 보정 신경망 값(흑 관점)을 `residual_black`으로 따로 주고 점수에 더한다.
pub fn eval_json(pos: &Position, w: &Weights, nn: bool) -> String {
    let c = eval::contributions(pos, Color::Black, w);
    let r = if nn {
        eval::residual_for(pos, Color::Black)
    } else {
        0
    };
    let field = |vals: &[i32]| -> String {
        let v: Vec<String> = FEATURE_NAMES
            .iter()
            .zip(vals)
            .map(|(n, v)| format!("\"{n}\":{v}"))
            .collect();
        format!("{{{}}}", v.join(","))
    };
    format!(
        "{{\"score_black\":{},\"weights\":{},\"contrib_black\":{},\"residual_black\":{}}}",
        c.iter().sum::<i32>() + r,
        field(&w.0),
        field(&c),
        if nn {
            r.to_string()
        } else {
            "null".to_string()
        }
    )
}

fn line_json(root: &Position, l: &Line, mover: Color) -> String {
    let pv: Vec<String> =
        l.pv.iter()
            .map(|&m| format!("\"{}\"", move_to_string(m)))
            .collect();
    let black = if mover == Color::Black {
        l.score
    } else {
        -l.score
    };
    format!(
        "{{\"move\":{},\"score\":{},\"score_black\":{},\"pv\":[{}]}}",
        move_json(root, l.first),
        l.score,
        black,
        pv.join(",")
    )
}

fn sentences_json(v: &[String]) -> String {
    let s: Vec<String> = v
        .iter()
        .map(|x| format!("\"{}\"", x.replace('"', "'")))
        .collect();
    format!("[{}]", s.join(","))
}

fn reasons_json(rs: &[crate::explain::Reason]) -> String {
    let v: Vec<String> = rs
        .iter()
        .map(|r| {
            format!(
                "{{\"key\":\"{}\",\"delta\":{},\"mine_side\":{},\"played\":{},\"best\":{}}}",
                r.key, r.delta, r.mine_side, r.played_value, r.best_value
            )
        })
        .collect();
    format!("[{}]", v.join(","))
}

/// 수 분석. 점수는 수 두는 쪽 관점(score)과 흑 관점(score_black).
pub fn analysis_json(root: &Position, a: &MoveAnalysis) -> String {
    let contrib = match a.contributions {
        Some(c) => {
            let mut v: Vec<String> = FEATURE_NAMES
                .iter()
                .zip(c)
                .map(|(n, v)| format!("\"{n}\":{v}"))
                .collect();
            if let Some(r) = a.residual {
                v.push(format!("\"residual\":{r}"));
            }
            format!("{{{}}}", v.join(","))
        }
        None => "null".to_string(),
    };
    format!(
        "{{\"mover\":\"{}\",\"grade\":\"{}\",\"grade_ko\":\"{}\",\"loss\":{},\"depth\":{},\
         \"best\":{},\"played\":{},\"contributions\":{},\"reasons\":{},\"sentences\":{},\"material_diff\":{},\"material_drop\":{}}}",
        color_name(a.mover),
        a.grade.code(),
        a.grade.korean(),
        a.loss,
        a.depth,
        line_json(root, &a.best, a.mover),
        line_json(root, &a.played, a.mover),
        contrib,
        reasons_json(&a.reasons),
        sentences_json(&a.sentences),
        a.material_diff,
        a.material_drop
    )
}

pub fn hint_json(root: &Position, h: &Hint) -> String {
    format!(
        "{{\"depth\":{},\"best\":{},\"reasons\":{},\"sentences\":{}}}",
        h.depth,
        line_json(root, &h.best, root.side),
        reasons_json(&h.reasons),
        sentences_json(&h.sentences)
    )
}

/// JSON 문자열 리터럴 (따옴표 포함).
pub fn string_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 기보 읽기 결과. 성공: {"ok":true,"tags":[[이름,값],...],"converted":n,"plies":n}
/// 실패: {"ok":false,"error":코드,"index":몇 번째 수 또는 null,"token":표기 또는 null,"value":태그 값 또는 null}
pub fn record_result_json(r: &Result<Record, RecordError>) -> String {
    match r {
        Ok(rec) => {
            let tags: Vec<String> = rec
                .tags
                .iter()
                .map(|(k, v)| format!("[{},{}]", string_json(k), string_json(v)))
                .collect();
            format!(
                "{{\"ok\":true,\"tags\":[{}],\"converted\":{},\"plies\":{}}}",
                tags.join(","),
                rec.converted,
                rec.game.ply()
            )
        }
        Err(e) => {
            let (index, token) = match e.at() {
                Some((i, t)) => (i.to_string(), string_json(t)),
                None => ("null".to_string(), "null".to_string()),
            };
            let value = match e {
                RecordError::UnknownLayout(v) | RecordError::BadMoveLimit(v) => string_json(v),
                _ => "null".to_string(),
            };
            format!(
                "{{\"ok\":false,\"error\":\"{}\",\"index\":{index},\"token\":{token},\"value\":{value}}}",
                e.code()
            )
        }
    }
}
