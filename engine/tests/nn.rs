use abalone_engine::features;
use abalone_engine::game::{Game, GameConfig};
use abalone_engine::layouts::Layout;
use abalone_engine::nn::{self, Net};
use abalone_engine::nn_weights::*;
use abalone_engine::rng::Rng;

#[test]
fn net_loaded_from_text_matches_builtin() {
    // 내장 가중치를 train.py와 같은 텍스트 형식으로 써서 읽으면 같은 보정값이 나와야 한다
    let line = |v: &[f32]| {
        v.iter()
            .map(|x| format!("{x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut text = format!("{HIDDEN} {INPUTS}\n");
    for row in W1.iter() {
        text += &line(row);
        text += "\n";
    }
    text += &format!("{}\n{}\n{}\n", line(&B1), line(&W2), B2);
    let path = std::env::temp_dir().join(format!("abalone_net_{}.txt", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let net = Net::load(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(&path).ok();

    let mut rng = Rng::new(5);
    let mut checked = 0;
    for gi in 0..60 {
        let layout = [Layout::BelgianDaisy, Layout::Standard, Layout::GermanDaisy][gi % 3];
        let mut g = Game::new(GameConfig {
            layout,
            move_limit: 0,
            ..GameConfig::default()
        });
        for _ in 0..rng.below(120) {
            let ms = g.legal_moves();
            if ms.is_empty() {
                break;
            }
            g.play_unchecked(ms[rng.below(ms.len())]);
            let d = features::diff(&g.pos, g.pos.side);
            assert_eq!(net.residual(&g.pos, &d), nn::residual(&g.pos, &d));
            checked += 1;
        }
    }
    assert!(checked > 1000);
}
