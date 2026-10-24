//! 난이도끼리 자동 대국. 색과 시작 배치를 번갈아 바꾼다.
//! 실행: cargo run --release --example match -- <A 난이도> <B 난이도> [판 수]
//! 난이도: random | easy | normal | hard

use abalone_engine::ai::{self, Level};
use abalone_engine::board::Color;
use abalone_engine::eval::DEFAULT_WEIGHTS;
use abalone_engine::game::{Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;
use abalone_engine::search::Searcher;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a = Level::parse(args.get(1).map_or("normal", |s| s)).expect("level A");
    let b = Level::parse(args.get(2).map_or("easy", |s| s)).expect("level B");
    let games: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(12);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    let mut sa = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
    let mut sb = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
    let mut rng = Rng::new(2026);
    let (mut wa, mut wb, mut draws, mut pushed_a, mut pushed_b) = (0, 0, 0, 0, 0);
    for gi in 0..games {
        let layout = layouts[(gi / 2) % 3];
        let a_color = if gi % 2 == 0 {
            Color::Black
        } else {
            Color::White
        };
        let mut g = Game::new(GameConfig {
            layout,
            ..GameConfig::default()
        });
        sa.clear();
        sb.clear();
        while !g.is_over() {
            let (lvl, s) = if g.pos.side == a_color {
                (a, &mut sa)
            } else {
                (b, &mut sb)
            };
            let (m, _) = ai::choose(&g, lvl, s, &mut rng).unwrap();
            g.play_unchecked(m);
        }
        pushed_a += g.pos.pushed_off_by(a_color);
        pushed_b += g.pos.pushed_off_by(a_color.other());
        let r = match g.outcome() {
            Outcome::Win(c, why) if c == a_color => {
                wa += 1;
                format!("A win ({})", why.name())
            }
            Outcome::Win(_, why) => {
                wb += 1;
                format!("B win ({})", why.name())
            }
            Outcome::Draw(why) => {
                draws += 1;
                format!("draw ({})", why.name())
            }
            Outcome::Ongoing => unreachable!(),
        };
        println!(
            "game {gi:>3} {:<9} A={a_color:?}: {r}, {} plies",
            layout.name(),
            g.ply()
        );
    }
    println!(
        "{a:?} vs {b:?}: A {wa} / draw {draws} / B {wb}  (pushed off: A {pushed_a}, B {pushed_b})"
    );
}
