// 엔진(WASM)을 메인 스레드와 분리해서 돌리는 웹 워커.
// 요청: { id, cmd, ...args } → 응답: { id, ok, state, legal, features, singles, evaluation, move?, search?, options?, analysis?, hint?, text?, result?, error? }
import init, { WasmGame } from './pkg/abalone_engine.js';

let game = null;
const ready = init();

function snapshot(extra = {}) {
  return {
    ok: true,
    state: JSON.parse(game.state()),
    legal: JSON.parse(game.legal_moves()),
    features: JSON.parse(game.features()),
    singles: JSON.parse(game.single_options()),
    evaluation: JSON.parse(game.evaluate()),
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
  group({ cells }) {
    return { ok: true, options: JSON.parse(game.group_options(Uint8Array.from(cells))) };
  },
  stateAt({ ply }) {
    return { ok: true, state: JSON.parse(game.state_at(ply)) };
  },
  analyzePly({ ply, ms }) {
    return { ok: true, analysis: JSON.parse(game.analyze_ply(ply, ms)) };
  },
  analyzeCandidate({ text, ms }) {
    return { ok: true, analysis: JSON.parse(game.analyze_candidate(text, ms)) };
  },
  exportRecord({ black, white, date }) {
    return { ok: true, text: game.export_record(black, white, date) };
  },
  loadRecord({ text }) {
    const result = JSON.parse(game.load_record(text));
    return { ...snapshot(), result };
  },
  hint({ ms }) {
    return { ok: true, hint: JSON.parse(game.hint(ms)) };
  },
  ponder({ ms }) {
    return { ok: true, ponder: JSON.parse(game.ponder(ms)) };
  },
  ai({ level }) {
    const r = JSON.parse(game.ai_move(level, Math.random() * 2 ** 52));
    return { ok: true, move: r.move, search: r.search };
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
