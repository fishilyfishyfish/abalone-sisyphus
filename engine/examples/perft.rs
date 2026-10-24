//! 성능 측정: perft(합법수 트리 노드 수)와 특징값 계산 속도.
//! 실행: cargo run --release --example perft -- [깊이]

use std::time::Instant;

use abalone_engine::board::*;
use abalone_engine::features;
use abalone_engine::layouts::Layout;

fn perft(p: &mut Position, depth: u32, buf: &mut Vec<Vec<Move>>) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = buf.pop().unwrap_or_default();
    moves.clear();
    p.legal_moves_into(&mut moves);
    let n = if depth == 1 {
        moves.len() as u64
    } else {
        let mut n = 0;
        for &m in &moves {
            let u = p.make_move(m);
            n += perft(p, depth - 1, buf);
            p.unmake_move(u);
        }
        n
    };
    buf.push(moves);
    n
}

fn main() {
    let depth: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    for l in [Layout::Standard, Layout::BelgianDaisy, Layout::GermanDaisy] {
        let mut p = l.position();
        let mut buf = Vec::new();
        for d in 1..=depth {
            let t = Instant::now();
            let n = perft(&mut p, d, &mut buf);
            let s = t.elapsed().as_secs_f64();
            println!(
                "{:<9} perft({d}) = {n:>12}  {:>8.3}s  {:>7.1} Mnps",
                l.name(),
                s,
                n as f64 / s / 1e6
            );
        }
    }
    let p = Layout::BelgianDaisy.position();
    let iters = 200_000;
    let t = Instant::now();
    let mut acc = 0i64;
    for _ in 0..iters {
        let f = features::compute(std::hint::black_box(&p));
        acc += f.black.cohesion as i64;
    }
    let s = t.elapsed().as_secs_f64();
    println!(
        "features::compute: {:.2} us/call ({acc})",
        s / iters as f64 * 1e6
    );
}
