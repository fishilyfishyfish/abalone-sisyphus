use abalone_engine::board::*;
use abalone_engine::game::{EndReason, Game, GameConfig, Outcome};
use abalone_engine::layouts::Layout;
use abalone_engine::notation::{move_to_string, parse_move};
use abalone_engine::record::{self, RecordError};
use abalone_engine::rng::Rng;

const LAYOUTS: [Layout; 3] = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy];

/// 무작위 대국. 밀어내기 수가 있으면 자주 골라서 6개 밀어내기로 끝나는 판도 나오게 한다.
fn random_game(rng: &mut Rng, layout: Layout, move_limit: u32, max_plies: usize) -> Game {
    let mut g = Game::new(GameConfig {
        layout,
        move_limit,
        ..GameConfig::default()
    });
    while !g.is_over() && g.ply() < max_plies {
        let ms = g.legal_moves();
        let offs: Vec<Move> = ms
            .iter()
            .copied()
            .filter(|&m| g.pos.is_push_off(m))
            .collect();
        let m = if !offs.is_empty() && rng.below(10) < 8 {
            offs[rng.below(offs.len())]
        } else {
            ms[rng.below(ms.len())]
        };
        g.play_unchecked(m);
    }
    g
}

fn same_game(a: &Game, b: &Game) {
    assert_eq!(a.config, b.config);
    assert_eq!(a.history.len(), b.history.len());
    for (x, y) in a.history.iter().zip(&b.history) {
        assert_eq!(x.0, y.0);
    }
    assert_eq!(a.pos, b.pos);
    assert_eq!(a.outcome(), b.outcome());
}

#[test]
fn export_import_round_trip() {
    let mut rng = Rng::new(61);
    let mut ends = std::collections::BTreeMap::new();
    for i in 0..300 {
        let layout = LAYOUTS[i % 3];
        let limit = [0, 40, 200][i % 3 * 7 % 3];
        let g = random_game(&mut rng, layout, limit, 250);
        let text = record::export(
            &g,
            &[
                ("Black", "사람 \"꼬마\""),
                ("White", "컴퓨터(쉬움)"),
                ("Date", ""),
            ],
        );
        let rec = record::import(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
        same_game(&g, &rec.game);
        assert_eq!(rec.converted, 0);
        // 따옴표는 공백으로 바뀌고, 빈 태그는 빠진다
        assert!(rec
            .tags
            .iter()
            .any(|(k, v)| k == "Black" && v == "사람  꼬마"));
        assert!(!rec.tags.iter().any(|(k, _)| k == "Date"));
        let result = &rec.tags.iter().find(|(k, _)| k == "Result").unwrap().1;
        assert_eq!(result, record::result_token(g.outcome()));
        let key = match g.outcome() {
            Outcome::Ongoing => "ongoing",
            Outcome::Win(_, r) | Outcome::Draw(r) => r.name(),
        };
        *ends.entry(key).or_insert(0) += 1;
    }
    eprintln!("round trip ends: {ends:?}");
    assert!(ends.get("pushed_off").copied().unwrap_or(0) > 10);
    assert!(ends.get("move_limit").copied().unwrap_or(0) > 10);
}

#[test]
fn variant_spellings_never_mean_another_move() {
    let mut rng = Rng::new(62);
    let (mut checked, mut ambiguous) = (0, 0);
    for i in 0..400 {
        let plies = rng.below(80);
        let g = random_game(&mut rng, LAYOUTS[i % 3], 0, plies);
        if g.is_over() {
            continue;
        }
        let legal = g.legal_moves();
        for &m in &legal {
            for sp in record::variant_spellings(m) {
                let text = format!("[Layout \"{}\"]\n{}", g.config.layout.name(), sp);
                // 앞 수순은 기보로 다시 넣는다
                let prefix: Vec<String> =
                    g.history.iter().map(|(m, _)| move_to_string(*m)).collect();
                let text = text.replace('\n', &format!("\n{} ", prefix.join(" ")));
                match record::import(&text) {
                    Ok(rec) => assert_eq!(rec.game.history.last().unwrap().0, m, "{sp}"),
                    Err(RecordError::Ambiguous { .. }) => ambiguous += 1,
                    Err(e) => panic!("{sp} for {}: {e:?}", move_to_string(m)),
                }
                checked += 1;
            }
        }
    }
    eprintln!("variant spellings checked {checked}, ambiguous {ambiguous}");
    assert!(checked > 10_000);
}

#[test]
fn lenient_parsing() {
    // 번호, 주석, 소문자, 구분자, 결과 표시, 변형 표기(맨 앞 구슬 도착 칸)
    let text = "{ 친구랑 둔 판 }\n[Layout \"Belgian Daisy\"]\n1. a1-b2 i5:h4 ; 첫 수\n2.B1C2 { 여기도\n 주석 } I6xH5 *";
    let rec = record::import(text).unwrap();
    let moves: Vec<String> = rec
        .game
        .history
        .iter()
        .map(|(m, _)| move_to_string(*m))
        .collect();
    assert_eq!(moves, ["A1B2", "I5H4", "B1C2", "I6H5"]);

    // 3개 줄 인라인을 맨 뒤 구슬 + 맨 앞 구슬 도착 칸으로 적은 기보
    let mut g = Game::new(GameConfig {
        layout: Layout::Standard,
        ..GameConfig::default()
    });
    let m = g
        .legal_moves()
        .into_iter()
        .find(|&m| m.n == 3 && m.is_inline())
        .unwrap();
    let (head, rear) = m.head_and_rear();
    let d = DIRS[m.dir as usize];
    let variant = format!(
        "{}{}",
        abalone_engine::notation::cell_name(rear),
        abalone_engine::notation::cell_name(head + d)
    );
    let rec = record::import(&format!("[Layout \"standard\"]\n1. {variant}")).unwrap();
    assert_eq!(rec.game.history[0].0, m);
    assert_eq!(rec.converted, 1);
    assert!(g.play(m));
}

#[test]
fn layout_is_detected_when_tag_is_missing() {
    let mut rng = Rng::new(63);
    for (i, &layout) in LAYOUTS.iter().enumerate() {
        let g = random_game(&mut rng, layout, 200, 30 + i * 5);
        let moves: Vec<String> = g.history.iter().map(|(m, _)| move_to_string(*m)).collect();
        let rec = record::import(&moves.join(" ")).unwrap();
        assert_eq!(rec.game.config.layout, layout);
        assert!(rec
            .tags
            .iter()
            .any(|(k, v)| k == "Layout" && v == layout.name()));
        same_game(&g, &rec.game);
    }
}

#[test]
fn errors_point_at_the_move() {
    let base = "[Layout \"standard\"]\n";
    assert!(parse_move(&Layout::Standard.position(), "C3D3").is_some());
    // 백 차례에 흑 구슬을 움직이려 함
    let e = record::import(&format!("{base}1. C3D3 C4D4")).unwrap_err();
    assert_eq!(e.code(), "illegal");
    assert_eq!(e.at(), Some((1, "C4D4")));
    let e = record::import(&format!("{base}1. C3D3 Q9Z1")).unwrap_err();
    assert_eq!(
        e,
        RecordError::BadToken {
            index: 1,
            token: "Q9Z1".into()
        }
    );
    let e = record::import("[Layout \"hexagon\"]\nA1B2").unwrap_err();
    assert_eq!(e, RecordError::UnknownLayout("hexagon".into()));
    let e = record::import("[Layout \"standard\"]\n[MoveLimit \"many\"]\nC3D3").unwrap_err();
    assert_eq!(e.code(), "bad_move_limit");
    assert_eq!(record::import("  \n").unwrap_err(), RecordError::Empty);

    // 수 제한 2수인데 3수째가 있음
    let e = record::import(&format!("{base}[MoveLimit \"2\"]\n1. C3D3 G7F7 2. D3E3")).unwrap_err();
    assert_eq!(
        e,
        RecordError::AfterEnd {
            index: 2,
            token: "D3E3".into()
        }
    );
    let rec = record::import(&format!("{base}[MoveLimit \"2\"]\n1. C3D3 G7F7")).unwrap();
    assert_eq!(rec.game.outcome(), Outcome::Draw(EndReason::MoveLimit));
}

#[test]
fn missing_move_limit_tag() {
    let mut rng = Rng::new(64);
    let g = random_game(&mut rng, Layout::Standard, 0, 230);
    assert!(g.ply() > 200 || g.is_over());
    let moves: Vec<String> = g.history.iter().map(|(m, _)| move_to_string(*m)).collect();
    let rec = record::import(&format!("[Layout \"standard\"]\n{}", moves.join(" "))).unwrap();
    let expect = if g.ply() > 200 { 0 } else { 200 };
    assert_eq!(rec.game.config.move_limit, expect);
}

#[test]
fn record_result_json_escapes_text() {
    use abalone_engine::json::record_result_json;
    let ok = record::import("[Layout \"standard\"]\n[Black \"꼬마 \\\\ 친구\"]\nC3D3");
    let j = record_result_json(&ok);
    assert!(j.starts_with("{\"ok\":true,"));
    assert!(j.contains("[\"Black\",\"꼬마 \\\\\\\\ 친구\"]"), "{j}");
    let err = record::import("[Layout \"standard\"]\nC3D3 Q\"9Z1");
    let j = record_result_json(&err);
    assert_eq!(
        j,
        "{\"ok\":false,\"error\":\"bad_token\",\"index\":1,\"token\":\"Q\\\"9Z1\",\"value\":null}"
    );
}
