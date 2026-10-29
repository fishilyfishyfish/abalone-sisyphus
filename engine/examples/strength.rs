//! 강도 측정: 두 설정을 무작위 오프닝 쌍으로 붙여 Elo 차이를 잰다.
//! 같은 오프닝을 색만 바꿔 두 번 두므로 선후 유불리가 상쇄된다. 4스레드 병렬.
//!
//! 실행: cargo run --release --example strength -- <A> <B> [오프닝 쌍 수=20] [무작위 시작 수=4]
//! 설정: random | easy | easy<점수 폭> (예 easy600) | d<깊이> (예 d3) | t<ms> (예 t1500, 깊이 제한 없음)
//! d·t 뒤에 p를 붙이면 가지치기(널 무브, LMR)를 쓴다: t500p, d6p. 그 뒤에 n을 붙이면 보정 신경망: t500pn
//! 뒤에 @가중치를 붙이면 그 가중치로 둔다: d3@v0, t500@w:1000,-20,10,-15,-75,75,-30
//! 맨 끝에 +ms를 붙이면 상대 차례마다 그만큼 미리 생각한다(150ms 조각, 웹과 같은 방식): t1500pn+3000

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use abalone_engine::ai::{self, Level};
use abalone_engine::board::{Color, Move};
use abalone_engine::eval::{weights_by_name, Weights};
use abalone_engine::game::{Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Ponder, Searcher};

#[derive(Clone, Copy, Debug)]
enum Player {
    Random,
    Easy,
    EasyMargin(i32),
    Depth(u32, bool),
    Time(f64, bool),
}

/// (설정, 가중치, 보정 신경망 사용, 신경망 파일, 미리 생각 ms). 설정 끝의 n은 보정 신경망(예: t1500pn),
/// 맨 끝 `#경로`는 내장 대신 그 파일의 신경망(예: t1500pn#/tmp/net.txt).
fn parse_spec(spec: &str) -> (Player, Weights, bool, Option<String>, f64) {
    let (spec, net) = match spec.split_once('#') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (spec, None),
    };
    let (spec, ponder) = match spec.split_once('+') {
        Some((a, b)) => (a, b.parse().expect("ponder ms")),
        None => (spec, 0.0),
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
        ponder,
    )
}

/// 상대가 생각하는 동안 미리 생각하기. 웹 워커처럼 짧은 조각으로 나눠 부른다.
const PONDER_SLICE: f64 = 150.0;
fn ponder(g: &Game, p: Player, s: &mut Searcher, pd: &mut Ponder, ms: f64) {
    let prune = matches!(p, Player::Time(_, true) | Player::Depth(_, true));
    let mut left = ms;
    while left > 0.0 {
        pd.slice(s, g, left.min(PONDER_SLICE), prune);
        left -= PONDER_SLICE;
    }
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

/// (수, 도달 깊이). 깊이는 탐색하는 설정만 (나머지는 0)
fn pick(g: &Game, p: Player, s: &mut Searcher, rng: &mut Rng) -> (Move, u32) {
    let r = match p {
        Player::Random => return (ai::choose(g, Level::Random, s, rng).unwrap().0, 0),
        Player::Easy => return (ai::choose(g, Level::Easy, s, rng).unwrap().0, 0),
        Player::EasyMargin(m) => return (ai::choose_easy(g, s, rng, m).unwrap().0, 0),
        Player::Depth(d, p) => s.search(
            g,
            Limits {
                prune: p,
                ..Limits::depth(d)
            },
        ),
        Player::Time(t, p) => s.search(
            g,
            Limits {
                max_depth: 64,
                time_ms: t,
                exact_root: false,
                prune: p,
            },
        ),
    };
    (r.best.unwrap(), r.depth)
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
    /// A가 탐색한 수의 (도달 깊이 합, 수 개수)
    depth_a: (u64, u64),
    /// A가 미리 생각할 때 상대 수가 후보 안에 있었던 (횟수, 전체)
    hits_a: (u64, u64),
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a_spec = args.get(1).map_or("d3", |s| s).to_string();
    let b_spec = args.get(2).map_or("d2", |s| s).to_string();
    let (a, wa_, na, fa, pa_ms) = parse_spec(&a_spec);
    let (b, wb_, nb, fb, pb_ms) = parse_spec(&b_spec);
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
                    let mut depth_a = (0, 0);
                    let mut hits_a = (0, 0);
                    let (mut pda, mut pdb) = (Ponder::new(), Ponder::new());
                    while !g.is_over() {
                        let m = if g.pos.side == a_color {
                            if pb_ms > 0.0 {
                                ponder(&g, b, &mut sb, &mut pdb, pb_ms);
                            }
                            let (m, d) = pick(&g, a, &mut sa, &mut rng);
                            depth_a.0 += d as u64;
                            depth_a.1 += 1;
                            m
                        } else {
                            let m = pick(&g, b, &mut sb, &mut rng).0;
                            if pa_ms > 0.0 {
                                ponder(&g, a, &mut sa, &mut pda, pa_ms);
                                hits_a.0 += pda.moves.contains(&m) as u64;
                                hits_a.1 += 1;
                            }
                            m
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
                        depth_a,
                        hits_a,
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
    let (mut dsum, mut dn) = (0, 0);
    let (mut hit, mut hn) = (0, 0);
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
        dsum += r.depth_a.0;
        dn += r.depth_a.1;
        hit += r.hits_a.0;
        hn += r.hits_a.1;
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
        "{a_spec} vs {b_spec}: +{w} ={d} -{l}  score {:.1}%  Elo {:+.0} (95%: {:+.0} ~ {:+.0})  pushed {pa}:{pb}  avg {:.0} plies  ends {:?}  A avg depth {:.2}  A ponder hits {hit}/{hn}",
        s * 100.0,
        elo(s, n),
        elo(s - 1.96 * se, n),
        elo(s + 1.96 * se, n),
        plies as f64 / n,
        reasons,
        dsum as f64 / dn.max(1) as f64
    );
}
