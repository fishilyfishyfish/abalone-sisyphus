//! 설명 층 시연: 쉬움 대 보통 대국을 두고, 수마다 등급과 설명을 출력한다.
//! 실행: cargo run --release --example explain_demo -- [분석 시간 ms=300] [최대 출력 수=40]

use abalone_engine::ai::{self, Level};
use abalone_engine::eval::DEFAULT_WEIGHTS;
use abalone_engine::explain;
use abalone_engine::features::FEATURE_NAMES;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::notation::move_to_string;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ms: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let max: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(40);
    let mut g = Game::new(GameConfig::default());
    let mut player = Searcher::with_std_clock(18, DEFAULT_WEIGHTS);
    let mut analyst = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
    let mut rng = Rng::new(3);
    let mut shown = 0;
    while !g.is_over() && shown < max {
        let level = if g.ply().is_multiple_of(2) {
            Level::Easy
        } else {
            Level::Normal
        };
        let (m, _) = ai::choose(&g, level, &mut player, &mut rng).unwrap();
        if level == Level::Easy {
            let lim = Limits {
                max_depth: 64,
                time_ms: ms,
                exact_root: false,
                prune: false,
            };
            let a = explain::analyze(&g, m, &mut analyst, lim).unwrap();
            let pv = |l: &explain::Line| {
                l.pv.iter()
                    .map(|&x| move_to_string(x))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!(
                "#{} {} {} [{} 손실 {} 깊이 {}]\n  둔 수 PV: {}\n  최선 PV: {}",
                g.ply() + 1,
                if g.pos.side == abalone_engine::Color::Black {
                    "흑"
                } else {
                    "백"
                },
                move_to_string(m),
                a.grade.korean(),
                a.loss,
                a.depth,
                pv(&a.played),
                pv(&a.best)
            );
            if let Some(c) = a.contributions {
                let nz: Vec<String> = c
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| **v != 0)
                    .map(|(i, v)| format!("{}={v:+}", FEATURE_NAMES[i]))
                    .collect();
                println!("  기여도 차이: {}", nz.join(" "));
            }
            for s in &a.sentences {
                println!("  · {s}");
            }
            shown += 1;
        }
        g.play_unchecked(m);
    }
}
