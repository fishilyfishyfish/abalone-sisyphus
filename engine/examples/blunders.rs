//! 실수 측정: 한 설정끼리 자기대국한 뒤, 모든 수를 더 긴 시간의 탐색으로 다시 평가해
//! "둔 수가 최선 수보다 몇 점 나빴나"(손실)를 센다. 1000점 = 구슬 하나.
//! 기준 탐색도 같은 엔진이라 그 탐색이 보는 범위 안의 실수만 잡힌다.
//!
//! 실행: cargo run --release --example blunders -- [대국 설정 ms=1500] [판 수=4] [기준 ms=6000]

use std::sync::{Arc, Mutex};

use abalone_engine::board::Move;
use abalone_engine::eval::{DEFAULT_WEIGHTS, WIN_BOUND};
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::Layout;
use abalone_engine::notation::move_to_string;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

fn timed(ms: f64) -> Limits {
    Limits {
        max_depth: 64,
        time_ms: ms,
        exact_root: false,
        prune: false,
    }
}

fn clamp(s: i32) -> i32 {
    s.clamp(-3 * 1000, 3 * 1000) // 승패 점수는 구슬 3개 손실로 눌러서 센다
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let play_ms: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1500.0);
    let games: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let ref_ms: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6000.0);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    let losses = Arc::new(Mutex::new(Vec::<(i32, u32, u32, String)>::new()));
    let threads: Vec<_> = (0..games)
        .map(|gi| {
            let losses = losses.clone();
            std::thread::spawn(move || {
                let mut rng = Rng::new(31 + gi as u64);
                let layout = layouts[gi % 3];
                let mut g = Game::new(GameConfig {
                    layout,
                    ..GameConfig::default()
                });
                for _ in 0..4 {
                    let ms: Vec<Move> = g
                        .legal_moves()
                        .into_iter()
                        .filter(|&m| !g.pos.is_push_off(m))
                        .collect();
                    g.play_unchecked(ms[rng.below(ms.len())]);
                }
                let start = g.ply();
                let mut player = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
                let mut depths = Vec::new();
                while !g.is_over() {
                    let r = player.search(&g, timed(play_ms));
                    depths.push(r.depth);
                    g.play_unchecked(r.best.unwrap());
                }
                let avg_depth = depths.iter().sum::<u32>() as f64 / depths.len() as f64;
                eprintln!(
                    "game {gi} ({}) done: {} plies, outcome {:?}, avg depth {avg_depth:.1}",
                    layout.name(),
                    g.ply(),
                    g.outcome()
                );
                // 복기: 기록을 되짚으며 기준 탐색
                let moves: Vec<Move> = g.history.iter().map(|x| x.0).collect();
                let mut r = Game::new(GameConfig {
                    layout,
                    ..GameConfig::default()
                });
                for &m in &moves[..start] {
                    r.play_unchecked(m);
                }
                let mut referee = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
                for (k, &m) in moves[start..].iter().enumerate() {
                    let root = referee.search(&r, timed(ref_ms));
                    let best = root.score;
                    let loss = if Some(m) == root.best {
                        0
                    } else {
                        let mut c = r.clone();
                        c.play_unchecked(m);
                        let played = if c.is_over() {
                            // 둔 수로 바로 끝났으면 그 결과
                            match c.outcome() {
                                abalone_engine::Outcome::Win(w, _) if w == r.pos.side => WIN_BOUND,
                                abalone_engine::Outcome::Win(..) => -WIN_BOUND,
                                _ => 0,
                            }
                        } else {
                            -referee.search(&c, timed(ref_ms)).score
                        };
                        (clamp(best) - clamp(played)).max(0)
                    };
                    let note = format!(
                        "g{gi} ply {:>3} {} played {} best {} loss {loss} (ref depth {})",
                        start + k,
                        if r.pos.side == abalone_engine::Color::Black {
                            "B"
                        } else {
                            "W"
                        },
                        move_to_string(m),
                        root.best.map(move_to_string).unwrap_or_default(),
                        root.depth
                    );
                    losses
                        .lock()
                        .unwrap()
                        .push((loss, gi as u32, root.depth, note));
                    r.play_unchecked(m);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let mut v = Arc::try_unwrap(losses).ok().unwrap().into_inner().unwrap();
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    let n = v.len();
    let count = |th: i32| v.iter().filter(|x| x.0 >= th).count();
    let avg: f64 = v.iter().map(|x| x.0 as f64).sum::<f64>() / n as f64;
    let ref_depth: f64 = v.iter().map(|x| x.2 as f64).sum::<f64>() / n as f64;
    for x in v.iter().take(12) {
        println!("{}", x.3);
    }
    println!(
        "play {play_ms}ms vs reference {ref_ms}ms (avg ref depth {ref_depth:.1}): {n} moves, \
         avg loss {avg:.0}, best-move agreement {:.0}%, loss>=100: {}, >=300: {}, >=700 (about a marble): {}",
        100.0 * v.iter().filter(|x| x.0 == 0).count() as f64 / n as f64,
        count(100),
        count(300),
        count(700)
    );
}
