use abalone_engine::board::*;
use abalone_engine::features::{self, SideFeatures};
use abalone_engine::game::*;
use abalone_engine::layouts::{mask_of, Layout};
use abalone_engine::notation::{move_to_string, parse_cell, parse_move};

fn pos(black: &[&str], white: &[&str], side: Color) -> Position {
    Position::new(mask_of(black), mask_of(white), side)
}

fn texts(p: &Position) -> Vec<String> {
    p.legal_moves().into_iter().map(move_to_string).collect()
}

fn has_move(p: &Position, t: &str) -> bool {
    texts(p).iter().any(|x| x == t)
}

#[test]
fn board_has_61_cells_and_layouts_have_14_each() {
    assert_eq!(VALID.count_ones(), 61);
    assert_eq!(EDGE.count_ones(), 24);
    for l in [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy] {
        let p = l.position();
        assert_eq!(p.count(Color::Black), 14, "{l:?}");
        assert_eq!(p.count(Color::White), 14, "{l:?}");
        assert_eq!(p.side, Color::Black);
    }
}

#[test]
fn notation_round_trip() {
    assert_eq!(parse_cell("A1"), Some(sq(0, 0)));
    assert_eq!(parse_cell("I9"), Some(sq(8, 8)));
    assert_eq!(parse_cell("A6"), None);
    assert_eq!(parse_cell("I4"), None);
    for &s in CELLS.iter() {
        let n = abalone_engine::notation::cell_name(s as i32);
        assert_eq!(parse_cell(&n), Some(s as i32));
    }
    let p = Layout::BelgianDaisy.position();
    for m in p.legal_moves() {
        assert_eq!(parse_move(&p, &move_to_string(m)), Some(m));
    }
}

#[test]
fn standard_opening_move_count() {
    // 표준 배치 흑의 첫 수는 44가지로 알려져 있다
    assert_eq!(Layout::Standard.position().legal_moves().len(), 44);
}

#[test]
fn two_vs_two_and_three_vs_three_cannot_push() {
    // E 방향: 흑 E3 E4 → 백 E5 E6, 뒤 E7 빈칸
    let p = pos(&["E3", "E4"], &["E5", "E6"], Color::Black);
    assert!(!has_move(&p, "E3E4"), "2:2 push must be illegal");
    let p = pos(&["E2", "E3", "E4"], &["E5", "E6", "E7"], Color::Black);
    assert!(!has_move(&p, "E2E3"), "3:3 push must be illegal");
    // 대조군: 3:2, 2:1은 가능
    let p = pos(&["E2", "E3", "E4"], &["E5", "E6"], Color::Black);
    assert!(has_move(&p, "E2E3"));
    let p = pos(&["E3", "E4"], &["E5"], Color::Black);
    assert!(has_move(&p, "E3E4"));
    // 1:1도 불가
    let p = pos(&["E4"], &["E5"], Color::Black);
    assert!(!has_move(&p, "E4E5"));
}

#[test]
fn push_result_board() {
    let mut p = pos(&["E2", "E3", "E4"], &["E5", "E6"], Color::Black);
    let m = parse_move(&p, "E2E3").unwrap();
    let before = p;
    let u = p.make_move(m);
    assert_eq!(p.stones[0], mask_of(&["E3", "E4", "E5"]));
    assert_eq!(p.stones[1], mask_of(&["E6", "E7"]));
    assert_eq!(p.side, Color::White);
    assert_eq!(p.hash, p.compute_hash());
    p.unmake_move(u);
    assert_eq!(p, before);
}

#[test]
fn cannot_move_four_or_more() {
    // 흑 4개 일렬, 앞은 빈칸
    let p = pos(&["E2", "E3", "E4", "E5"], &["A1"], Color::Black);
    let moves = p.legal_moves();
    assert!(moves.iter().all(|m| m.n <= 3));
    // 맨 뒤 E2는 E 방향으로 움직일 수 없다 (그 줄은 4개)
    assert!(!has_move(&p, "E2E3"));
    // 앞쪽 3개(E3~E5)는 움직일 수 있다
    assert!(has_move(&p, "E3E4"));
    // 4개 줄로 2개를 미는 것도 불가 (3개가 최대이므로 뒤의 4번째는 막는 쪽)
    let p = pos(&["E1", "E2", "E3", "E4"], &["E5", "E6"], Color::Black);
    assert!(!has_move(&p, "E1E2"));
    assert!(has_move(&p, "E2E3")); // E2 E3 E4가 3:2로 민다
}

#[test]
fn cannot_push_when_own_marble_behind_opponent() {
    // 흑 E3 E4 → 백 E5, 그 뒤 E6에 흑
    let p = pos(&["E3", "E4", "E6"], &["E5"], Color::Black);
    assert!(!has_move(&p, "E3E4"));
    // 3:2 상황에서 상대 줄 뒤 흑
    let p = pos(&["E1", "E2", "E3", "E6"], &["E4", "E5"], Color::Black);
    assert!(!has_move(&p, "E1E2"));
    // 상대 줄 뒤가 상대 구슬이면 상대 줄이 길어진 것: 3:3 불가
    let p = pos(&["E1", "E2", "E3"], &["E4", "E5", "E6"], Color::Black);
    assert!(!has_move(&p, "E1E2"));
}

#[test]
fn push_off_board() {
    // 흑 E7 E8 → 백 E9, 그 뒤는 판 밖
    let mut g = Game::from_position(
        pos(&["E7", "E8", "A1"], &["E9", "I9"], Color::Black),
        GameConfig::default(),
    );
    let before = g.pos.pushed_off_by(Color::Black);
    let m = parse_move(&g.pos, "E7E8").unwrap();
    assert!(g.pos.is_push_off(m));
    assert!(g.play(m));
    assert_eq!(g.pos.count(Color::White), 1);
    assert_eq!(g.pos.pushed_off_by(Color::Black), before + 1);
    assert_eq!(g.pos.stones[0], mask_of(&["E8", "E9", "A1"]));
    g.undo();
    assert_eq!(g.pos.count(Color::White), 2);

    // 3:2로 두 개 중 하나만 밀려 나간다
    let mut p = pos(&["E5", "E6", "E7"], &["E8", "E9"], Color::Black);
    let m = parse_move(&p, "E5E6").unwrap();
    assert!(p.is_push_off(m));
    p.make_move(m);
    assert_eq!(p.stones[1], mask_of(&["E9"]));

    // 내 구슬을 스스로 판 밖으로 내보낼 수는 없다
    let p = pos(&["E9"], &["A1"], Color::Black);
    let mut t = texts(&p);
    t.sort();
    assert_eq!(t, vec!["E9D8", "E9E8", "E9F9"]);
}

#[test]
fn six_pushed_off_wins() {
    // 백 9개 남음(흑이 5개 밀어낸 상태), 한 개 더 밀면 승리
    let white = ["E9", "I5", "I6", "I7", "I8", "H4", "H5", "H6", "H7"];
    let mut g = Game::from_position(
        pos(&["E7", "E8"], &white, Color::Black).with_pushed(5, 0),
        GameConfig::default(),
    );
    assert_eq!(g.pos.pushed_off_by(Color::Black), 5);
    assert!(!g.is_over());
    let m = parse_move(&g.pos, "E7E8").unwrap();
    assert!(g.play(m));
    assert_eq!(
        g.outcome(),
        Outcome::Win(Color::Black, EndReason::PushedOff)
    );
    assert!(g.legal_moves().is_empty());
    g.undo();
    assert_eq!(g.outcome(), Outcome::Ongoing);
}

#[test]
fn broadside_blocked() {
    // 흑 E4 E5 가로 줄, NE(+1,+1) 방향으로 옆걸음 → F5 F6
    let p = pos(&["E4", "E5"], &["A1"], Color::Black);
    assert!(has_move(&p, "E4E5F5"));
    // 도착 칸 하나에 백이 있으면 막힘 (브로드사이드는 밀 수 없다)
    let p = pos(&["E4", "E5"], &["F6"], Color::Black);
    assert!(!has_move(&p, "E4E5F5"));
    // 도착 칸 하나에 흑이 있어도 막힘
    let p = pos(&["E4", "E5", "F5"], &["A1"], Color::Black);
    assert!(!has_move(&p, "E4E5F5"));
    // 판 밖으로의 옆걸음도 불가: I행 줄을 위로
    let p = pos(&["I6", "I7"], &["A1"], Color::Black);
    let side: Vec<String> = texts(&p)
        .into_iter()
        .filter(|t| t.len() == 6 && t.starts_with("I6I7"))
        .collect();
    assert_eq!(side.len(), 2);
    assert!(side.iter().all(|t| &t[4..5] == "H"));
    // 3개 브로드사이드
    let p = pos(&["E3", "E4", "E5"], &["A1"], Color::Black);
    assert!(has_move(&p, "E3E5D3"));
    let p = pos(&["E3", "E4", "E5"], &["D4"], Color::Black);
    assert!(!has_move(&p, "E3E5D3"));
}

#[test]
fn move_limit_adjudication() {
    let cfg = GameConfig {
        move_limit: 2,
        ..GameConfig::default()
    };
    // 같은 밀어낸 수 → 무승부
    let mut g = Game::new(cfg);
    for _ in 0..2 {
        let m = g.legal_moves()[0];
        g.play(m);
    }
    assert_eq!(g.outcome(), Outcome::Draw(EndReason::MoveLimit));
    assert!(g.legal_moves().is_empty());
    // 흑이 하나 더 밀어낸 상태 → 흑 승
    let white = [
        "I5", "I6", "I7", "I8", "I9", "H4", "H5", "H6", "H7", "H8", "H9", "G5", "G6",
    ];
    let black = [
        "A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3", "B4", "B5", "B6", "C3", "C4", "C5",
    ];
    let mut g = Game::from_position(pos(&black, &white, Color::Black).with_pushed(1, 0), cfg);
    g.play(g.legal_moves()[0]);
    assert_eq!(g.outcome(), Outcome::Ongoing);
    g.play(g.legal_moves()[0]);
    assert_eq!(
        g.outcome(),
        Outcome::Win(Color::Black, EndReason::MoveLimit)
    );
    // 무르면 다시 진행 중
    g.undo();
    assert_eq!(g.outcome(), Outcome::Ongoing);
    // 기본값은 200수
    assert_eq!(GameConfig::default().move_limit, 200);
}

#[test]
fn repetition_ends_game() {
    let mut g = Game::from_position(
        pos(&["A1"], &["I9"], Color::Black),
        GameConfig {
            move_limit: 0,
            ..GameConfig::default()
        },
    );
    let seq = ["A1A2", "I9I8", "A2A1", "I8I9"];
    let mut plies = 0;
    'outer: for _ in 0..3 {
        for t in seq {
            let m = parse_move(&g.pos, t).unwrap();
            assert!(g.play(m));
            plies += 1;
            if g.is_over() {
                break 'outer;
            }
        }
    }
    // 시작 국면이 4수, 8수 뒤에 다시 나와 3회째
    assert_eq!(plies, 8);
    assert_eq!(g.outcome(), Outcome::Draw(EndReason::Repetition));
}

#[test]
fn features_basic() {
    // 흑: E5(중앙), E6, A1(고립, 가장자리) / 백: E7 → 흑 E5 E6이 E 방향으로 2:1 밀기 가능하나 E8 빈칸이라 밀어내기는 아님
    let p = pos(&["E5", "E6", "A1"], &["E8", "E9"], Color::Black);
    let f = features::compute(&p);
    assert_eq!(
        f.black,
        SideFeatures {
            pushed_off: 0,
            center_distance: 1 + 4, // E5=0, E6=1, A1=4
            cohesion: 1,
            edge: 1,
            endangered: 0,
            attackable: 0,
            isolated: 1,
            push_moves: 0,
            triples: 0,
            groups: 2,
            mobility: f.black.mobility,
        }
    );
    assert_eq!(f.white.cohesion, 1);
    assert_eq!(f.white.edge, 1);
    assert_eq!(f.white.center_distance, 3 + 4);
    assert_eq!(f.white.isolated, 0);

    // 밀려날 위험: 백 E7 E8이 흑 E9를 밀어낼 수 있다 (흑 차례여도 "상대가 둔다면"으로 계산)
    let p = pos(&["E9", "A1"], &["E7", "E8"], Color::Black);
    let f = features::compute(&p);
    assert_eq!(f.black.endangered, 1);
    assert_eq!(f.white.attackable, 1);
    assert_eq!(f.white.endangered, 0);

    // 같은 구슬을 여러 방법으로 밀 수 있어도 한 번만 센다, 서로 다른 구슬은 따로 센다
    // 백 3개 E6 E7 E8 → 흑 E9 (3:1), 백 E7 E8도 2:1. 흑 I9는 백 G7 H8로 밀린다 (NE 방향)
    let p = pos(&["E9", "I9"], &["E6", "E7", "E8", "G7", "H8"], Color::White);
    let f = features::compute(&p);
    assert_eq!(f.black.endangered, 2);
    assert_eq!(f.white.attackable, 2);
}

#[test]
fn pushed_off_marbles_count_as_off_board_distance() {
    // 백이 흑 구슬 2개를 밀어낸 상태: 흑 거리 합 = 판 위 구슬 + 5 × 2
    let p = pos(&["E5", "A1"], &["E8"], Color::Black).with_pushed(0, 2);
    assert_eq!(features::compute(&p).black.center_distance, 4 + 5 * 2);
    // 가장자리 구슬 하나를 밀려나면 거리 합은 4 → 5로 늘어난다
    let mut g = Game::from_position(
        pos(&["E9", "A1"], &["E7", "E8", "I5"], Color::White),
        GameConfig::default(),
    );
    let before = features::compute(&g.pos).black.center_distance;
    let m = parse_move(&g.pos, "E7E8").unwrap();
    assert!(g.play(m));
    assert_eq!(features::compute(&g.pos).black.center_distance, before + 1);
}

#[test]
fn new_shape_features() {
    // 흑: E2 E3 E4 E5 (4개 일렬 → 3개 줄 2곳), A1 따로 → 덩어리 2. 백: E6, I9
    let p = pos(&["E2", "E3", "E4", "E5", "A1"], &["E6", "I9"], Color::Black);
    let f = features::compute(&p);
    assert_eq!(f.black.triples, 2);
    assert_eq!(f.black.groups, 2);
    // 흑이 백 E6을 미는 수: E3E4(3:1 → E3 E4 E5가 E6을 E7로), E4E5(2:1)
    assert_eq!(f.black.push_moves, 2);
    assert_eq!(f.white.push_moves, 0);
    assert_eq!(f.white.groups, 2);
    assert_eq!(f.white.triples, 0);
    // 합법수 개수는 수 생성 결과와 같다
    assert_eq!(f.black.mobility as usize, p.legal_moves().len());
    let mut pw = p;
    pw.side = Color::White;
    assert_eq!(f.white.mobility as usize, pw.legal_moves().len());
}

#[test]
fn layout_feature_symmetry() {
    // 세 시작 배치는 흑백 대칭이라 특징값이 같아야 한다
    for l in [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy] {
        let f = features::compute(&l.position());
        assert_eq!(f.black, f.white, "{l:?}");
        assert_eq!(f.black.pushed_off, 0);
        assert_eq!(f.black.endangered, 0);
    }
}
