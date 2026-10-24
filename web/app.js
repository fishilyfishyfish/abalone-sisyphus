// 아발론 웹 UI. 규칙 판단(둘 수 있는 수, 못 두는 이유)은 전부 워커 안의 엔진이 하고,
// 여기서는 그리기와 입력만 다룬다.

const SVG_NS = 'http://www.w3.org/2000/svg';
const S = 100; // 칸 간격
const H = S * Math.sqrt(3) / 2;
const R_MARBLE = 42;
// 엔진과 같은 방향 순서: E, NE, NW, W, SW, SE  (dr, dc)
const DIRS = [[0, 1], [1, 1], [1, 0], [0, -1], [-1, -1], [-1, 0]];
const ROWS = 'ABCDEFGHI';
const AI_DELAY_MS = 600;
const MOVE_MS = 280;

// 정규 칸 순서 (행 오름차순, 사선 오름차순) — 엔진의 CELLS와 같다
const CELLS = [];
const CELL_AT = new Map();
for (let r = 0; r < 9; r++) {
  for (let c = 0; c < 9; c++) {
    if (Math.abs(r - c) <= 4) {
      CELL_AT.set(r * 9 + c, CELLS.length);
      CELLS.push({ r, c, name: ROWS[r] + (c + 1) });
    }
  }
}
const cellIndex = (r, c) => (Math.abs(r - c) <= 4 && r >= 0 && r < 9 && c >= 0 && c < 9 ? CELL_AT.get(r * 9 + c) : undefined);
const px = (r, c) => [(c - r / 2) * S, (8 - r) * H];
const dirVec = (d) => {
  const [dr, dc] = DIRS[d];
  const x = (dc - dr / 2) * S, y = -dr * H;
  const l = Math.hypot(x, y);
  return [x / l, y / l];
};
const step = (i, d) => cellIndex(CELLS[i].r + DIRS[d][0], CELLS[i].c + DIRS[d][1]);

// ---------------------------------------------------------------- 워커

const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
let reqId = 0;
const pending = new Map();
worker.onmessage = (e) => {
  const { id, ...res } = e.data;
  const p = pending.get(id);
  if (!p) return;
  pending.delete(id);
  if (res.error) p.reject(new Error(res.error));
  else p.resolve(res);
};
worker.onerror = (e) => say(`엔진을 불러오지 못했어요: ${e.message || e}`, 'warn');
function call(cmd, args = {}) {
  const id = ++reqId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, cmd, ...args });
  });
}

// ---------------------------------------------------------------- 상태

const ui = {
  state: null,
  features: null,
  evaluation: null,
  lastSearch: null, // { color, ply, search }
  singles: {}, // 정규 칸 번호 → 방향별 선택지. 움직일 수 있는 구슬을 빛내는 데만 쓴다
  selection: [], // 정규 칸 번호
  options: null, // 지금 선택에 대한 방향별 선택지
  human: 'black',
  busy: false,
  aiToken: 0,
  gesture: null,
  dragging: false,
  wasOnlyThis: false,
};

const $ = (id) => document.getElementById(id);
const board = $('board');
const KO = { black: '흑', white: '백' };
const ownChar = () => (ui.state.side === 'black' ? 'b' : 'w');

const isHumanTurn = () =>
  ui.state && ui.state.outcome.status === 'ongoing' && (ui.human === 'both' || ui.state.side === ui.human);

function setState(res) {
  ui.state = res.state;
  ui.features = res.features;
  ui.evaluation = res.evaluation;
  ui.singles = res.singles || {};
  ui.selection = [];
  ui.options = null;
}

// ---------------------------------------------------------------- 판 그리기 (고정 요소)

const layers = {};
let marbleEls = new Map(); // 정규 칸 번호 → circle
function el(tag, attrs = {}, parent) {
  const e = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (parent) parent.appendChild(e);
  return e;
}

function buildBoard() {
  const cx = 2 * S, cy = 4 * H;
  const corners = [[0, 0], [0, 4], [4, 8], [8, 8], [8, 4], [4, 0]].map(([r, c]) => px(r, c));
  const k = 4.95 / 4;
  const poly = corners.map(([x, y]) => `${cx + (x - cx) * k},${cy + (y - cy) * k}`).join(' ');
  const pad = 60;
  const minX = cx - 4 * S * k - pad, maxX = cx + 4 * S * k + pad;
  const minY = cy - 4 * H * k - pad + 20, maxY = cy + 4 * H * k + pad + 30;
  board.setAttribute('viewBox', `${minX} ${minY} ${maxX - minX} ${maxY - minY}`);

  const defs = el('defs', {}, board);
  const gb = el('radialGradient', { id: 'g-black', cx: '35%', cy: '30%', r: '70%' }, defs);
  el('stop', { offset: '0%', 'stop-color': 'var(--black-a)' }, gb);
  el('stop', { offset: '100%', 'stop-color': 'var(--black-b)' }, gb);
  const gw = el('radialGradient', { id: 'g-white', cx: '35%', cy: '30%', r: '70%' }, defs);
  el('stop', { offset: '0%', 'stop-color': 'var(--white-a)' }, gw);
  el('stop', { offset: '100%', 'stop-color': 'var(--white-b)' }, gw);

  el('polygon', { points: poly, class: 'board-bg' }, board);
  const holes = el('g', {}, board);
  for (const { r, c } of CELLS) {
    const [x, y] = px(r, c);
    el('circle', { cx: x, cy: y, r: R_MARBLE * 0.82, class: 'hole' }, holes);
  }
  // 좌표 표시: 행 글자는 각 행 왼쪽, 사선 숫자는 아래·오른쪽 아래 가장자리 바깥
  const labels = el('g', {}, board);
  for (let r = 0; r < 9; r++) {
    const [x, y] = px(r, Math.max(0, r - 4) - 1.32);
    el('text', { x, y, class: 'label' }, labels).textContent = ROWS[r];
  }
  for (let c = 0; c < 9; c++) {
    const [x, y] = px(Math.max(0, c - 4) - 1.3, c);
    el('text', { x, y, class: 'label' }, labels).textContent = String(c + 1);
  }

  layers.marks = el('g', {}, board);
  layers.glow = el('g', {}, board);
  layers.marbles = el('g', {}, board);
  layers.sel = el('g', {}, board);
  layers.hits = el('g', {}, board);
  layers.arrows = el('g', {}, board);

  CELLS.forEach(({ r, c, name }, i) => {
    const [x, y] = px(r, c);
    const hit = el('circle', { cx: x, cy: y, r: S / 2, class: 'hit', 'data-i': i }, layers.hits);
    hit.setAttribute('aria-label', name);
  });

  // 내 구슬에서 시작해 쓸면 지나간 줄(최대 3개)을 고른다. 쓸지 않고 떼면 탭.
  layers.hits.addEventListener('pointerdown', (e) => {
    const i = e.target.getAttribute('data-i');
    if (i === null) return;
    const start = Number(i);
    const own = isHumanTurn() && !ui.busy && ui.state.board[start] === ownChar();
    ui.gesture = { start, id: e.pointerId, own, cells: [start], moved: false };
    if (own) {
      ui.dragging = true;
      ui.wasOnlyThis = ui.selection.length === 1 && ui.selection[0] === start;
      showSelection([start]);
    }
  });
  window.addEventListener('pointermove', (e) => {
    const g = ui.gesture;
    if (!g || !g.own || e.pointerId !== g.id) return;
    const cell = cellAt(svgPoint(e));
    if (cell === undefined) return;
    if (cell !== g.start) g.moved = true;
    const seg = segment(g.start, cell);
    if (!seg || !seg.every((c) => ui.state.board[c] === ownChar())) return; // 줄이 아니면 직전 선택 유지
    if (seg.join(',') === g.cells.join(',')) return;
    g.cells = seg;
    showSelection(seg);
  });
  const end = (e) => {
    const g = ui.gesture;
    if (!g || e.pointerId !== g.id) return;
    ui.gesture = null;
    ui.dragging = false;
    if (e.type !== 'pointerup') {
      if (g.own) select([]);
      return;
    }
    if (!g.own) {
      onCellTap(g.start);
    } else if (!g.moved && g.cells.length === 1 && ui.wasOnlyThis) {
      select([]); // 고른 구슬을 다시 누르면 해제
    } else {
      select(g.cells);
    }
  };
  window.addEventListener('pointerup', end);
  window.addEventListener('pointercancel', end);
}

/** SVG 좌표에서 가장 가까운 칸 (반 칸 넘게 떨어지면 없음) */
function cellAt(p) {
  let best, bestD = S * 0.55;
  CELLS.forEach(({ r, c }, i) => {
    const [x, y] = px(r, c);
    const d = Math.hypot(p.x - x, p.y - y);
    if (d < bestD) [best, bestD] = [i, d];
  });
  return best;
}

/** 쓰는 중: 고른 줄만 보여주고 화살표는 손을 뗀 뒤에 */
function showSelection(cells) {
  ui.selection = cells;
  ui.options = null;
  renderSelection();
  say(cells.length === 1 ? '그대로 떼면 이 구슬 하나, 옆 구슬로 쓸면 같이 골라져요.' : `${cells.length}개 골랐어요. 손을 떼면 화살표가 나와요.`);
}

function svgPoint(e) {
  const pt = board.createSVGPoint();
  pt.x = e.clientX;
  pt.y = e.clientY;
  return pt.matrixTransform(board.getScreenCTM().inverse());
}

// ---------------------------------------------------------------- 렌더

function render() {
  const st = ui.state;
  if (!st) return;
  renderMarbles(st.board);
  renderLast(st);
  renderSelection();
  renderPanel(st);
}

function renderMarbles(boardStr) {
  layers.marbles.replaceChildren();
  marbleEls = new Map();
  for (let i = 0; i < CELLS.length; i++) {
    const v = boardStr[i];
    if (v === '.') continue;
    const [x, y] = px(CELLS[i].r, CELLS[i].c);
    const m = el('circle', { cx: x, cy: y, r: R_MARBLE, class: `marble ${v === 'b' ? 'black' : 'white'}` }, layers.marbles);
    marbleEls.set(i, m);
  }
}

function renderLast(st) {
  layers.marks.replaceChildren();
  if (!st.last) return;
  for (const i of st.last.cells) {
    const t = step(i, st.last.dir);
    if (st.board[i] === '.') {
      const [x, y] = px(CELLS[i].r, CELLS[i].c);
      el('circle', { cx: x, cy: y, r: R_MARBLE * 0.55, class: 'last-from' }, layers.marks);
    }
    if (t !== undefined) {
      const [tx, ty] = px(CELLS[t].r, CELLS[t].c);
      el('circle', { cx: tx, cy: ty, r: R_MARBLE + 3, class: 'last-to' }, layers.marks);
    }
  }
}

/** 수를 두면 내 구슬이 새로 차지하는 칸 */
function destinations(m) {
  return m.cells.map((i) => step(i, m.dir)).filter((i) => i !== undefined && !m.cells.includes(i));
}

/** 인라인 수에서 밀리는 상대 구슬 칸 */
function pushedCells(m, boardStr = ui.state.board) {
  if (!m.inline) return [];
  const opp = ownChar() === 'b' ? 'w' : 'b';
  const head = m.cells.find((i) => !m.cells.includes(step(i, m.dir)));
  const out = [];
  for (let t = step(head, m.dir); t !== undefined && boardStr[t] === opp; t = step(t, m.dir)) out.push(t);
  return out;
}

function renderSelection() {
  layers.sel.replaceChildren();
  layers.arrows.replaceChildren();
  layers.glow.replaceChildren();
  const myTurn = isHumanTurn();

  if (!ui.selection.length) {
    if (myTurn && !ui.dragging) {
      // 움직일 수 있는 내 구슬을 은은하게 표시
      for (const [k, opts] of Object.entries(ui.singles)) {
        if (!opts.some((o) => o.move)) continue;
        const { r, c } = CELLS[Number(k)];
        const [x, y] = px(r, c);
        el('circle', { cx: x, cy: y, r: R_MARBLE + 5, class: 'glow' }, layers.glow);
      }
    }
    return;
  }

  const pts = ui.selection.map((i) => px(CELLS[i].r, CELLS[i].c));
  for (const [x, y] of pts) el('circle', { cx: x, cy: y, r: R_MARBLE + 4, class: 'sel-ring' }, layers.sel);
  const opts = ui.options;
  if (!opts) return;

  for (const o of opts) {
    const [ux, uy] = dirVec(o.dir);
    const m = o.move;
    const [ax, ay] = arrowAnchor(o, pts);
    const angle = (Math.atan2(uy, ux) * 180) / Math.PI;
    const kind = !m ? 'blocked' : m.push_off ? 'off' : pushedCells(m).length ? 'push' : 'go';
    const g = el('g', { class: `arrow ${kind}`, transform: `translate(${ax} ${ay})`, role: 'button', tabindex: 0 }, layers.arrows);
    g.setAttribute('aria-label', m ? `${m.text}${m.push_off ? ' 밀어내기' : ''}` : '이 방향은 못 가요');
    if (m) {
      el('circle', { r: 38 }, g);
      el('path', { d: 'M -14 -16 L 19 0 L -14 16 Z', transform: `rotate(${angle})` }, g);
    } else {
      el('circle', { r: 18 }, g);
      el('path', { d: 'M -6 -7 L 9 0 L -6 7 Z', transform: `rotate(${angle})` }, g);
    }
    // 화살표를 누르고 있으면 같이 움직일 구슬을 미리 보여준다
    if (m) {
      g.addEventListener('pointerenter', () => preview(m));
      g.addEventListener('pointerleave', () => preview(null));
    }
    const go = (e) => {
      e.stopPropagation();
      choose(o);
    };
    g.addEventListener('click', go);
    g.addEventListener('keydown', (e) => (e.key === 'Enter' || e.key === ' ') && go(e));
  }
}

/**
 * 화살표 위치. 큰 화살표: 인라인은 줄 맨 앞 구슬의 다음 칸(판 밖이면 바로 바깥), 옆걸음은 도착 칸 가운데.
 * 회색 화살표: 고른 구슬 바로 옆 틈(반 칸). 어느 쪽도 이웃한 내 구슬의 탭을 가로채지 않는다.
 */
function arrowAnchor(o, pts) {
  const [ux, uy] = dirVec(o.dir);
  const cx = pts.reduce((a, p) => a + p[0], 0) / pts.length;
  const cy = pts.reduce((a, p) => a + p[1], 0) / pts.length;
  if (!o.move) {
    const sel = ui.selection;
    const along = sel.length === 1 || sel.includes(step(sel[0], o.dir)) || sel.includes(step(sel[0], (o.dir + 3) % 6));
    let [bx, by] = [cx, cy];
    if (along) {
      const head = sel.find((i) => !sel.includes(step(i, o.dir)));
      [bx, by] = px(CELLS[head].r, CELLS[head].c);
    }
    return [bx + ux * S * 0.5, by + uy * S * 0.5];
  }
  const cells = o.move.cells;
  if (o.move.inline) {
    const head = cells.find((i) => !cells.includes(step(i, o.dir)));
    const t = step(head, o.dir);
    if (t !== undefined) return px(CELLS[t].r, CELLS[t].c);
    const [hx, hy] = px(CELLS[head].r, CELLS[head].c);
    return [hx + ux * S * 0.72, hy + uy * S * 0.72];
  }
  return [cx + ux * S, cy + uy * S];
}

function preview(m) {
  layers.sel.querySelectorAll('.preview').forEach((e) => e.remove());
  if (!m) return;
  for (const i of m.cells) {
    if (ui.selection.includes(i)) continue;
    const [x, y] = px(CELLS[i].r, CELLS[i].c);
    el('circle', { cx: x, cy: y, r: R_MARBLE + 4, class: 'sel-ring preview' }, layers.sel);
  }
  for (const t of destinations(m)) {
    const [x, y] = px(CELLS[t].r, CELLS[t].c);
    el('circle', { cx: x, cy: y, r: R_MARBLE * 0.5, class: 'target preview' }, layers.sel);
  }
}

const REASON = {
  pushed_off: '상대 구슬 6개를 밀어냈어요',
  move_limit: '정해진 수를 다 뒀어요',
  repetition: '같은 모양이 3번 나왔어요',
  no_moves: '둘 수 있는 수가 없어요',
};

function renderPanel(st) {
  for (const color of ['black', 'white']) {
    const box = $(`pushed-${color}`);
    box.replaceChildren();
    const opp = color === 'black' ? 'white' : 'black';
    for (let k = 0; k < 6; k++) {
      const s = document.createElement('i');
      s.className = 'slot' + (k < st.pushed[color] ? ` ${opp}` : '');
      box.appendChild(s);
    }
    $(`count-${color}`).textContent = `${st.pushed[color]} / 6`;
    box.title = `${KO[color]}이 밀어낸 구슬 ${st.pushed[color]}개`;
    document.querySelector(`.score[data-color="${color}"]`).classList.toggle('active', st.outcome.status === 'ongoing' && st.side === color);
    $(`tag-${color}`).textContent = ui.human === 'both' ? '' : ui.human === color ? '나' : `컴퓨터 · ${$('level').selectedOptions[0].textContent.split(' ')[0]}`;
  }

  const banner = $('banner');
  const turn = $('turn');
  if (st.outcome.status === 'ongoing') {
    banner.hidden = true;
    const who = ui.human === 'both' ? '' : st.side === ui.human ? ' · 내 차례' : ' · 컴퓨터 차례';
    turn.innerHTML = `<i class="dot ${st.side}"></i>${KO[st.side]} 차례${who}`;
  } else {
    banner.hidden = false;
    const why = REASON[st.outcome.reason] || st.outcome.reason;
    const pushed = `밀어낸 구슬 흑 ${st.pushed.black} : 백 ${st.pushed.white}`;
    let head;
    if (st.outcome.status === 'win') {
      const w = st.outcome.winner;
      head = ui.human === 'both' ? `${KO[w]} 승리!` : w === ui.human ? '이겼어요!' : '컴퓨터가 이겼어요';
    } else {
      head = '비겼어요';
    }
    banner.innerHTML = `<strong>${head}</strong><span>${why} · ${pushed}</span>`;
    turn.textContent = '대국 끝';
  }
  $('ply').textContent = st.move_limit > 0 ? `${st.ply} / ${st.move_limit}수` : `${st.ply}수`;

  // 기보: 한 줄에 흑·백 한 쌍
  const list = $('moves');
  list.replaceChildren();
  const offs = st.push_offs || [];
  for (let i = 0; i < st.history.length; i += 2) {
    const li = document.createElement('li');
    for (const j of [i, i + 1]) {
      if (j >= st.history.length) break;
      const s = document.createElement('span');
      s.textContent = st.history[j] + (offs[j] ? '×' : '');
      if (offs[j]) s.classList.add('off');
      if (j === st.history.length - 1) s.classList.add('last');
      li.appendChild(s);
    }
    list.appendChild(li);
  }
  list.scrollTop = list.scrollHeight;

  $('undo').disabled = st.history.length === 0 || (ui.human !== 'both' && !st.history.some((_, j) => moverOf(j) === ui.human));
  $('clear').disabled = ui.selection.length === 0;
  renderFeatures();
}

const FEATURE_LABELS = {
  pushed_off: '밀어낸 구슬',
  center_distance: '중앙까지 거리 합',
  cohesion: '뭉침 (인접 쌍)',
  edge: '가장자리 구슬',
  endangered: '다음 수에 밀려날 구슬',
  attackable: '다음 수에 밀어낼 구슬',
  isolated: '고립된 구슬',
};

const signed = (v) => (v > 0 ? `+${v}` : String(v));

/** 점수(흑 관점)를 사람이 읽는 말로. 1000점 = 구슬 하나. */
function scoreText(v) {
  if (Math.abs(v) >= 999000) return v > 0 ? '흑이 이기는 수순을 찾음' : '백이 이기는 수순을 찾음';
  const who = v > 0 ? '흑' : '백';
  return v === 0 ? '비슷함' : `${who} 유리 (구슬 ${(Math.abs(v) / 1000).toFixed(2)}개만큼)`;
}

function renderFeatures() {
  const f = ui.features, ev = ui.evaluation;
  if (!f || !ev) return;
  const rows = Object.entries(FEATURE_LABELS)
    .map(([k, label]) => `<tr><td>${label}</td><td>${f.black[k]}</td><td>${f.white[k]}</td><td>${ev.weights[k]}</td><td>${signed(ev.contrib_black[k])}</td></tr>`)
    .join('');
  $('features').innerHTML = `<tr><th>특징값</th><th>흑</th><th>백</th><th>가중치</th><th>기여(흑)</th></tr>${rows}`;
  $('eval-line').textContent = `정적 평가 ${signed(ev.score_black)} — ${scoreText(ev.score_black)}. 기여 = 가중치 × (흑 − 백).`;
  const ls = ui.lastSearch;
  if (ls && ls.search) {
    const s = ls.search;
    $('search-line').textContent =
      `${ui.state.history.length - ls.ply === 1 ? '방금' : `${ls.ply + 1}수째`} 컴퓨터(${KO[ls.color]}) 탐색: 깊이 ${s.depth}, ` +
      `${s.nodes.toLocaleString()}노드, ${s.ms}ms, 점수 ${signed(s.score_black)}(흑 기준). 주 변화: ${s.pv.join(' ')}`;
  } else {
    $('search-line').textContent = '';
  }
}

function moverOf(ply) {
  // 모든 시작 배치에서 흑이 먼저 둔다
  return ply % 2 === 0 ? 'black' : 'white';
}

// ---------------------------------------------------------------- 안내 문장

/** 못 가는 이유. 엔진이 계산한 코드와 숫자로만 만든다. */
function reasonText(b) {
  switch (b.code) {
    case 'not_own': return '내 구슬이 아니에요.';
    case 'not_line': return '한 줄로 붙어 있는 구슬만 같이 움직일 수 있어요.';
    case 'too_many': return '한 번에 3개까지만 움직일 수 있어요.';
    case 'off_board': return '내 구슬을 판 밖으로 보낼 수는 없어요.';
    case 'not_stronger':
      return b.theirs >= 3
        ? `상대 구슬이 ${b.theirs}개 줄지어 있으면 밀 수 없어요. 한 번에 3개까지만 밀 수 있거든요.`
        : `상대 구슬 ${b.theirs}개를 밀려면 내 구슬이 ${b.theirs + 1}개 필요해요. 지금은 ${b.mine}개예요.`;
    case 'own_behind': return '상대 구슬 바로 뒤에 내 구슬이 있으면 밀 수 없어요.';
    case 'broadside_blocked': return '옆으로 나란히 갈 때는 가는 칸이 모두 비어 있어야 해요. 옆으로는 밀 수 없어요.';
    case 'own_in_way': return '앞에 내 구슬이 있어요. 그 구슬까지 같이 쓸어서 골라요.';
    default: return '그쪽으로는 못 가요.';
  }
}

function say(text, kind = 'info') {
  const m = $('msg');
  m.textContent = text;
  m.className = `msg ${kind}`;
}

function idleMessage() {
  const st = ui.state;
  if (!st) return;
  if (st.outcome.status !== 'ongoing') {
    say('새 대국을 눌러 다시 해 봐요.');
  } else if (!isHumanTurn()) {
    say('컴퓨터가 생각하고 있어요…');
  } else if (!ui.selection.length) {
    say(`움직일 내 구슬(${KO[st.side]})을 손가락으로 쓱 쓸어서 골라요. 하나만 움직이려면 그 구슬을 눌러요.`);
  }
}

function selectionMessage() {
  const opts = ui.options || [];
  const n = ui.selection.length;
  const ok = opts.filter((o) => o.move);
  if (!ok.length) {
    const reasons = [...new Set(opts.map((o) => o.blocked && o.blocked.code))];
    const none = `${n === 1 ? '이 구슬은' : '고른 구슬들은'} 지금 움직일 수 없어요. 회색 화살표를 누르면 이유를 알려 줘요.`;
    say(reasons.length === 1 && reasons[0] ? reasonText(opts[0].blocked) : none, 'warn');
    return;
  }
  const extra = ok.some((o) => o.move.push_off) ? ' 빨간 화살표는 상대 구슬을 판 밖으로 밀어내요!' : '';
  say(`${n === 1 ? '구슬 하나를' : `구슬 ${n}개를`} 골랐어요. 큰 화살표를 눌러 움직여요.${extra}`);
}

// ---------------------------------------------------------------- 입력

/** a에서 b까지 한 줄(최대 3칸)이면 칸 목록, 아니면 null */
function segment(a, b) {
  const A = CELLS[a], B = CELLS[b];
  const dr = B.r - A.r, dc = B.c - A.c;
  if (dr === 0 && dc === 0) return [a];
  for (const [ur, uc] of DIRS) {
    for (const k of [1, 2]) {
      if (ur * k === dr && uc * k === dc) {
        const out = [];
        for (let s = 0; s <= k; s++) out.push(cellIndex(A.r + ur * s, A.c + uc * s));
        return out;
      }
    }
  }
  return null;
}

async function select(cells) {
  ui.selection = cells;
  if (cells.length === 0) {
    ui.options = null;
  } else {
    ui.options = null;
    const want = cells.join(',');
    const res = await call('group', { cells });
    if (ui.selection.join(',') !== want) return; // 그새 선택이 바뀜
    ui.options = res.options;
  }
  render();
  if (cells.length) selectionMessage();
  else idleMessage();
}

/** 빈칸이나 상대 구슬을 눌렀을 때 (내 구슬은 쓸기 제스처가 처리) */
function onCellTap(i) {
  if (!isHumanTurn() || ui.busy) return;
  const v = ui.state.board[i];
  // 빈칸이나 상대 구슬: 고른 구슬이 갈 칸(또는 밀릴 상대 구슬)이면 그 수를 둔다
  if (ui.selection.length && ui.options) {
    const hit = ui.options.filter((o) => o.move && (destinations(o.move).includes(i) || pushedCells(o.move).includes(i)));
    if (hit.length === 1) {
      choose(hit[0]);
      return;
    }
  }
  if (v !== '.' && v !== ownChar()) {
    say(`그건 상대 구슬이에요. 내 구슬(${KO[ui.state.side]})을 눌러요.`, 'warn');
    ui.selection = [];
    ui.options = null;
    render();
    return;
  }
  select([]);
}

function choose(o) {
  if (o.move) {
    playHuman(o.move);
  } else {
    say(reasonText(o.blocked), 'warn');
    shake();
  }
}

function shake() {
  board.classList.remove('shake');
  void board.getBoundingClientRect();
  board.classList.add('shake');
}

// ---------------------------------------------------------------- 수 두기와 애니메이션

/** 직전 수(state.last)를 이전 판 위에서 미끄러지듯 보여준다. */
function animateLast(prevBoard, st) {
  const last = st.last;
  if (!last) return Promise.resolve();
  renderMarbles(prevBoard);
  layers.marks.replaceChildren();
  layers.sel.replaceChildren();
  layers.arrows.replaceChildren();
  layers.glow.replaceChildren();
  const color = prevBoard[last.cells[0]];
  const opp = color === 'b' ? 'w' : 'b';
  const moving = [...last.cells];
  const inline = last.cells.length === 1 || last.cells.includes(step(last.cells[0], last.dir)) || last.cells.includes(step(last.cells[0], (last.dir + 3) % 6));
  if (inline) {
    const head = last.cells.find((i) => !last.cells.includes(step(i, last.dir)));
    for (let t = step(head, last.dir); t !== undefined && prevBoard[t] === opp; t = step(t, last.dir)) moving.push(t);
  }
  const [dr, dc] = DIRS[last.dir];
  const dx = (dc - dr / 2) * S, dy = -dr * H;
  const anims = moving.map((i) => {
    const e = marbleEls.get(i);
    if (!e) return Promise.resolve();
    e.parentNode.appendChild(e); // 움직이는 구슬을 위로
    const off = step(i, last.dir) === undefined;
    const frames = off
      ? [
          { transform: 'translate(0px, 0px)', opacity: 1 },
          { transform: `translate(${dx}px, ${dy}px)`, opacity: 1, offset: 0.6 },
          { transform: `translate(${dx * 1.6}px, ${dy * 1.6 + 30}px) scale(0.4)`, opacity: 0 },
        ]
      : [{ transform: 'translate(0px, 0px)' }, { transform: `translate(${dx}px, ${dy}px)` }];
    e.style.transformBox = 'fill-box';
    e.style.transformOrigin = 'center';
    const a = e.animate(frames, { duration: off ? MOVE_MS * 2.2 : MOVE_MS, easing: 'ease-in-out', fill: 'forwards' });
    return a.finished.catch(() => {});
  });
  return Promise.all(anims);
}

async function applyMove(res) {
  const prev = ui.state.board;
  setState(res);
  if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    await animateLast(prev, res.state);
  }
  render();
}

async function playHuman(m) {
  if (!isHumanTurn() || ui.busy) return;
  ui.busy = true;
  try {
    const res = await call('play', { text: m.text });
    await applyMove(res);
    if (m.push_off) say('밀어냈어요! 상대 구슬 하나가 판 밖으로 나갔어요.', 'good');
    else idleMessage();
  } finally {
    ui.busy = false;
  }
  maybeAi();
}

function maybeAi() {
  const st = ui.state;
  if (!st || st.outcome.status !== 'ongoing' || ui.human === 'both' || st.side === ui.human) {
    if (st && st.outcome.status !== 'ongoing') idleMessage();
    return;
  }
  const token = ++ui.aiToken;
  render();
  idleMessage();
  setTimeout(async () => {
    if (token !== ui.aiToken) return;
    ui.busy = true;
    try {
      const color = ui.state.side;
      const ply = ui.state.history.length;
      const r = await call('ai', { level: $('level').value });
      if (token !== ui.aiToken || !r.move) return;
      ui.lastSearch = { color, ply, search: r.search };
      const pushedBefore = ui.state.pushed;
      const res = await call('play', { text: r.move });
      if (token !== ui.aiToken) return;
      await applyMove(res);
      const lost = res.state.pushed[res.state.side === 'black' ? 'white' : 'black'] > pushedBefore[res.state.side === 'black' ? 'white' : 'black'];
      if (res.state.outcome.status === 'ongoing') {
        say(lost ? '앗, 컴퓨터가 내 구슬을 밀어냈어요. 이제 내 차례예요.' : '컴퓨터가 움직였어요(노란 테두리). 이제 내 차례예요.', lost ? 'warn' : 'info');
      }
    } finally {
      ui.busy = false;
    }
    maybeAi();
  }, AI_DELAY_MS);
}

async function undo() {
  ui.aiToken++; // 진행 중인 컴퓨터 수 취소
  ui.busy = true;
  try {
    let res = await call('undo', { count: 1 });
    // 컴퓨터 상대일 때는 내 차례가 될 때까지 더 무른다
    while (ui.human !== 'both' && res.state.history.length > 0 && res.state.side !== ui.human) {
      res = await call('undo', { count: 1 });
    }
    setState(res);
    render();
    say('한 수 물렀어요.');
  } finally {
    ui.busy = false;
  }
  maybeAi();
}

async function newGame() {
  ui.aiToken++;
  ui.human = $('human').value;
  ui.lastSearch = null;
  const limit = Math.max(0, Math.min(2000, parseInt($('limit').value, 10) || 0));
  $('limit').value = limit;
  ui.busy = true;
  try {
    setState(await call('new', { layout: $('layout').value, moveLimit: limit }));
    render();
    idleMessage();
  } catch (e) {
    say(`엔진 오류: ${e.message}`, 'warn');
  } finally {
    ui.busy = false;
  }
  maybeAi();
}

$('setup').addEventListener('submit', (e) => {
  e.preventDefault();
  newGame();
});
$('undo').addEventListener('click', undo);
$('level').addEventListener('change', () => ui.state && renderPanel(ui.state)); // 다음 컴퓨터 수부터 적용
$('clear').addEventListener('click', () => select([]));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') select([]);
});

buildBoard();
newGame();
