//! wasm-bindgen 인터페이스. 웹 워커에서 사용한다.

use wasm_bindgen::prelude::*;

use crate::features;
use crate::game::{Game, GameConfig};
use crate::json;
use crate::layouts::Layout;
use crate::notation::parse_move;
use crate::rng::Rng;

#[wasm_bindgen]
pub struct WasmGame {
    game: Game,
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

    /// 합법수 중 무작위 하나의 표기 (없으면 빈 문자열). 두지는 않는다.
    pub fn random_move(&self, seed: f64) -> String {
        let moves = self.game.legal_moves();
        if moves.is_empty() {
            return String::new();
        }
        let mut rng = Rng::new(seed.to_bits());
        crate::notation::move_to_string(moves[rng.below(moves.len())])
    }
}
