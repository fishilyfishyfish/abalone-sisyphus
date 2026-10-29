//! 평가 보정 신경망: 평가 = 선형 가중합 + `residual(국면)`.
//!
//! 은닉층 하나(ReLU)짜리 작은 신경망이다. 입력은 둘 차례 쪽 구슬 61칸, 상대 구슬 61칸(0/1)과
//! 특징값 차이 앞 10개(mobility 제외). 가중치는 `tools/nn/train.py`가 `nn_weights.rs`로 만든다.
//! 선형 부분은 특징별 기여도로 정확히 나뉘지만, 이 보정값은 나눌 수 없어 설명 층에서는
//! "그 밖의 형세 판단" 한 덩어리로만 다룬다.

use crate::board::*;
use crate::features::NUM_FEATURES;
use crate::nn_weights::*;

/// 신경망 입력으로 쓰는 특징값 수 (특징값 순서의 앞에서부터).
pub const NN_FEATURES: usize = INPUTS - 122;
const _: () = assert!(NN_FEATURES <= NUM_FEATURES);

/// 둘 차례 관점 보정값(평가 점수 단위). `diff`는 둘 차례 관점 특징값 차이.
pub fn residual(pos: &Position, diff: &[i32; NUM_FEATURES]) -> i32 {
    let mut h = B1;
    let mut add = |row: &[f32; HIDDEN], x: f32| {
        for (hj, w) in h.iter_mut().zip(row.iter()) {
            *hj += w * x;
        }
    };
    for s in Bits(pos.own()) {
        add(&W1[CELL_INDEX[s as usize] as usize], 1.0);
    }
    for s in Bits(pos.opp()) {
        add(&W1[61 + CELL_INDEX[s as usize] as usize], 1.0);
    }
    for (f, &d) in diff.iter().take(NN_FEATURES).enumerate() {
        if d != 0 {
            add(&W1[122 + f], d as f32);
        }
    }
    let out: f32 = B2
        + h.iter()
            .zip(W2.iter())
            .map(|(&x, &w)| x.max(0.0) * w)
            .sum::<f32>();
    out.round() as i32
}
