//! 기보 입출력.
//!
//! 형식 (PGN과 비슷한 텍스트):
//! ```text
//! [Game "Abalone"]
//! [Layout "belgian"]
//! [MoveLimit "200"]
//! [Black "사람"]
//! [Result "1-0"]
//! [Termination "pushed_off"]
//!
//! 1. A1B2 I9H8
//! 2. ...
//! 1-0
//! ```
//! - 내보내기는 이 프로젝트 표기(`notation`)만 쓴다. 결과는 흑 승 "1-0", 백 승 "0-1", 무승부 "1/2-1/2", 진행 중 "*".
//! - 읽기는 다른 곳의 기보를 받아들이도록 너그럽게 한다.
//!   - 태그는 `[이름 "값"]` 줄. 모르는 태그는 그대로 보관한다. Result·Termination은 다시 계산하므로 읽지 않는다.
//!   - Layout 태그가 없으면 세 배치를 차례로 시도해 기보 전체가 합법인 배치를 고른다.
//!   - 수 번호(`12.`), 결과 표시, `{...}` 주석과 `;` 뒤 줄 주석은 건너뛴다. 대소문자, 칸 사이 `-`·`:`·`x`는 상관없다.
//!   - 수 하나는 이 프로젝트 표기를 먼저 찾고, 없으면 흔한 변형으로 해석한다(`variant_spellings`).

use crate::board::*;
use crate::game::{Game, GameConfig, Outcome};
use crate::layouts::Layout;
use crate::notation::{cell_name, move_to_string, parse_cell};

#[derive(Clone, Debug)]
pub struct Record {
    pub game: Game,
    /// 읽은 태그 (Layout, MoveLimit 포함, 순서 유지)
    pub tags: Vec<(String, String)>,
    /// 변형 표기로 해석한 수의 개수 (이 프로젝트 표기와 다르게 적힌 수)
    pub converted: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RecordError {
    /// Layout 태그 값을 모름
    UnknownLayout(String),
    /// MoveLimit 태그가 숫자가 아님
    BadMoveLimit(String),
    /// index번째 수(0부터)가 칸 표기로 읽히지 않음
    BadToken { index: usize, token: String },
    /// index번째 수가 그 국면의 합법수가 아님
    Illegal { index: usize, token: String },
    /// index번째 수가 합법수 여러 개로 해석됨
    Ambiguous { index: usize, token: String },
    /// 대국이 끝난 뒤에도 수가 더 있음 (index = 남은 첫 수)
    AfterEnd { index: usize, token: String },
    /// 수가 하나도 없고 Layout 태그도 없음
    Empty,
}

impl RecordError {
    pub fn code(&self) -> &'static str {
        match self {
            RecordError::UnknownLayout(_) => "unknown_layout",
            RecordError::BadMoveLimit(_) => "bad_move_limit",
            RecordError::BadToken { .. } => "bad_token",
            RecordError::Illegal { .. } => "illegal",
            RecordError::Ambiguous { .. } => "ambiguous",
            RecordError::AfterEnd { .. } => "after_end",
            RecordError::Empty => "empty",
        }
    }

    /// (몇 번째 수, 그 표기). 수와 관계없는 오류면 None.
    pub fn at(&self) -> Option<(usize, &str)> {
        match self {
            RecordError::BadToken { index, token }
            | RecordError::Illegal { index, token }
            | RecordError::Ambiguous { index, token }
            | RecordError::AfterEnd { index, token } => Some((*index, token)),
            _ => None,
        }
    }
}

pub fn result_token(o: Outcome) -> &'static str {
    match o {
        Outcome::Ongoing => "*",
        Outcome::Win(Color::Black, _) => "1-0",
        Outcome::Win(Color::White, _) => "0-1",
        Outcome::Draw(_) => "1/2-1/2",
    }
}

/// 기보 텍스트. `extra`는 Game·Layout·MoveLimit 뒤, Result 앞에 들어갈 태그(빈 값은 뺀다).
pub fn export(game: &Game, extra: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut tag = |k: &str, v: &str| {
        let v = v.replace(['"', '\n', '\r', ']'], " ");
        out.push_str(&format!("[{k} \"{}\"]\n", v.trim()));
    };
    tag("Game", "Abalone");
    tag("Layout", game.config.layout.name());
    tag("MoveLimit", &game.config.move_limit.to_string());
    for &(k, v) in extra {
        if !v.trim().is_empty() {
            tag(k, v);
        }
    }
    let o = game.outcome();
    tag("Result", result_token(o));
    if let Outcome::Win(_, r) | Outcome::Draw(r) = o {
        tag("Termination", r.name());
    }
    out.push('\n');
    for (i, pair) in game.history.chunks(2).enumerate() {
        out.push_str(&format!("{}.", i + 1));
        for &(m, _) in pair {
            out.push(' ');
            out.push_str(&move_to_string(m));
        }
        out.push('\n');
    }
    out.push_str(result_token(o));
    out.push('\n');
    out
}

/// 기보 텍스트를 읽어 대국을 다시 둔다.
pub fn import(text: &str) -> Result<Record, RecordError> {
    let (tags, tokens) = split(text);
    let get = |k: &str| {
        tags.iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    };
    let layouts: Vec<Layout> = match get("Layout") {
        Some(v) => vec![parse_layout(v).ok_or_else(|| RecordError::UnknownLayout(v.to_string()))?],
        None if tokens.is_empty() => return Err(RecordError::Empty),
        None => vec![Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy],
    };
    let move_limit = match get("MoveLimit") {
        Some(v) => v
            .trim()
            .parse::<u32>()
            .map_err(|_| RecordError::BadMoveLimit(v.to_string()))?,
        // 태그가 없으면 기본 200수, 기보가 그보다 길면 제한 없음
        None if tokens.len() > GameConfig::default().move_limit as usize => 0,
        None => GameConfig::default().move_limit,
    };
    // 배치를 차례로 시도하고, 모두 실패하면 가장 멀리 간 배치의 오류를 낸다
    let has_layout = get("Layout").is_some();
    let mut best_err: Option<RecordError> = None;
    for layout in layouts {
        let config = GameConfig {
            layout,
            move_limit,
            ..GameConfig::default()
        };
        match replay(config, &tokens) {
            Ok((game, converted)) => {
                let mut tags = tags;
                if !has_layout {
                    tags.push(("Layout".to_string(), layout.name().to_string()));
                }
                return Ok(Record {
                    game,
                    tags,
                    converted,
                });
            }
            Err(e) => {
                let far = |e: &RecordError| e.at().map_or(0, |(i, _)| i);
                if best_err.as_ref().is_none_or(|b| far(&e) > far(b)) {
                    best_err = Some(e);
                }
            }
        }
    }
    Err(best_err.unwrap_or(RecordError::Empty))
}

fn replay(config: GameConfig, tokens: &[String]) -> Result<(Game, usize), RecordError> {
    let mut g = Game::new(config);
    let mut converted = 0;
    for (index, token) in tokens.iter().enumerate() {
        let err_token = token.clone();
        if g.is_over() {
            return Err(RecordError::AfterEnd {
                index,
                token: err_token,
            });
        }
        let Some(cells) = parse_cells(token) else {
            return Err(RecordError::BadToken {
                index,
                token: err_token,
            });
        };
        let (m, exact) = resolve(&g.pos, &cells).map_err(|ambiguous| {
            if ambiguous {
                RecordError::Ambiguous {
                    index,
                    token: token.clone(),
                }
            } else {
                RecordError::Illegal {
                    index,
                    token: token.clone(),
                }
            }
        })?;
        if !exact {
            converted += 1;
        }
        g.play_unchecked(m);
    }
    Ok((g, converted))
}

/// 태그와 수 표기 토큰으로 나눈다.
fn split(text: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut tags = Vec::new();
    let mut body = String::new();
    let mut in_brace = false;
    for line in text.lines() {
        let t = line.trim();
        if !in_brace && t.starts_with('[') && t.ends_with(']') {
            if let Some(kv) = parse_tag(&t[1..t.len() - 1]) {
                tags.push(kv);
            }
            continue;
        }
        // 주석 제거: {...}는 여러 줄에 걸칠 수 있고, ;는 줄 끝까지
        for ch in line.chars() {
            match ch {
                '{' => in_brace = true,
                '}' if in_brace => in_brace = false,
                ';' if !in_brace => break,
                _ if !in_brace => body.push(ch),
                _ => {}
            }
        }
        body.push('\n');
    }
    let tokens = body
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(clean_token)
        .collect();
    (tags, tokens)
}

fn parse_tag(inner: &str) -> Option<(String, String)> {
    let inner = inner.trim();
    let sp = inner.find(char::is_whitespace)?;
    let key = inner[..sp].to_string();
    let val = inner[sp..].trim().trim_matches('"').to_string();
    Some((key, val))
}

/// 수 번호와 결과 표시는 버리고, 수 표기만 남긴다.
fn clean_token(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || matches!(t, "*" | "1-0" | "0-1" | "1/2-1/2" | "½-½") {
        return None;
    }
    // "12." "12..." "12)" 같은 번호, 또는 "12.A1B2"처럼 붙은 번호
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &t[digits..];
    let rest = if digits > 0 && (rest.starts_with('.') || rest.starts_with(')')) {
        rest.trim_start_matches(['.', ')'])
    } else {
        t
    };
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

/// "a1-b2", "A1:B2", "a1xb2", "C3E3D3" → 칸 목록. 2칸 또는 3칸만 받는다.
fn parse_cells(token: &str) -> Option<Vec<i32>> {
    let s: Vec<u8> = token
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric())
        .map(|b| b.to_ascii_uppercase())
        .collect();
    // 칸 사이의 x는 구분자로 본다 (칸 이름에는 A~I만 쓰인다)
    let s: Vec<u8> = s.into_iter().filter(|&b| b != b'X').collect();
    if !matches!(s.len(), 4 | 6) {
        return None;
    }
    s.chunks(2)
        .map(|c| parse_cell(std::str::from_utf8(c).ok()?))
        .collect()
}

/// 칸 목록을 합법수로 해석한다. Ok((수, 이 프로젝트 표기와 같은지)), Err(여러 개로 해석되는지).
fn resolve(pos: &Position, cells: &[i32]) -> Result<(Move, bool), bool> {
    let token: String = cells.iter().map(|&c| cell_name(c)).collect();
    let legal = pos.legal_moves();
    if let Some(&m) = legal.iter().find(|&&m| move_to_string(m) == token) {
        return Ok((m, true));
    }
    let found: Vec<Move> = legal
        .into_iter()
        .filter(|&m| variant_spellings(m).contains(&token))
        .collect();
    match found.len() {
        0 => Err(false),
        1 => Ok((found[0], false)),
        _ => Err(true),
    }
}

/// 이 프로젝트 표기 말고도 받아들이는 표기들.
/// - 인라인: `<맨 뒤 구슬><맨 앞 구슬의 도착 칸>` (예: A1..A3을 위로 → A1A4)
/// - 2개 이상: `<줄 한쪽 끝><다른 끝><앞에 쓴 끝의 도착 칸>`을 두 순서 모두 (인라인도 이 꼴로 쓸 수 있다)
///
/// 서로 다른 합법수가 같은 표기를 갖는 경우는 `resolve`가 모호함으로 알린다.
pub fn variant_spellings(m: Move) -> Vec<String> {
    let d = DIRS[m.dir as usize];
    let mut v = Vec::new();
    if m.is_inline() {
        let (head, rear) = m.head_and_rear();
        v.push(format!("{}{}", cell_name(rear), cell_name(head + d)));
    }
    if m.n >= 2 {
        let t = m.tail as i32;
        let last = t + (m.n as i32 - 1) * DIRS[m.axis as usize];
        for (a, b) in [(t, last), (last, t)] {
            v.push(format!(
                "{}{}{}",
                cell_name(a),
                cell_name(b),
                cell_name(a + d)
            ));
        }
    }
    v
}

/// "belgian", "Belgian Daisy", "belgian-daisy", "standard", "classic", "german daisy" 등.
pub fn parse_layout(s: &str) -> Option<Layout> {
    let k: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    match k.as_str() {
        "belgian" | "belgiandaisy" => Some(Layout::BelgianDaisy),
        "standard" | "classic" | "default" => Some(Layout::Standard),
        "german" | "germandaisy" => Some(Layout::GermanDaisy),
        _ => None,
    }
}
