//! 강도 측정: 두 설정을 무작위 오프닝 쌍으로 붙여 Elo 차이를 잰다.
//! 같은 오프닝을 색만 바꿔 두 번 두므로 선후 유불리가 상쇄된다. 4스레드 병렬.
//!
//! 실행: cargo run --release --example strength -- <A> <B> [오프닝 쌍 수=20] [무작위 시작 수=4]
//! 설정: random | easy | easy<점수 폭> (예 easy600) | d<깊이> (예 d3) | t<ms> (예 t1500, 깊이 제한 없음)
//! d·t 뒤에 p를 붙이면 가지치기(널 무브, LMR)를 쓴다: t500p, d6p. 그 뒤에 n을 붙이면 보정 신경망: t500pn
//! 뒤에 @가중치를 붙이면 그 가중치로 둔다: d3@v0, t500@w:1000,-20,10,-15,-75,75,-30

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use abalone_engine::ai::{self, Level};
use abalone_engine::board::{Color, Move};
use abalone_engine::eval::{weights_by_name, Weights};
use abalone_engine::game::{Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

#[derive(Clone, Copy, Debug)]
enum Player {
    Random,
    Easy,
    EasyMargin(i32),
    Depth(u32, bool),
    Time(f64, bool),
}

/// (설정, 가중치, 보정 신경망 사용, 신경망 파일). 설정 끝의 n은 보정 신경망(예: t1500pn),
/// 맨 끝 `#경로`는 내장 대신 그 파일의 신경망(예: t1500pn#/tmp/net.txt).
fn parse_spec(spec: &str) -> (Player, Weights, bool, Option<String>) {
    let (spec, net) = match spec.split_once('#') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (spec, None),
    };
    let (p, w) = spec.split_once('@').unwrap_or((spec, "default"));
    let (p, nn) = match p.strip_suffix('n') {
        Some(q) => (q, true),
        None => (p, false),
    };
    (
        parse(p),
        weights_by_name(w).expect("unknown weights"),
        nn,
        net,
    )
}

fn parse(s: &str) -> Player {
    match s {
        "random" => Player::Random,
        "easy" => Player::Easy,
        _ if s.starts_with("easy") => Player::EasyMargin(s[4..].parse().expect("margin")),
        _ if s.starts_with('d') => {
            let p = s.ends_with('p');
            Player::Depth(s[1..s.len() - p as usize].parse().expect("depth"), p)
        }
        _ if s.starts_with('t') => {
            let p = s.ends_with('p');
            Player::Time(s[1..s.len() - p as usize].parse().expect("ms"), p)
        }
        _ => panic!("unknown player {s}"),
    }
}

fn pick(g: &Game, p: Player, s: &mut Searcher, rng: &mut Rng) -> Move {
    match p {
        Player::Random => ai::choose(g, Level::Random, s, rng).unwrap().0,
        Player::Easy => ai::choose(g, Level::Easy, s, rng).unwrap().0,
        Player::EasyMargin(m) => ai::choose_easy(g, s, rng, m).unwrap().0,
        Player::Depth(d, p) => s
            .search(
                g,
                Limits {
                    prune: p,
                    ..Limits::depth(d)
                },
            )
            .best
            .unwrap(),
        Player::Time(t, p) => s
            .search(
                g,
                Limits {
                    max_depth: 64,
                    time_ms: t,
                    exact_root: false,
                    prune: p,
                },
            )
            .best
            .unwrap(),
    }
}

/// 무작위 시작 수순 (밀어내기는 피한다)
fn opening(layout: Layout, plies: usize, rng: &mut Rng) -> Vec<Move> {
    let mut g = Game::new(GameConfig {
        layout,
        ..GameConfig::default()
    });
    let mut line = Vec::new();
    for _ in 0..plies {
        let ms: Vec<Move> = g
            .legal_moves()
            .into_iter()
            .filter(|&m| !g.pos.is_push_off(m))
            .collect();
        let m = ms[rng.below(ms.len())];
        g.play_unchecked(m);
        line.push(m);
    }
    line
}

/// Elo 차이 (점수율 s, 판 수 n). 전승·전패는 반 판을 빼서 하한만 낸다.
pub fn elo(s: f64, n: f64) -> f64 {
    let s = s.clamp(0.5 / n, 1.0 - 0.5 / n);
    -400.0 * (1.0 / s - 1.0).log10()
}

struct GameResult {
    idx: usize,
    a_color: Color,
    layout: Layout,
    outcome: Outcome,
    plies: usize,
    pushed_a: u32,
    pushed_b: u32,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a_spec = args.get(1).map_or("d3", |s| s).to_string();
    let b_spec = args.get(2).map_or("d2", |s| s).to_string();
    let (a, wa_, na, fa) = parse_spec(&a_spec);
    let (b, wb_, nb, fb) = parse_spec(&b_spec);
    let load = |f: &Option<String>| {
        f.as_ref()
            .map(|p| Arc::new(abalone_engine::nn::Net::load(p).expect("net file")))
    };
    let (neta, netb) = (load(&fa), load(&fb));
    let pairs: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
    let open_plies: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(4);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    let mut orng = Rng::new(777);
    let openings: Vec<(Layout, Vec<Move>)> = (0..pairs)
        .map(|i| {
            let l = layouts[i % 3];
            (l, opening(l, open_plies, &mut orng))
        })
        .collect();
    let openings = Arc::new(openings);
    let next = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(Mutex::new(Vec::new()));
    let threads: Vec<_> = (0..4)
        .map(|t| {
            let (openings, next, results) = (openings.clone(), next.clone(), results.clone());
            let (neta, netb) = (neta.clone(), netb.clone());
            std::thread::spawn(move || {
                let mut sa = Searcher::with_std_clock(20, wa_);
                let mut sb = Searcher::with_std_clock(20, wb_);
                sa.nn = na;
                sb.nn = nb;
                sa.net = neta.clone();
                sb.net = netb.clone();
                let mut rng = Rng::new(1000 + t);
                loop {
                    let job = next.fetch_add(1, Ordering::SeqCst);
                    if job >= openings.len() * 2 {
                        break;
                    }
                    let (layout, line) = &openings[job / 2];
                    let a_color = if job % 2 == 0 {
                        Color::Black
                    } else {
                        Color::White
                    };
                    let mut g = Game::new(GameConfig {
                        layout: *layout,
                        ..GameConfig::default()
                    });
                    for &m in line {
                        g.play_unchecked(m);
                    }
                    sa.clear();
                    sb.clear();
                    while !g.is_over() {
                        let m = if g.pos.side == a_color {
                            pick(&g, a, &mut sa, &mut rng)
                        } else {
                            pick(&g, b, &mut sb, &mut rng)
                        };
                        g.play_unchecked(m);
                    }
                    results.lock().unwrap().push(GameResult {
                        idx: job,
                        a_color,
                        layout: *layout,
                        outcome: g.outcome(),
                        plies: g.ply(),
                        pushed_a: g.pos.pushed_off_by(a_color),
                        pushed_b: g.pos.pushed_off_by(a_color.other()),
                    });
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let mut res = Arc::try_unwrap(results).ok().unwrap().into_inner().unwrap();
    res.sort_by_key(|r| r.idx);
    let (mut w, mut d, mut l) = (0, 0, 0);
    let (mut pa, mut pb, mut plies) = (0, 0, 0);
    let mut reasons = std::collections::BTreeMap::new();
    for r in &res {
        let tag = match r.outcome {
            Outcome::Win(c, why) => {
                *reasons.entry(why.name()).or_insert(0) += 1;
                if c == r.a_color {
                    w += 1;
                    format!("A ({})", why.name())
                } else {
                    l += 1;
                    format!("B ({})", why.name())
                }
            }
            Outcome::Draw(why) => {
                *reasons.entry(why.name()).or_insert(0) += 1;
                d += 1;
                format!("draw ({})", why.name())
            }
            Outcome::Ongoing => unreachable!(),
        };
        pa += r.pushed_a;
        pb += r.pushed_b;
        plies += r.plies;
        println!(
            "game {:>3} {:<9} A={:?}: {tag:<20} {:>3} plies, pushed {}:{}",
            r.idx,
            r.layout.name(),
            r.a_color,
            r.plies,
            r.pushed_a,
            r.pushed_b
        );
    }
    let n = res.len() as f64;
    let s = (w as f64 + d as f64 / 2.0) / n;
    let se = (s * (1.0 - s) / n).sqrt().max(0.5 / n);
    println!(
        "{a_spec} vs {b_spec}: +{w} ={d} -{l}  score {:.1}%  Elo {:+.0} (95%: {:+.0} ~ {:+.0})  pushed {pa}:{pb}  avg {:.0} plies  ends {:?}",
        s * 100.0,
        elo(s, n),
        elo(s - 1.96 * se, n),
        elo(s + 1.96 * se, n),
        plies as f64 / n,
        reasons
    );
}
