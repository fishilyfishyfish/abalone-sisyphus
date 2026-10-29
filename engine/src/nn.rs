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

/// 파일에서 읽은 신경망 (실험용: 학습한 가중치를 다시 빌드하지 않고 대국으로 비교할 때).
/// 파일 형식은 `tools/nn/train.py`가 `.rs`와 같이 쓰는 `.txt`: 첫 줄 "은닉 입력",
/// 다음 입력 수만큼 W1 행, 그다음 B1, W2, B2 한 줄씩(공백 구분).
#[derive(Clone, Debug)]
pub struct Net {
    hidden: usize,
    inputs: usize,
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: f32,
}

impl Net {
    pub fn load(path: &str) -> Result<Net, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut lines = text.lines();
        let nums = |l: Option<&str>| -> Result<Vec<f32>, String> {
            l.ok_or("unexpected end")?
                .split_whitespace()
                .map(|x| x.parse::<f32>().map_err(|e| e.to_string()))
                .collect()
        };
        let head = nums(lines.next())?;
        let (hidden, inputs) = (head[0] as usize, head[1] as usize);
        if inputs != INPUTS {
            return Err(format!("inputs {inputs} != {INPUTS}"));
        }
        let mut w1 = Vec::with_capacity(inputs * hidden);
        for _ in 0..inputs {
            w1.extend(nums(lines.next())?);
        }
        let b1 = nums(lines.next())?;
        let w2 = nums(lines.next())?;
        let b2 = nums(lines.next())?[0];
        if w1.len() != inputs * hidden || b1.len() != hidden || w2.len() != hidden {
            return Err("size mismatch".into());
        }
        Ok(Net {
            hidden,
            inputs,
            w1,
            b1,
            w2,
            b2,
        })
    }

    /// `residual`과 같은 계산을 이 가중치로 한다.
    pub fn residual(&self, pos: &Position, diff: &[i32; NUM_FEATURES]) -> i32 {
        let h_n = self.hidden;
        let mut h = self.b1.clone();
        let mut add = |row: usize, x: f32| {
            let r = &self.w1[row * h_n..(row + 1) * h_n];
            for (hj, w) in h.iter_mut().zip(r) {
                *hj += w * x;
            }
        };
        for s in Bits(pos.own()) {
            add(CELL_INDEX[s as usize] as usize, 1.0);
        }
        for s in Bits(pos.opp()) {
            add(61 + CELL_INDEX[s as usize] as usize, 1.0);
        }
        for (f, &d) in diff.iter().take(self.inputs - 122).enumerate() {
            if d != 0 {
                add(122 + f, d as f32);
            }
        }
        let out: f32 = self.b2
            + h.iter()
                .zip(self.w2.iter())
                .map(|(&x, &w)| x.max(0.0) * w)
                .sum::<f32>();
        out.round() as i32
    }
}
