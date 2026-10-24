// 아발론 웹 UI. 규칙 판단은 전부 워커 안의 엔진이 하고, 여기서는 그리기와 입력만 다룬다.

const SVG_NS = 'http://www.w3.org/2000/svg';
const S = 100; // 칸 간격
const H = S * Math.sqrt(3) / 2;
const R_MARBLE = 42;
// 엔진과 같은 방향 순서: E, NE, NW, W, SW, SE  (dr, dc)
const DIRS = [[0, 1], [1, 1], [1, 0], [0, -1], [-1, -1], [-1, 0]];
const ROWS = 'ABCDEFGHI';
const AI_DELAY_MS = 450;

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
worker.onerror = (e) => showHint(`엔진을 불러오지 못했다: ${e.message || e}`);
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
  legal: [],
  features: null,
  legalByKey: new Map(),
  selection: [], // 정규 칸 번호
  human: 'black',
  busy: false,
  aiToken: 0,
};

const $ = (id) => document.getElementById(id);
const board = $('board');

function moveKey(cells, dir) {
  return [...cells].sort((a, b) => a - b).join(',') + ':' + dir;
}

function apply(res) {
  ui.state = res.state;
  ui.legal = res.legal;
  ui.features = res.features;
  ui.legalByKey = new Map(res.legal.map((m) => [moveKey(m.cells, m.dir), m]));
  ui.selection = [];
  render();
}

const isHumanTurn = () =>
  ui.state && ui.state.outcome.status === 'ongoing' && (ui.human === 'both' || ui.state.side === ui.human);

// ---------------------------------------------------------------- 판 그리기 (고정 요소)

const layers = {};
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
    const c0 = Math.max(0, r - 4);
    const [x, y] = px(r, c0 - 1.32);
    el('text', { x, y, class: 'label' }, labels).textContent = ROWS[r];
  }
  for (let c = 0; c < 9; c++) {
    const r0 = Math.max(0, c - 4);
    const [x, y] = px(r0 - 1.3, c);
    el('text', { x, y, class: 'label' }, labels).textContent = String(c + 1);
  }

  layers.marks = el('g', {}, board);
  layers.marbles = el('g', {}, board);
  layers.sel = el('g', {}, board);
  layers.hits = el('g', {}, board);
  layers.arrows = el('g', {}, board);

  CELLS.forEach(({ r, c, name }, i) => {
    const [x, y] = px(r, c);
    const hit = el('circle', { cx: x, cy: y, r: S / 2, class: 'hit', 'data-i': i }, layers.hits);
    hit.setAttribute('aria-label', name);
  });
  layers.hits.addEventListener('click', (e) => {
    const i = e.target.getAttribute('data-i');
    if (i !== null) onCellTap(Number(i));
  });
}

// ---------------------------------------------------------------- 렌더

function render() {
  const st = ui.state;
  if (!st) return;
  renderMarbles(st);
  renderSelection();
  renderPanel(st);
}

function renderMarbles(st) {
  layers.marbles.replaceChildren();
  layers.marks.replaceChildren();
  for (let i = 0; i < CELLS.length; i++) {
    const v = st.board[i];
    if (v === '.') continue;
    const [x, y] = px(CELLS[i].r, CELLS[i].c);
    el('circle', { cx: x, cy: y, r: R_MARBLE, class: `marble ${v === 'b' ? 'black' : 'white'}` }, layers.marbles);
  }
  if (st.last) {
    const [dr, dc] = DIRS[st.last.dir];
    for (const i of st.last.cells) {
      const { r, c } = CELLS[i];
      const [x, y] = px(r, c);
      const [tx, ty] = px(r + dr, c + dc);
      if (st.board[i] === '.') el('circle', { cx: x, cy: y, r: R_MARBLE * 0.55, class: 'last-from' }, layers.marks);
      el('circle', { cx: tx, cy: ty, r: R_MARBLE + 3, class: 'last-to' }, layers.marks);
    }
  }
}

/** 선택된 줄에서 둘 수 있는 수 (방향별) */
function selectionMoves() {
  if (!ui.selection.length) return [];
  const out = [];
  for (let d = 0; d < 6; d++) {
    const m = ui.legalByKey.get(moveKey(ui.selection, d));
    if (m) out.push(m);
  }
  return out;
}

/** 수를 두면 내 구슬이 새로 차지하는 칸 */
function destinations(m) {
  const [dr, dc] = DIRS[m.dir];
  return m.cells
    .map((i) => cellIndex(CELLS[i].r + dr, CELLS[i].c + dc))
    .filter((i) => i !== undefined && !m.cells.includes(i));
}

function renderSelection() {
  layers.sel.replaceChildren();
  layers.arrows.replaceChildren();
  if (!ui.selection.length) {
    showHint(isHumanTurn() ? '움직일 구슬을 1~3개 고른다' : '');
    return;
  }
  const pts = ui.selection.map((i) => px(CELLS[i].r, CELLS[i].c));
  for (const [x, y] of pts) el('circle', { cx: x, cy: y, r: R_MARBLE + 4, class: 'sel-ring' }, layers.sel);

  const moves = selectionMoves();
  if (!moves.length) {
    showHint('이 줄은 지금 움직일 수 있는 방향이 없다');
    return;
  }
  const cx = pts.reduce((a, p) => a + p[0], 0) / pts.length;
  const cy = pts.reduce((a, p) => a + p[1], 0) / pts.length;
  for (const m of moves) {
    for (const t of destinations(m)) {
      const [x, y] = px(CELLS[t].r, CELLS[t].c);
      el('circle', { cx: x, cy: y, r: R_MARBLE * 0.5, class: 'target' }, layers.sel);
    }
    const [dr, dc] = DIRS[m.dir];
    const [vx0, vy0] = [(dc - dr / 2) * S, -dr * H];
    const len = Math.hypot(vx0, vy0);
    const [ux, uy] = [vx0 / len, vy0 / len];
    const reach = Math.max(...pts.map(([x, y]) => (x - cx) * ux + (y - cy) * uy));
    const dist = reach + S * 0.95;
    const ax = cx + ux * dist, ay = cy + uy * dist;
    const angle = (Math.atan2(uy, ux) * 180) / Math.PI;
    const kind = m.push_off ? 'off' : isPush(m) ? 'push' : '';
    const g = el('g', { class: `arrow ${kind}`, transform: `translate(${ax} ${ay})`, role: 'button', tabindex: 0 }, layers.arrows);
    g.setAttribute('aria-label', `${m.text}${m.push_off ? ' 밀어내기' : ''}`);
    el('circle', { r: 30 }, g);
    el('path', { d: 'M -12 -13 L 16 0 L -12 13 Z', transform: `rotate(${angle})` }, g);
    const go = (e) => {
      e.stopPropagation();
      playHuman(m);
    };
    g.addEventListener('click', go);
    g.addEventListener('keydown', (e) => (e.key === 'Enter' || e.key === ' ') && go(e));
  }
  const offs = moves.filter((m) => m.push_off).length;
  showHint(offs ? '빨간 화살표는 상대 구슬을 판 밖으로 밀어낸다' : '화살표나 표시된 칸을 눌러 이동');
}

function isPush(m) {
  if (!m.inline) return false;
  const [dr, dc] = DIRS[m.dir];
  const color = ui.state.side === 'black' ? 'w' : 'b';
  return m.cells.some((i) => {
    const t = cellIndex(CELLS[i].r + dr, CELLS[i].c + dc);
    return t !== undefined && ui.state.board[t] === color;
  });
}

const REASON = {
  pushed_off: '구슬 6개를 밀어냄',
  move_limit: '수 제한 도달',
  repetition: '같은 국면 3회 반복',
  no_moves: '둘 수 있는 수 없음',
};
const KO = { black: '흑', white: '백' };

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
    box.title = `${KO[color]}이 밀어낸 구슬 ${st.pushed[color]}개`;
    document.querySelector(`.score[data-color="${color}"]`).classList.toggle('active', st.outcome.status === 'ongoing' && st.side === color);
    $(`tag-${color}`).textContent = ui.human === 'both' ? '' : ui.human === color ? ' 나' : ' 랜덤 AI';
  }

  const banner = $('banner');
  if (st.outcome.status === 'ongoing') {
    banner.hidden = true;
    $('turn').textContent = `${KO[st.side]} 차례` + (isHumanTurn() ? '' : ' (AI 생각 중)');
  } else {
    banner.hidden = false;
    const why = REASON[st.outcome.reason] || st.outcome.reason;
    const pushed = `밀어낸 구슬 흑 ${st.pushed.black} : 백 ${st.pushed.white}`;
    banner.textContent =
      st.outcome.status === 'win' ? `${KO[st.outcome.winner]} 승리 — ${why} (${pushed})` : `무승부 — ${why} (${pushed})`;
    $('turn').textContent = '대국 종료';
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

function renderFeatures() {
  const f = ui.features;
  const t = $('features');
  if (!f) return;
  const rows = Object.entries(FEATURE_LABELS)
    .map(([k, label]) => `<tr><td>${label}</td><td>${f.black[k]}</td><td>${f.white[k]}</td></tr>`)
    .join('');
  t.innerHTML = `<tr><th>특징값</th><th>흑</th><th>백</th></tr>${rows}`;
}

function moverOf(ply) {
  // 모든 시작 배치에서 흑이 먼저 둔다
  return ply % 2 === 0 ? 'black' : 'white';
}

function showHint(text) {
  $('hint').textContent = text;
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

function trySelect(cells) {
  // 가장 먼 두 칸을 잇는 줄이 모든 칸을 포함하고 전부 내 구슬이면 그 줄을 선택
  const own = ui.state.side === 'black' ? 'b' : 'w';
  let best = null;
  for (const a of cells) {
    for (const b of cells) {
      const seg = segment(a, b);
      if (seg && (!best || seg.length > best.length)) best = seg;
    }
  }
  if (!best || !cells.every((c) => best.includes(c))) return null;
  if (!best.every((c) => ui.state.board[c] === own)) return null;
  return best;
}

function onCellTap(i) {
  if (!isHumanTurn() || ui.busy) return;
  const own = ui.state.side === 'black' ? 'b' : 'w';
  const v = ui.state.board[i];
  if (v === own) {
    if (ui.selection.includes(i)) {
      const rest = ui.selection.filter((x) => x !== i);
      ui.selection = rest.length && trySelect(rest)?.length === rest.length ? rest : [];
    } else {
      ui.selection = trySelect([...ui.selection, i]) || [i];
    }
    render();
    return;
  }
  // 빈칸이나 상대 구슬: 선택한 줄의 이동 도착 칸이면 그 수를 둔다
  if (ui.selection.length) {
    const hit = selectionMoves().filter((m) => destinations(m).includes(i) || pushedCells(m).includes(i));
    if (hit.length === 1) {
      playHuman(hit[0]);
      return;
    }
  }
  ui.selection = [];
  render();
}

/** 인라인 수에서 앞에 있는 상대 구슬 칸 (그 칸을 눌러도 밀기로 인식) */
function pushedCells(m) {
  if (!m.inline) return [];
  const [dr, dc] = DIRS[m.dir];
  const out = [];
  for (const i of m.cells) {
    const t = cellIndex(CELLS[i].r + dr, CELLS[i].c + dc);
    if (t !== undefined && !m.cells.includes(t) && ui.state.board[t] !== '.') out.push(t);
  }
  return out;
}

async function playHuman(m) {
  if (!isHumanTurn() || ui.busy) return;
  ui.busy = true;
  try {
    const res = await call('play', { text: m.text });
    apply(res);
  } finally {
    ui.busy = false;
  }
  maybeAi();
}

function maybeAi() {
  const st = ui.state;
  if (!st || st.outcome.status !== 'ongoing' || ui.human === 'both' || st.side === ui.human) return;
  const token = ++ui.aiToken;
  render();
  setTimeout(async () => {
    if (token !== ui.aiToken) return;
    ui.busy = true;
    try {
      const r = await call('random');
      if (token !== ui.aiToken || !r.move) return;
      const res = await call('play', { text: r.move });
      if (token !== ui.aiToken) return;
      apply(res);
    } finally {
      ui.busy = false;
    }
    maybeAi();
  }, AI_DELAY_MS);
}

async function undo() {
  ui.aiToken++; // 진행 중인 AI 수 취소
  ui.busy = true;
  try {
    let res = await call('undo', { count: 1 });
    // AI 상대일 때는 내 차례가 될 때까지 더 무른다
    while (ui.human !== 'both' && res.state.history.length > 0 && res.state.side !== ui.human) {
      res = await call('undo', { count: 1 });
    }
    apply(res);
  } finally {
    ui.busy = false;
  }
  maybeAi();
}

async function newGame() {
  ui.aiToken++;
  ui.human = $('human').value;
  const limit = Math.max(0, Math.min(2000, parseInt($('limit').value, 10) || 0));
  $('limit').value = limit;
  ui.busy = true;
  try {
    apply(await call('new', { layout: $('layout').value, moveLimit: limit }));
  } catch (e) {
    showHint(`엔진 오류: ${e.message}`);
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
$('clear').addEventListener('click', () => {
  ui.selection = [];
  render();
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    ui.selection = [];
    render();
  }
});

buildBoard();
newGame();
