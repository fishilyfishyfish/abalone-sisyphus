//! 대국 진행: 기보, 무르기, 종국 판정.

use crate::board::*;
use crate::layouts::Layout;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GameConfig {
    pub layout: Layout,
    /// 총 수(양쪽 합산 플라이) 제한. 0이면 제한 없음.
    pub move_limit: u32,
    /// 같은 국면(배치 + 둘 차례)이 이 횟수만큼 나오면 종료. 0이면 끔.
    pub repetition_limit: u32,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            layout: Layout::BelgianDaisy,
            move_limit: 200,
            repetition_limit: 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndReason {
    /// 상대 구슬 6개를 밀어냄
    PushedOff,
    /// 수 제한 도달 → 밀어낸 수 비교
    MoveLimit,
    /// 동일 국면 반복 → 밀어낸 수 비교
    Repetition,
    /// 둘 차례인 쪽에 합법수가 없음 → 밀어낸 수 비교
    NoMoves,
}

impl EndReason {
    pub fn name(self) -> &'static str {
        match self {
            EndReason::PushedOff => "pushed_off",
            EndReason::MoveLimit => "move_limit",
            EndReason::Repetition => "repetition",
            EndReason::NoMoves => "no_moves",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Ongoing,
    Win(Color, EndReason),
    Draw(EndReason),
}

#[derive(Clone, Debug)]
pub struct Game {
    pub config: GameConfig,
    pub start: Position,
    pub pos: Position,
    pub history: Vec<(Move, Undo)>,
    /// 시작 국면부터 현재까지 각 국면의 해시 (길이 = history.len() + 1)
    hashes: Vec<u64>,
    outcome: Outcome,
}

impl Game {
    pub fn new(config: GameConfig) -> Game {
        Game::from_position(config.layout.position(), config)
    }

    pub fn from_position(pos: Position, config: GameConfig) -> Game {
        let mut g = Game {
            config,
            start: pos,
            pos,
            history: Vec::new(),
            hashes: vec![pos.hash],
            outcome: Outcome::Ongoing,
        };
        g.outcome = g.evaluate_outcome();
        g
    }

    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    pub fn is_over(&self) -> bool {
        self.outcome != Outcome::Ongoing
    }

    pub fn ply(&self) -> usize {
        self.history.len()
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        if self.is_over() {
            Vec::new()
        } else {
            self.pos.legal_moves()
        }
    }

    /// 합법수면 두고 true. 종국 후나 불법수면 false.
    pub fn play(&mut self, m: Move) -> bool {
        if self.is_over() || !self.pos.legal_moves().contains(&m) {
            return false;
        }
        self.play_unchecked(m);
        true
    }

    pub fn play_unchecked(&mut self, m: Move) {
        let u = self.pos.make_move(m);
        self.history.push((m, u));
        self.hashes.push(self.pos.hash);
        self.outcome = self.evaluate_outcome();
    }

    pub fn undo(&mut self) -> Option<Move> {
        let (m, u) = self.history.pop()?;
        self.hashes.pop();
        self.pos.unmake_move(u);
        self.outcome = self.evaluate_outcome();
        Some(m)
    }

    /// 시작 국면부터 현재까지 각 국면의 해시 (마지막이 현재 국면).
    pub fn hashes(&self) -> &[u64] {
        &self.hashes
    }

    /// 처음 n수만 다시 둔 대국 (복기용). n이 기보보다 길면 전체.
    pub fn prefix(&self, n: usize) -> Game {
        let mut g = Game::from_position(self.start, self.config);
        for &(m, _) in self.history.iter().take(n) {
            g.play_unchecked(m);
        }
        g
    }

    pub fn repetition_count(&self) -> u32 {
        let h = self.pos.hash;
        self.hashes.iter().filter(|&&x| x == h).count() as u32
    }

    fn adjudicate(&self, reason: EndReason) -> Outcome {
        let b = self.pos.pushed_off_by(Color::Black);
        let w = self.pos.pushed_off_by(Color::White);
        if b > w {
            Outcome::Win(Color::Black, reason)
        } else if w > b {
            Outcome::Win(Color::White, reason)
        } else {
            Outcome::Draw(reason)
        }
    }

    fn evaluate_outcome(&self) -> Outcome {
        for c in [Color::Black, Color::White] {
            if self.pos.pushed_off_by(c) >= WIN_PUSHED {
                return Outcome::Win(c, EndReason::PushedOff);
            }
        }
        let rl = self.config.repetition_limit;
        if rl > 0 && self.repetition_count() >= rl {
            return self.adjudicate(EndReason::Repetition);
        }
        let ml = self.config.move_limit;
        if ml > 0 && self.ply() as u32 >= ml {
            return self.adjudicate(EndReason::MoveLimit);
        }
        if self.pos.legal_moves().is_empty() {
            return self.adjudicate(EndReason::NoMoves);
        }
        Outcome::Ongoing
    }
}
