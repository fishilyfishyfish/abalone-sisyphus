//! 아발론 엔진: 규칙, 합법수 생성, 특징값.

pub mod ai;
pub mod assist;
pub mod board;
pub mod eval;
pub mod features;
pub mod game;
pub mod json;
pub mod layouts;
pub mod notation;
pub mod rng;
pub mod search;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use board::{Color, Move, Position};
pub use game::{EndReason, Game, GameConfig, Outcome};
pub use layouts::Layout;
