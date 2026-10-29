//! 자기대국 기반 가중치 튜닝 (텍셀 방식).
//!
//! 평가함수가 특징값 차이의 선형 결합이므로, "국면의 특징값 차이 → 그 대국의 최종 결과"를
//! 로지스틱 회귀로 맞추면 가중치가 나온다. 볼록 문제라 뉴턴법으로 바로 수렴한다.
//!
//! cargo run --release --example tune -- gen <판 수> <설정> <무작위 시작 수> <출력.csv> [시드]
//!   설정은 strength와 같다 (예: d3@v0, 가지치기는 d5p). 조용한 국면(둘 차례에 밀어내기 수가 없는 국면)만 기록.
//!   한 줄 = 둘 차례 관점 특징값 차이, 둘 차례 관점 결과(1 / 0.5 / 0).
//!   끝에 board를 붙이면(시드 다음) 결과 앞에 둘 차례 쪽·상대 구슬 비트(16진)도 쓴다 (신경망 학습용, tools/nn).
//! cargo run --release --example tune -- fit <csv>... [비교할 가중치=default]
//!   pushed_off 계수를 1000으로 맞춘 가중치를 낸다.
//!   endangered/attackable는 차이로 보면 정확히 부호만 반대라 한 계수로 맞추고 반씩 나눈다.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use abalone_engine::board::{gen_push_offs, Move};
use abalone_engine::eval::{weights_by_name, Weights, NUM_FEATURES};
use abalone_engine::features;
use abalone_engine::game::{Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

type Row = ([i32; NUM_FEATURES], f64);
/// gen이 모으는 한 국면: 특징값 차이, 결과, 둘 차례 쪽 구슬, 상대 구슬
type PosRow = ([i32; NUM_FEATURES], f64, u128, u128);

fn parse_spec(spec: &str) -> (Limits, Weights, bool) {
    let (p, w) = spec.split_once('@').unwrap_or((spec, "default"));
    // 끝에 n을 붙이면 보정 신경망(내장 가중치), 그 앞의 p는 가지치기 탐색 (예: d5p, d5pn)
    let (p, nn) = match p.strip_suffix('n') {
        Some(q) => (q, true),
        None => (p, false),
    };
    let (p, prune) = match p.strip_suffix('p') {
        Some(q) => (q, true),
        None => (p, false),
    };
    let lim = if let Some(d) = p.strip_prefix('d') {
        Limits::depth(d.parse().expect("depth"))
    } else if let Some(t) = p.strip_prefix('t') {
        Limits {
            max_depth: 64,
            time_ms: t.parse().expect("ms"),
            exact_root: false,
            prune: false,
        }
    } else {
        panic!("spec must be d<depth> or t<ms>")
    };
    let lim = Limits { prune, ..lim };
    (lim, weights_by_name(w).expect("weights"), nn)
}

fn gen(games: usize, spec: &str, open_plies: usize, out: &str, seed: u64, with_board: bool) {
    let (lim, w, nn) = parse_spec(spec);
    let layouts = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];
    let next = Arc::new(AtomicUsize::new(0));
    let rows = Arc::new(Mutex::new(Vec::<PosRow>::new()));
    let outcomes = Arc::new(Mutex::new(
        std::collections::BTreeMap::<String, usize>::new(),
    ));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (next, rows, outcomes) = (next.clone(), rows.clone(), outcomes.clone());
            std::thread::spawn(move || {
                let mut s = Searcher::with_std_clock(20, w);
                s.nn = nn;
                loop {
                    let gi = next.fetch_add(1, Ordering::SeqCst);
                    if gi >= games {
                        break;
                    }
                    let mut rng = Rng::new(seed.wrapping_mul(1_000_003) + gi as u64);
                    let layout = layouts[gi % 3];
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
                    let mut local: Vec<([i32; NUM_FEATURES], abalone_engine::Color, u128, u128)> =
                        Vec::new();
                    let mut buf = Vec::new();
                    while !g.is_over() {
                        buf.clear();
                        gen_push_offs(g.pos.own(), g.pos.opp(), &mut buf);
                        if buf.is_empty() {
                            local.push((
                                features::diff(&g.pos, g.pos.side),
                                g.pos.side,
                                g.pos.own(),
                                g.pos.opp(),
                            ));
                        }
                        let m = s.search(&g, lim).best.unwrap();
                        g.play_unchecked(m);
                    }
                    let tag = match g.outcome() {
                        Outcome::Win(_, r) => format!("win:{}", r.name()),
                        Outcome::Draw(r) => format!("draw:{}", r.name()),
                        Outcome::Ongoing => unreachable!(),
                    };
                    *outcomes.lock().unwrap().entry(tag).or_insert(0) += 1;
                    let mut r = rows.lock().unwrap();
                    for (d, side, own, opp) in local {
                        let y = match g.outcome() {
                            Outcome::Win(c, _) if c == side => 1.0,
                            Outcome::Win(..) => 0.0,
                            _ => 0.5,
                        };
                        r.push((d, y, own, opp));
                    }
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let rows = rows.lock().unwrap();
    let mut f = std::io::BufWriter::new(std::fs::File::create(out).expect("create"));
    for (d, y, own, opp) in rows.iter() {
        let v: Vec<String> = d.iter().map(|x| x.to_string()).collect();
        if with_board {
            // 신경망 학습용: 특징값 차이, 둘 차례 쪽 구슬·상대 구슬(패딩 칸 번호 비트, 16진), 결과
            writeln!(f, "{},{own:x},{opp:x},{}", v.join(","), y).unwrap();
        } else {
            writeln!(f, "{},{}", v.join(","), y).unwrap();
        }
    }
    eprintln!(
        "{games} games, {} quiet positions -> {out}; outcomes {:?}",
        rows.len(),
        outcomes.lock().unwrap()
    );
}

fn read(paths: &[String]) -> Vec<Row> {
    let mut rows = Vec::new();
    for p in paths {
        let f = std::io::BufReader::new(std::fs::File::open(p).expect("open"));
        for line in f.lines() {
            let line = line.unwrap();
            let v: Vec<&str> = line.split(',').collect();
            let d: [i32; NUM_FEATURES] = std::array::from_fn(|i| v[i].parse().unwrap());
            rows.push((d, v[NUM_FEATURES].parse().unwrap()));
        }
    }
    rows
}

/// 회귀 입력 개수: attackable(5)은 endangered(4)와 차이가 정확히 부호만 반대라 빼고 합친다.
const K: usize = NUM_FEATURES - 1;
/// 회귀 j번째 계수가 가리키는 특징 번호 (attackable 건너뜀)
const COLS: [usize; K] = {
    let mut c = [0; K];
    let mut j = 0;
    while j < K {
        c[j] = if j < 5 { j } else { j + 1 };
        j += 1;
    }
    c
};

fn x_of(d: &[i32; NUM_FEATURES]) -> [f64; K] {
    std::array::from_fn(|j| d[COLS[j]] as f64)
}

/// 가중치를 회귀 계수 꼴로 (합친 항 = w_end − w_att)
fn base_of(w: &Weights) -> [f64; K] {
    std::array::from_fn(|j| {
        if COLS[j] == 4 {
            (w.0[4] - w.0[5]) as f64
        } else {
            w.0[COLS[j]] as f64
        }
    })
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

fn log_loss(rows: &[Row], beta: &[f64; K]) -> f64 {
    let mut l = 0.0;
    for (d, y) in rows {
        let x = x_of(d);
        let z: f64 = (0..K).map(|j| beta[j] * x[j]).sum();
        let p = sigmoid(z).clamp(1e-12, 1.0 - 1e-12);
        l -= y * p.ln() + (1.0 - y) * (1.0 - p).ln();
    }
    l / rows.len() as f64
}

/// K×K 연립방정식 (가우스 소거)
fn solve(mut a: [[f64; K]; K], mut b: [f64; K]) -> [f64; K] {
    for i in 0..K {
        let piv = (i..K)
            .max_by(|&x, &y| a[x][i].abs().total_cmp(&a[y][i].abs()))
            .unwrap();
        a.swap(i, piv);
        b.swap(i, piv);
        for r in 0..K {
            if r != i {
                let f = a[r][i] / a[i][i];
                let pivot_row = a[i];
                for (x, p) in a[r].iter_mut().zip(pivot_row).skip(i) {
                    *x -= f * p;
                }
                b[r] -= f * b[i];
            }
        }
    }
    std::array::from_fn(|i| b[i] / a[i][i])
}

/// 설명 문장이 상식과 같은 방향이 되도록 정한 부호 (회귀 계수 순서):
/// 밀어낸 구슬 +, 중앙 거리 −, 뭉침 +, 가장자리 −, 밀려날 위험 −, 고립 −,
/// 밀기 기회 +, 3개 줄 +, 덩어리 수 −, 움직임 +.
/// 회귀가 반대 부호를 내면 그 항목을 0에 묶고 다시 맞춘다.
const SIGN: [f64; K] = [1.0, -1.0, 1.0, -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0];

fn fit_constrained(rows: &[Row], lambda: f64) -> ([f64; K], Vec<usize>) {
    let mut fixed = Vec::new();
    loop {
        let beta = fit_newton_masked(rows, lambda, &fixed);
        let bad: Vec<usize> = (0..K)
            .filter(|&j| !fixed.contains(&j) && beta[j] * SIGN[j] < 0.0)
            .collect();
        if bad.is_empty() {
            return (beta, fixed);
        }
        // 가장 크게 어긋난 항목부터 하나씩 묶는다
        let worst = *bad
            .iter()
            .max_by(|&&a, &&b| {
                (beta[a] * SIGN[a])
                    .total_cmp(&(beta[b] * SIGN[b]))
                    .reverse()
            })
            .unwrap();
        fixed.push(worst);
    }
}

fn fit_newton(rows: &[Row], lambda: f64) -> [f64; K] {
    fit_newton_masked(rows, lambda, &[])
}

/// fixed에 든 계수는 0으로 둔 채 나머지만 맞춘다
fn fit_newton_masked(rows: &[Row], lambda: f64, fixed: &[usize]) -> [f64; K] {
    let mut beta = [0.0; K];
    let n = rows.len() as f64;
    for _ in 0..30 {
        let mut g = [0.0; K];
        let mut h = [[0.0; K]; K];
        for (d, y) in rows {
            let x = x_of(d);
            let z: f64 = (0..K).map(|j| beta[j] * x[j]).sum();
            let p = sigmoid(z);
            let wgt = p * (1.0 - p);
            for j in 0..K {
                g[j] += (p - y) * x[j] / n;
                for k in 0..K {
                    h[j][k] += wgt * x[j] * x[k] / n;
                }
            }
        }
        for j in 0..K {
            g[j] += 2.0 * lambda * beta[j];
            h[j][j] += 2.0 * lambda;
        }
        for &j in fixed {
            g[j] = 0.0;
            h[j] = [0.0; K];
            for row in h.iter_mut() {
                row[j] = 0.0;
            }
            h[j][j] = 1.0;
        }
        let step = solve(h, g);
        for j in 0..K {
            beta[j] -= step[j];
        }
        if step.iter().map(|s| s.abs()).fold(0.0, f64::max) < 1e-10 {
            break;
        }
    }
    beta
}

/// 주어진 가중치를 그대로 쓰고 점수 배율 k만 맞췄을 때의 손실
fn loss_of_weights(rows: &[Row], w: &Weights) -> (f64, f64) {
    let base = base_of(w);
    let mut best = (f64::MAX, 0.0);
    let mut k = 1e-5;
    while k < 1e-2 {
        let beta: [f64; K] = std::array::from_fn(|j| base[j] * k);
        let l = log_loss(rows, &beta);
        if l < best.0 {
            best = (l, k);
        }
        k *= 1.05;
    }
    best
}

fn to_weights(beta: &[f64; K]) -> Weights {
    let s = 1000.0 / beta[0];
    let mut w = [0; NUM_FEATURES];
    for j in 0..K {
        let v = (beta[j] * s).round() as i32;
        if COLS[j] == 4 {
            // 합친 계수 c = w_end − w_att 를 반씩: w_end = c/2, w_att = −c/2
            let half = (v as f64 / 2.0).round() as i32;
            w[4] = half;
            w[5] = -half;
        } else {
            w[COLS[j]] = v;
        }
    }
    w[0] = 1000;
    Weights(w)
}

fn fit(paths: &[String], compare: &str) {
    let rows = read(paths);
    for (d, _) in &rows {
        assert_eq!(
            d[4], -d[5],
            "endangered/attackable must be exact negatives in diff form"
        );
    }
    // 시드 고정 셔플 후 90/10 분할
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let mut rng = Rng::new(5);
    for i in (1..idx.len()).rev() {
        idx.swap(i, rng.below(i + 1));
    }
    let cut = rows.len() * 9 / 10;
    let train: Vec<Row> = idx[..cut].iter().map(|&i| rows[i]).collect();
    let test: Vec<Row> = idx[cut..].iter().map(|&i| rows[i]).collect();
    let draws = rows.iter().filter(|r| r.1 == 0.5).count();
    println!(
        "rows {} (train {}, test {}), draw rows {draws}",
        rows.len(),
        train.len(),
        test.len()
    );

    let free = fit_newton(&train, 1e-7);
    println!(
        "unconstrained: w:{}  test loss {:.5}",
        to_weights(&free).0.map(|x| x.to_string()).join(","),
        log_loss(&test, &free)
    );
    let (beta, fixed) = fit_constrained(&train, 1e-7);
    let w = to_weights(&beta);
    println!(
        "beta {:?}, fixed to 0: {:?}",
        beta.map(|b| (b * 1e6).round() / 1e6),
        fixed
    );
    println!(
        "fitted weights (sign-constrained): w:{}",
        w.0.map(|x| x.to_string()).join(",")
    );
    println!(
        "  train loss {:.5}  test loss {:.5}",
        log_loss(&train, &beta),
        log_loss(&test, &beta)
    );
    let cw = weights_by_name(compare).expect("compare weights");
    let (l_tr, k) = loss_of_weights(&train, &cw);
    let beta_c: [f64; K] = {
        let base = base_of(&cw);
        std::array::from_fn(|j| base[j] * k)
    };
    println!(
        "compare {compare} (w:{}) best scale {k:.2e}: train loss {l_tr:.5}  test loss {:.5}",
        cw.0.map(|x| x.to_string()).join(","),
        log_loss(&test, &beta_c)
    );
    // 기준선: 밀어낸 구슬 차이만
    let only_material = fit_newton(
        &train
            .iter()
            .map(|(d, y)| {
                let mut z = [0; NUM_FEATURES];
                z[0] = d[0];
                (z, *y)
            })
            .collect::<Vec<_>>(),
        1e-7,
    );
    println!(
        "material-only test loss {:.5}",
        log_loss(&test, &only_material)
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("gen") => gen(
            args[2].parse().unwrap(),
            &args[3],
            args[4].parse().unwrap(),
            &args[5],
            args.get(6).and_then(|s| s.parse().ok()).unwrap_or(1),
            args.get(7).is_some_and(|s| s == "board"),
        ),
        Some("fit") => {
            let (paths, compare): (Vec<String>, String) = match args.last() {
                Some(l) if !l.ends_with(".csv") => (args[2..args.len() - 1].to_vec(), l.clone()),
                _ => (args[2..].to_vec(), "default".to_string()),
            };
            fit(&paths, &compare)
        }
        _ => eprintln!("usage: tune gen <games> <spec> <open plies> <out.csv> [seed] | tune fit <csv>... [weights]"),
    }
}
