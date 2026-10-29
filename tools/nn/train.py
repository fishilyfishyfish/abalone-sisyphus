#!/usr/bin/env python3
"""평가 보정 신경망 학습.

평가 = 선형 가중합(v2, 고정) + r(국면). r을 작은 신경망(은닉층 하나, ReLU)으로 배운다.
- 입력: 둘 차례 쪽 구슬 61칸 + 상대 구슬 61칸(0/1) + 특징값 차이 10개(mobility 제외, 배율 맞춤).
- 목표: 그 국면이 나온 대국의 결과(둘 차례 관점 1 / 0.5 / 0). 텍셀 튜닝과 같이
  sigmoid(k × (선형 + r))를 로지스틱 손실로 맞춘다. k는 선형만으로 가장 잘 맞는 값으로 먼저 정한다.
- r의 단위는 평가 점수(밀어낸 구슬 1개 = 1000)다. 출력층을 0으로 시작해 처음에는 선형과 같다.
- 판의 대칭 12가지(회전 6 × 뒤집기 2)로 데이터를 늘린다. 규칙이 대칭이고 입력이 둘 차례 관점이라 값이 같다.
- 검증: 파일 뒤쪽 10%(대국 단위로 이어져 있다)를 떼어 둔다.

사용: python3 -I tools/nn/train.py <out.rs> <csv>... [--hidden 32] [--epochs 6]
csv 한 줄 = 특징값 차이 11개, 둘 차례 쪽 구슬(16진), 상대 구슬(16진), 결과 (tune gen ... board).
"""
import sys
import numpy as np

V2 = np.array([1000, -123, 0, -17, -28, 28, 0, 39, 55, -15, 0], dtype=np.float64)
NF = 11
# 신경망 입력으로 쓰는 특징값 수: 앞의 10개 (mobility는 세는 데 비싸서 뺀다)
NF_IN = 10
R_SCALE = 1000.0

# 정규 칸 순서(행 오름차순, 사선 오름차순)와 패딩 칸 번호
CELLS = [(r, c) for r in range(9) for c in range(9) if abs(r - c) <= 4]
PAD = [(r + 1) * 11 + (c + 1) for r, c in CELLS]
INDEX = {rc: i for i, rc in enumerate(CELLS)}


def symmetries():
    """정규 칸 번호의 순열 12개. (dr, dc) 격자에서 60도 회전 R(dr, dc) = (dc, dc − dr), 뒤집기 (dc, dr)."""
    perms = []
    for flip in (False, True):
        for rot in range(6):
            p = []
            for r, c in CELLS:
                a, b = r - 4, c - 4
                if flip:
                    a, b = b, a
                for _ in range(rot):
                    a, b = b, b - a
                p.append(INDEX[(a + 4, b + 4)])
            perms.append(np.array(p))
    return perms


def load(paths):
    feats, own, opp, ys = [], [], [], []
    for path in paths:
        with open(path) as f:
            for line in f:
                v = line.strip().split(',')
                feats.append([int(x) for x in v[:NF]])
                own.append(int(v[NF], 16))
                opp.append(int(v[NF + 1], 16))
                ys.append(float(v[NF + 2]))
    n = len(ys)
    X = np.zeros((n, 122), dtype=np.uint8)
    for i in range(n):
        o, p = own[i], opp[i]
        for k, s in enumerate(PAD):
            if (o >> s) & 1:
                X[i, k] = 1
            elif (p >> s) & 1:
                X[i, 61 + k] = 1
    return np.array(feats, dtype=np.float64), X, np.array(ys)


def logloss(z, y):
    # z = 로짓. 수치 안정한 로지스틱 손실
    return np.mean(np.logaddexp(0, z) - y * z)


def main():
    args = sys.argv[1:]
    hidden, epochs = 32, 6
    if '--hidden' in args:
        i = args.index('--hidden'); hidden = int(args[i + 1]); del args[i:i + 2]
    if '--epochs' in args:
        i = args.index('--epochs'); epochs = int(args[i + 1]); del args[i:i + 2]
    out, paths = args[0], args[1:]
    F, X, y = load(paths)
    n = len(y)
    lin = F @ V2
    split = int(n * 0.9)
    print(f'rows {n} (train {split}, test {n - split}), hidden {hidden}', flush=True)

    # k: 선형만의 최적 배율 (1차원 뉴턴법)
    k = 1e-3
    for _ in range(30):
        z = k * lin[:split]
        p = 1 / (1 + np.exp(-z))
        g = np.mean((p - y[:split]) * lin[:split])
        h = np.mean(p * (1 - p) * lin[:split] ** 2)
        k -= g / h
    print(f'k = {k:.6g}  linear-only loss train {logloss(k * lin[:split], y[:split]):.5f} '
          f'test {logloss(k * lin[split:], y[split:]):.5f}', flush=True)

    # 특징값 배율: 표준편차로 나눈다 (가중치에 같이 담아 Rust에서는 나누지 않는다)
    fscale = F[:split, :NF_IN].std(axis=0)
    fscale[fscale == 0] = 1
    perms = symmetries()

    rng = np.random.default_rng(1)
    din = 122 + NF_IN
    W1 = rng.normal(0, 1 / np.sqrt(30), (din, hidden))
    b1 = np.zeros(hidden)
    w2 = np.zeros(hidden)  # 처음에는 r = 0 (선형과 같음)
    b2 = 0.0
    params = [W1, b1, w2]
    m = [np.zeros_like(p) for p in params] + [0.0]
    v = [np.zeros_like(p) for p in params] + [0.0]
    lr, beta1, beta2, l2 = 1e-3, 0.9, 0.999, 1e-5
    batch = 1024
    step = 0

    def inputs(Xb, Fb, perm=None):
        if perm is not None:
            Xb = np.concatenate([Xb[:, :61][:, perm], Xb[:, 61:][:, perm]], axis=1)
        return np.concatenate([Xb.astype(np.float64), Fb[:, :NF_IN] / fscale], axis=1)

    def residual(A):
        # 학습은 1000점 단위로 한다(점수 단위 그대로면 기울기가 너무 작다). 내보낼 때 곱해 둔다
        H = np.maximum(A @ W1 + b1, 0)
        return H, R_SCALE * (H @ w2 + b2)

    def test_loss():
        _, r = residual(inputs(X[split:], F[split:]))
        return logloss(k * (lin[split:] + r), y[split:]), np.std(r)

    print(f'epoch 0 test {test_loss()[0]:.5f}', flush=True)
    for ep in range(1, epochs + 1):
        order = rng.permutation(split)
        for bi in range(0, split, batch):
            idx = order[bi:bi + batch]
            perm = perms[rng.integers(12)]
            A = inputs(X[idx], F[idx], perm)
            H, r = residual(A)
            z = k * (lin[idx] + r)
            p = 1 / (1 + np.exp(-z))
            dz = (p - y[idx]) * k * R_SCALE / len(idx)  # d loss / d (H·w2 + b2)
            gw2 = H.T @ dz + l2 * w2
            gb2 = dz.sum()
            dH = np.outer(dz, w2) * (H > 0)
            gW1 = A.T @ dH + l2 * W1
            gb1 = dH.sum(axis=0)
            step += 1
            for j, (p_, g) in enumerate(zip([W1, b1, w2], [gW1, gb1, gw2])):
                m[j] = beta1 * m[j] + (1 - beta1) * g
                v[j] = beta2 * v[j] + (1 - beta2) * g * g
                p_ -= lr * (m[j] / (1 - beta1 ** step)) / (np.sqrt(v[j] / (1 - beta2 ** step)) + 1e-8)
            m[3] = beta1 * m[3] + (1 - beta1) * gb2
            v[3] = beta2 * v[3] + (1 - beta2) * gb2 * gb2
            b2 -= lr * (m[3] / (1 - beta1 ** step)) / (np.sqrt(v[3] / (1 - beta2 ** step)) + 1e-8)
        tl, rs = test_loss()
        print(f'epoch {ep} test {tl:.5f}  residual std {rs:.0f} points', flush=True)
        lr *= 0.6

    # Rust로 내보내기: 특징값 배율은 W1의 해당 행에 접어 넣는다
    W1f = W1.copy()
    W1f[122:] /= fscale[:, None]
    with open(out, 'w') as f:
        f.write('//! 평가 보정 신경망 가중치. tools/nn/train.py가 만든 파일이다(손으로 고치지 않는다).\n')
        f.write(f'//! 학습: {n}국면, 은닉 {hidden}, k = {k:.6g}, 검증 손실 선형 '
                f'{logloss(k * lin[split:], y[split:]):.5f} → 보정 {test_loss()[0]:.5f}\n\n')
        f.write(f'pub const HIDDEN: usize = {hidden};\n')
        f.write(f'pub const INPUTS: usize = {din};\n\n')
        f.write('/// 입력 i(0..61 둘 차례 쪽 구슬, 61..122 상대 구슬, 122..132 특징값 차이 앞 10개, 정규 칸 순서) → 은닉층\n')
        f.write('#[rustfmt::skip]\npub static W1: [[f32; HIDDEN]; INPUTS] = [\n')
        for row in W1f:
            f.write('    [' + ', '.join(f'{x:.6}' for x in row) + '],\n')
        f.write('];\n\n#[rustfmt::skip]\npub static B1: [f32; HIDDEN] = [' + ', '.join(f'{x:.6}' for x in b1) + '];\n')
        f.write('/// 은닉층 → 보정값(평가 점수 단위)\n')
        f.write('#[rustfmt::skip]\npub static W2: [f32; HIDDEN] = [' + ', '.join(f'{x * R_SCALE:.4}' for x in w2) + '];\n')
        f.write(f'pub const B2: f32 = {b2 * R_SCALE:.4};\n')
    print('wrote', out)


if __name__ == '__main__':
    main()
