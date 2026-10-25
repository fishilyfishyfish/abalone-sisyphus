//! 시작 배치별 승률: 같은 설정끼리 자기대국을 두고 흑(선)·백 승률을 배치마다 잰다. 4스레드 병렬.
//! 판마다 다른 수순이 되도록 처음 몇 수는 무작위로 둔다(밀어내기는 피함, 짝수면 흑백 같은 수만큼).
//!
//! 실행: cargo run --release --example layout_stats -- <설정> [배치당 판 수=60] [무작위 시작 수=2] [JSON 출력 경로]
//! 설정: d<깊이> (예 d3) | t<ms> (예 t500)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use abalone_engine::board::{Color, Move};
use abalone_engine::eval::DEFAULT_WEIGHTS;
use abalone_engine::game::{Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

const LAYOUTS: [Layout; 3] = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];

fn limits(spec: &str) -> Limits {
    if let Some(d) = spec.strip_prefix('d') {
        Limits::depth(d.parse().expect("depth"))
    } else if let Some(t) = spec.strip_prefix('t') {
        Limits {
            max_depth: 64,
            time_ms: t.parse().expect("ms"),
            exact_root: false,
        }
    } else {
        panic!("unknown setting {spec}")
    }
}

#[derive(Default, Clone)]
struct Tally {
    black: u32,
    draw: u32,
    white: u32,
    plies: u64,
    pushed_black: u32,
    pushed_white: u32,
    ends: std::collections::BTreeMap<&'static str, u32>,
}

impl Tally {
    fn games(&self) -> u32 {
        self.black + self.draw + self.white
    }

    /// 흑 점수율과 95% 구간 (무승부 = 반 판)
    fn black_score(&self) -> (f64, f64, f64) {
        let n = self.games() as f64;
        let s = (self.black as f64 + self.draw as f64 / 2.0) / n;
        let se = (s * (1.0 - s) / n).sqrt().max(0.5 / n);
        (s, (s - 1.96 * se).max(0.0), (s + 1.96 * se).min(1.0))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let spec = args.get(1).map_or("d3", |s| s).to_string();
    let per_layout: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let open_plies: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2);
    let out_path = args.get(4).cloned();
    let lim = limits(&spec);

    let jobs = Arc::new(
        (0..per_layout * 3)
            .map(|i| (LAYOUTS[i % 3], i as u64))
            .collect::<Vec<_>>(),
    );
    let next = Arc::new(AtomicUsize::new(0));
    let tallies = Arc::new(Mutex::new(vec![Tally::default(); 3]));
    let started = std::time::Instant::now();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (jobs, next, tallies) = (jobs.clone(), next.clone(), tallies.clone());
            std::thread::spawn(move || {
                let mut s = Searcher::with_std_clock(20, DEFAULT_WEIGHTS);
                loop {
                    let j = next.fetch_add(1, Ordering::SeqCst);
                    if j >= jobs.len() {
                        break;
                    }
                    let (layout, seed) = jobs[j];
                    let mut rng = Rng::new(0xA11CE + seed);
                    let mut g = Game::new(GameConfig {
                        layout,
                        ..GameConfig::default()
                    });
                    for _ in 0..open_plies {
                        let ms: Vec<Move> = g
                            .legal_moves()
                            .into_iter()
                            .filter(|&m| !g.pos.is_push_off(m))
                            .collect();
                        g.play_unchecked(ms[rng.below(ms.len())]);
                    }
                    s.clear();
                    while !g.is_over() {
                        let m = s.search(&g, lim).best.expect("move");
                        g.play_unchecked(m);
                    }
                    let li = LAYOUTS.iter().position(|&l| l == layout).unwrap();
                    let mut t = tallies.lock().unwrap();
                    let t = &mut t[li];
                    match g.outcome() {
                        Outcome::Win(Color::Black, r) => {
                            t.black += 1;
                            *t.ends.entry(r.name()).or_insert(0) += 1;
                        }
                        Outcome::Win(Color::White, r) => {
                            t.white += 1;
                            *t.ends.entry(r.name()).or_insert(0) += 1;
                        }
                        Outcome::Draw(r) => {
                            t.draw += 1;
                            *t.ends.entry(r.name()).or_insert(0) += 1;
                        }
                        Outcome::Ongoing => unreachable!(),
                    }
                    t.plies += g.ply() as u64;
                    t.pushed_black += g.pos.pushed_off_by(Color::Black);
                    t.pushed_white += g.pos.pushed_off_by(Color::White);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let tallies = tallies.lock().unwrap().clone();
    let secs = started.elapsed().as_secs_f64();
    println!("setting {spec}, {per_layout} games per layout, {open_plies} random opening plies, {secs:.0}s");
    let mut rows = Vec::new();
    for (l, t) in LAYOUTS.iter().zip(&tallies) {
        let n = t.games();
        let (s, lo, hi) = t.black_score();
        println!(
            "{:<8} black {:>3}  draw {:>3}  white {:>3}  black score {:.1}% (95% {:.1}~{:.1})  avg {:.0} plies  pushed {}:{}  ends {:?}",
            l.name(),
            t.black,
            t.draw,
            t.white,
            s * 100.0,
            lo * 100.0,
            hi * 100.0,
            t.plies as f64 / n as f64,
            t.pushed_black,
            t.pushed_white,
            t.ends
        );
        let ends: Vec<String> = t.ends.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        rows.push(format!(
            "    \"{}\": {{\"games\":{n},\"black\":{},\"draw\":{},\"white\":{},\"avg_plies\":{:.0},\"pushed_black\":{},\"pushed_white\":{},\"ends\":{{{}}}}}",
            l.name(),
            t.black,
            t.draw,
            t.white,
            t.plies as f64 / n as f64,
            t.pushed_black,
            t.pushed_white,
            ends.join(",")
        ));
    }
    if let Some(p) = out_path {
        let json = format!(
            "{{\n  \"setting\": \"{spec}\",\n  \"opening_plies\": {open_plies},\n  \"move_limit\": {},\n  \"layouts\": {{\n{}\n  }}\n}}\n",
            GameConfig::default().move_limit,
            rows.join(",\n")
        );
        std::fs::write(&p, json).expect("write");
        println!("wrote {p}");
    }
}
