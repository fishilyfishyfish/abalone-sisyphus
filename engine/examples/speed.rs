//! 탐색 속도 측정: 세 배치에서 12수씩, 고정 깊이 가지치기 탐색(수마다 같은 탐색기)을 두고 노드 수·시간을 잰다.
//! 탐색·평가를 고칠 때 같은 깊이까지 걸리는 시간을 비교하는 데 쓴다 (노드 수는 수 정렬이 바뀌면 달라진다).
//! 실행: cargo run --release --example speed -- [깊이=7]

use abalone_engine::eval::DEFAULT_WEIGHTS;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::Layout;
use abalone_engine::search::{Limits, Searcher};
fn main() {
    let d: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    let mut total = 0;
    let t = std::time::Instant::now();
    for layout in [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy] {
        let mut g = Game::new(GameConfig {
            layout,
            ..GameConfig::default()
        });
        let mut s = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
        for _ in 0..12 {
            let r = s.search(
                &g,
                Limits {
                    prune: true,
                    ..Limits::depth(d)
                },
            );
            total += r.nodes;
            g.play_unchecked(r.best.unwrap());
        }
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("nodes {total} {ms:.0} ms {:.0} knps", total as f64 / ms);
}
