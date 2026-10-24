// 엔진(WASM)을 메인 스레드와 분리해서 돌리는 웹 워커.
// 요청: { id, cmd, ...args } → 응답: { id, ok, state, legal, features?, move?, error? }
import init, { WasmGame } from './pkg/abalone_engine.js';

let game = null;
const ready = init();

function snapshot(extra = {}) {
  return {
    ok: true,
    state: JSON.parse(game.state()),
    legal: JSON.parse(game.legal_moves()),
    features: JSON.parse(game.features()),
    ...extra,
  };
}

const handlers = {
  new({ layout, moveLimit }) {
    if (game) game.free();
    game = new WasmGame(layout, moveLimit);
    return snapshot();
  },
  state() {
    return snapshot();
  },
  play({ text }) {
    const ok = game.play(text);
    return { ...snapshot(), ok };
  },
  undo({ count = 1 }) {
    let undone = 0;
    for (let i = 0; i < count && game.undo(); i++) undone++;
    return snapshot({ undone });
  },
  random() {
    return snapshot({ move: game.random_move(Math.random() * 2 ** 52) });
  },
};

self.onmessage = async (e) => {
  const { id, cmd, ...args } = e.data;
  try {
    await ready;
    if (!game && cmd !== 'new') throw new Error('game not started');
    const res = handlers[cmd](args);
    self.postMessage({ id, ...res });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err && err.message ? err.message : err) });
  }
};
