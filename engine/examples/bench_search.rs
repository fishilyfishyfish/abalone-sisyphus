//! 탐색 성능 측정: 시작 배치별로 고정 깊이·고정 시간 탐색의 노드 수, 속도, 도달 깊이(가지치기 끔/켬).
//! 실행: cargo run --release --example bench_search

use abalone_engine::eval::DEFAULT_WEIGHTS;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::Layout;
use abalone_engine::notation::move_to_string;
use abalone_engine::search::{Limits, Searcher};

fn main() {
    for layout in [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy] {
        let g = Game::new(GameConfig {
            layout,
            ..GameConfig::default()
        });
        for d in 1..=5 {
            let mut s = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
            let r = s.search(&g, Limits::depth(d));
            println!(
                "{:<9} depth {d}: {:>9} nodes {:>8.1} ms {:>6.0} knps  score {:>5}  best {}",
                layout.name(),
                r.nodes,
                r.elapsed_ms,
                r.nodes as f64 / r.elapsed_ms,
                r.score,
                r.best.map(move_to_string).unwrap_or_default()
            );
        }
        for (t, prune) in [
            (500.0, false),
            (1500.0, false),
            (500.0, true),
            (1500.0, true),
        ] {
            let mut s = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
            let r = s.search(
                &g,
                Limits {
                    max_depth: 30,
                    time_ms: t,
                    exact_root: false,
                    prune,
                },
            );
            let pv: Vec<String> = r.pv.iter().map(|&m| move_to_string(m)).collect();
            println!(
                "{:<9} {t:>5} ms{}: depth {} {:>9} nodes {:>6.0} knps  pv {}",
                layout.name(),
                if prune { " prune" } else { "      " },
                r.depth,
                r.nodes,
                r.nodes as f64 / r.elapsed_ms,
                pv.join(" ")
            );
        }
    }
}
