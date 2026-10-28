//! 외부 엔진과 대국: 무작위 오프닝 쌍, 색 교대, 4스레드(스레드마다 외부 프로세스 하나).
//!
//! 외부 엔진은 다음 한 줄 규약을 따르는 실행 파일이면 된다(표준 입출력):
//!   입력  `go <둘 차례 1|2> <최대 깊이> <수당 ms, 0이면 깊이만> <121자>`
//!   출력  `RESULT <121자> [수 이름]` 또는 `RESULT none`
//!   121자는 패딩 칸 번호(이 엔진의 sq) 순서이고 '.' 빈칸, '1' 흑, '2' 백, 'x' 판 밖.
//! 외부 엔진이 낸 국면이 되는 합법수를 찾아 두므로 수 표기를 맞출 필요가 없다.
//!
//! 실행: cargo run --release --example vs_external -- <외부 실행 파일> "<외부 인자>" <외부 깊이> <외부 ms>
//!       <우리 설정(strength와 같음, 예 t1500@v1)> [오프닝 쌍=10] [무작위 시작 수=4] [배치=standard|all]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use abalone_engine::board::{Color, Move, Position, VALID};
use abalone_engine::eval::{weights_by_name, Weights};
use abalone_engine::game::{Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::rng::Rng;
use abalone_engine::search::{Limits, Searcher};

struct External {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl External {
    fn spawn(path: &str, args: &str) -> External {
        let mut child = Command::new(path)
            .args(args.split_whitespace())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn external engine");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        External {
            _child: child,
            stdin,
            stdout,
        }
    }

    fn board_string(p: &Position) -> String {
        (0..121)
            .map(|s| {
                if (VALID >> s) & 1 == 0 {
                    'x'
                } else {
                    match p.at(s) {
                        Some(Color::Black) => '1',
                        Some(Color::White) => '2',
                        None => '.',
                    }
                }
            })
            .collect()
    }

    /// 외부 엔진에게 수를 받아 우리 합법수로 바꾼다.
    fn pick(&mut self, g: &Game, depth: u32, ms: u32) -> Move {
        let side = if g.pos.side == Color::Black { 1 } else { 2 };
        writeln!(
            self.stdin,
            "go {side} {depth} {ms} {}",
            Self::board_string(&g.pos)
        )
        .unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line).unwrap() == 0 {
                panic!("external engine exited");
            }
            if line.starts_with("RESULT") {
                break;
            }
        }
        let target = line.split_whitespace().nth(1).expect("RESULT board");
        assert_ne!(target, "none", "external engine found no move");
        for m in g.legal_moves() {
            let mut p = g.pos;
            p.make_move(m);
            if Self::board_string(&p) == target {
                return m;
            }
        }
        panic!(
            "external move is not legal here:\n{}\n-> {target}",
            Self::board_string(&g.pos)
        );
    }
}

fn parse_ours(spec: &str) -> (Limits, Weights) {
    let (p, w) = spec.split_once('@').unwrap_or((spec, "default"));
    let lim = if let Some(d) = p.strip_prefix('d') {
        Limits::depth(d.parse().unwrap())
    } else if let Some(t) = p.strip_prefix('t') {
        Limits {
            max_depth: 64,
            time_ms: t.parse().unwrap(),
            exact_root: false,
            prune: false,
        }
    } else {
        panic!("our spec must be d<depth> or t<ms>")
    };
    (lim, weights_by_name(w).expect("weights"))
}

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

fn elo(s: f64, n: f64) -> f64 {
    let s = s.clamp(0.5 / n, 1.0 - 0.5 / n);
    -400.0 * (1.0 / s - 1.0).log10()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, ext_args) = (a[1].clone(), a[2].clone());
    let ext_depth: u32 = a[3].parse().unwrap();
    let ext_ms: u32 = a[4].parse().unwrap();
    let ours_spec = a[5].clone();
    let pairs: usize = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(10);
    let open_plies: usize = a.get(7).and_then(|s| s.parse().ok()).unwrap_or(4);
    let layouts: Vec<Layout> = match a.get(8).map(|s| s.as_str()) {
        Some("all") => vec![Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy],
        _ => vec![Layout::Standard],
    };
    let (lim, w) = parse_ours(&ours_spec);
    let mut orng = Rng::new(4242);
    let openings: Arc<Vec<(Layout, Vec<Move>)>> = Arc::new(
        (0..pairs)
            .map(|i| {
                let l = layouts[i % layouts.len()];
                (l, opening(l, open_plies, &mut orng))
            })
            .collect(),
    );
    let next = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(Mutex::new(Vec::new()));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (openings, next, results) = (openings.clone(), next.clone(), results.clone());
            let (path, ext_args) = (path.clone(), ext_args.clone());
            std::thread::spawn(move || {
                let mut ext = External::spawn(&path, &ext_args);
                let mut s = Searcher::with_std_clock(20, w);
                loop {
                    let job = next.fetch_add(1, Ordering::SeqCst);
                    if job >= openings.len() * 2 {
                        break;
                    }
                    let (layout, line) = &openings[job / 2];
                    let ours = if job % 2 == 0 {
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
                    s.clear();
                    while !g.is_over() {
                        let m = if g.pos.side == ours {
                            s.search(&g, lim).best.unwrap()
                        } else {
                            ext.pick(&g, ext_depth, ext_ms)
                        };
                        g.play_unchecked(m);
                    }
                    let r = (
                        job,
                        *layout,
                        ours,
                        g.outcome(),
                        g.ply(),
                        g.pos.pushed_off_by(ours),
                        g.pos.pushed_off_by(ours.other()),
                    );
                    println!(
                        "game {:>3} {:<9} ours={:?}: {:?} {} plies, pushed {}:{}",
                        r.0,
                        r.1.name(),
                        r.2,
                        r.3,
                        r.4,
                        r.5,
                        r.6
                    );
                    results.lock().unwrap().push(r);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let res = results.lock().unwrap();
    let (mut w_, mut d_, mut l_, mut pa, mut pb) = (0, 0, 0, 0, 0);
    for r in res.iter() {
        match r.3 {
            Outcome::Win(c, _) if c == r.2 => w_ += 1,
            Outcome::Win(..) => l_ += 1,
            _ => d_ += 1,
        }
        pa += r.5;
        pb += r.6;
    }
    let n = res.len() as f64;
    let s = (w_ as f64 + d_ as f64 / 2.0) / n;
    let se = (s * (1.0 - s) / n).sqrt().max(0.5 / n);
    println!(
        "ours {ours_spec} vs external d{ext_depth} {ext_ms}ms: +{w_} ={d_} -{l_}  score {:.1}%  Elo {:+.0} (95%: {:+.0} ~ {:+.0})  pushed {pa}:{pb}",
        s * 100.0,
        elo(s, n),
        elo(s - 1.96 * se, n),
        elo(s + 1.96 * se, n)
    );
}
