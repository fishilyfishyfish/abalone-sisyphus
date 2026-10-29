//! wasm-bindgen 인터페이스. 웹 워커에서 사용한다.

use wasm_bindgen::prelude::*;

use crate::ai::{self, Level};
use crate::eval::DEFAULT_WEIGHTS;
use crate::explain;
use crate::features;
use crate::game::{Game, GameConfig};
use crate::json;
use crate::layouts::Layout;
use crate::notation::{move_to_string, parse_move};
use crate::record;
use crate::rng::Rng;
use crate::search::Limits;
use crate::search::Searcher;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;
}

/// 브라우저 치환표 크기. 강한 탐색기(어려움·아주 어려움·분석·힌트, 보정 신경망 사용)는 2^20칸 × 16바이트 = 16MB
/// (아주 어려움은 4초에 수백만 노드를 본다). 쉬움·보통용 선형 탐색기는 얕게 보므로 2^18칸 = 4MB.
const TT_BITS: u32 = 20;
const TT_BITS_LINEAR: u32 = 18;

#[wasm_bindgen]
pub struct WasmGame {
    game: Game,
    /// 쉬움·보통: 선형 평가만 (아이용으로 세기를 맞춘 난이도라 평가를 바꾸지 않는다)
    searcher: Searcher,
    /// 어려움·아주 어려움과 수 분석·힌트: 선형 평가 + 보정 신경망
    strong: Searcher,
}

#[wasm_bindgen]
impl WasmGame {
    /// layout: "belgian" | "standard" | "german", move_limit: 0이면 제한 없음
    #[wasm_bindgen(constructor)]
    pub fn new(layout: &str, move_limit: u32) -> WasmGame {
        let config = GameConfig {
            layout: Layout::parse(layout).unwrap_or(Layout::BelgianDaisy),
            move_limit,
            ..GameConfig::default()
        };
        WasmGame {
            game: Game::new(config),
            searcher: Searcher::new(TT_BITS_LINEAR, DEFAULT_WEIGHTS, Box::new(now)),
            strong: {
                let mut s = Searcher::new(TT_BITS, DEFAULT_WEIGHTS, Box::new(now));
                s.nn = true;
                s
            },
        }
    }

    pub fn state(&self) -> String {
        json::state_json(&self.game)
    }

    pub fn legal_moves(&self) -> String {
        json::legal_moves_json(&self.game)
    }

    pub fn features(&self) -> String {
        json::features_json(&features::compute(&self.game.pos))
    }

    /// 둘 차례인 쪽 구슬마다 방향별 선택지 (구슬 하나를 눌렀을 때).
    pub fn single_options(&self) -> String {
        json::single_options_json(&self.game)
    }

    /// 고른 칸들(정규 칸 번호)을 그대로 움직이는 방향별 선택지.
    pub fn group_options(&self, cells: &[u8]) -> String {
        json::group_options_json(&self.game, cells)
    }

    /// 표기로 수를 둔다. 성공하면 true.
    pub fn play(&mut self, text: &str) -> bool {
        if self.game.is_over() {
            return false;
        }
        match parse_move(&self.game.pos, text) {
            Some(m) => self.game.play(m),
            None => false,
        }
    }

    pub fn undo(&mut self) -> bool {
        self.game.undo().is_some()
    }

    /// 현재 국면의 흑 관점 평가와 특징별 기여도.
    pub fn evaluate(&self) -> String {
        json::eval_json(&self.game.pos, &self.strong.weights, true)
    }

    /// 난이도("random" | "easy" | "normal" | "hard")에 맞는 수를 고른다. 두지는 않는다.
    /// 결과: {"move": 표기 또는 "", "search": 탐색 정보 또는 null}
    pub fn ai_move(&mut self, level: &str, seed: f64) -> String {
        let level = Level::parse(level).unwrap_or(Level::Random);
        let mut rng = Rng::new(seed.to_bits());
        let searcher = match level {
            Level::Hard | Level::Expert => &mut self.strong,
            _ => &mut self.searcher,
        };
        match ai::choose(&self.game, level, searcher, &mut rng) {
            None => "{\"move\":\"\",\"search\":null}".to_string(),
            Some((m, res)) => format!(
                "{{\"move\":\"{}\",\"search\":{}}}",
                move_to_string(m),
                res.map_or("null".to_string(), |r| json::search_json(
                    &self.game.pos,
                    &r
                ))
            ),
        }
    }

    /// 사람이 생각하는 동안 컴퓨터(어려움·아주 어려움)가 미리 생각한다. 지금 국면을 ms만큼 탐색해
    /// 보정 신경망 탐색기의 치환표를 채우고, 다음 컴퓨터 수가 그 결과를 이어받는다. 짧게 여러 번 부른다.
    /// 결과: {"depth": 이번 조각에서 끝낸 깊이, "nodes": 노드 수}
    pub fn ponder(&mut self, time_ms: f64) -> String {
        if self.game.is_over() {
            return "{\"depth\":0,\"nodes\":0}".to_string();
        }
        let r = self.strong.ponder(&self.game, time_ms, true);
        format!("{{\"depth\":{},\"nodes\":{}}}", r.depth, r.nodes)
    }

    /// 처음 ply수까지 둔 국면의 상태 (복기에서 그 수 직전 국면을 보여줄 때).
    pub fn state_at(&self, ply: u32) -> String {
        json::state_json(&self.game.prefix(ply as usize))
    }

    /// 기보 ply번째 수(0부터)를 분석한다. 없으면 "null".
    pub fn analyze_ply(&mut self, ply: u32, time_ms: f64) -> String {
        let ply = ply as usize;
        let Some(&(m, _)) = self.game.history.get(ply) else {
            return "null".to_string();
        };
        let before = self.game.prefix(ply);
        match explain::analyze(&before, m, &mut self.strong, timed(time_ms)) {
            Some(a) => json::analysis_json(&before.pos, &a),
            None => "null".to_string(),
        }
    }

    /// 지금 국면에서 아직 두지 않은 수를 분석한다 (큰 실수 경고용).
    pub fn analyze_candidate(&mut self, text: &str, time_ms: f64) -> String {
        let Some(m) = parse_move(&self.game.pos, text) else {
            return "null".to_string();
        };
        match explain::analyze(&self.game, m, &mut self.strong, timed(time_ms)) {
            Some(a) => json::analysis_json(&self.game.pos, &a),
            None => "null".to_string(),
        }
    }

    /// 지금 대국의 기보 텍스트. 빈 문자열인 태그는 뺀다.
    pub fn export_record(&self, black: &str, white: &str, date: &str) -> String {
        record::export(
            &self.game,
            &[("Date", date), ("Black", black), ("White", white)],
        )
    }

    /// 기보 텍스트를 읽어 지금 대국을 바꾼다. 실패하면 대국은 그대로다. 결과는 `json::record_result_json`.
    pub fn load_record(&mut self, text: &str) -> String {
        let r = record::import(text);
        if let Ok(rec) = &r {
            self.game = rec.game.clone();
            self.searcher.clear();
            self.strong.clear();
        }
        json::record_result_json(&r)
    }

    /// 힌트: 지금 국면의 최선 수와 좋아지는 점.
    pub fn hint(&mut self, time_ms: f64) -> String {
        match explain::hint(&self.game, &mut self.strong, timed(time_ms)) {
            Some(h) => json::hint_json(&self.game.pos, &h),
            None => "null".to_string(),
        }
    }
}

fn timed(ms: f64) -> Limits {
    Limits {
        max_depth: 64,
        time_ms: ms,
        exact_root: false,
        prune: false,
    }
}
