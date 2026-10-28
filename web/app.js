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
const CANDIDATE_MS = 300; // 친절 모드: 두기 전에 수를 분석하는 시간
const HINT_MS = 700;
const REVIEW_MS = 250; // 복기: 수마다 분석 시간

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
  mode: 'play', // 'play' | 'review'
  grades: {}, // 기보 번호 → 내가 둔 수의 분석 (친절 모드)
  hint: null, // 힌트로 보여줄 수
  review: { cache: {}, cur: -1, token: 0, board: null },
  loadedTags: null, // 불러온 기보의 태그 ([[이름, 값], ...]). 저장할 때 이름·날짜를 그대로 쓴다
  counted: false, // 이 대국 결과를 내 기록에 넣었는지 (불러온 대국은 넣지 않는다)
  only: null, // 누르기 조작: 누른 칸으로 갈 수 있는 수가 여럿일 때 화살표로 보여줄 방향들
  legal: [], // 둘 차례의 합법수 (첫 돌·끝점 조작에서 쓴다)
  endsFirst: null, // 첫 돌·끝점 조작: 고른 첫 돌
  endsChoice: null, // 첫 돌·끝점 조작: 같은 두 칸으로 갈 수 있는 수가 여럿일 때 그 수들
};

const $ = (id) => document.getElementById(id);
const board = $('board');
const KO = { black: '흑', white: '백' };
const ownChar = () => (ui.state.side === 'black' ? 'b' : 'w');

const isHumanTurn = () =>
  ui.mode === 'play' &&
  ui.state &&
  ui.state.outcome.status === 'ongoing' &&
  (ui.human === 'both' || ui.state.side === ui.human);
const friendly = () => $('friendly').checked;
const control = () => $('control').value; // 'swipe' | 'tap' | 'ends'

function setState(res) {
  ui.state = res.state;
  ui.features = res.features;
  ui.evaluation = res.evaluation;
  ui.singles = res.singles || {};
  ui.legal = res.legal || [];
  ui.endsFirst = null;
  ui.endsChoice = null;
  ui.selection = [];
  ui.options = null;
  ui.only = null;
  ui.hint = null;
  // 무르기 등으로 기보가 짧아지면 그 뒤 분석은 버린다
  const n = ui.state.history.length;
  for (const k of Object.keys(ui.grades)) if (Number(k) >= n) delete ui.grades[k];
  for (const k of Object.keys(ui.review.cache)) if (Number(k) >= n) delete ui.review.cache[k];
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
  layers.notes = el('g', {}, board); // 힌트·복기 화살표
  layers.sel = el('g', {}, board);
  layers.hits = el('g', {}, board);
  layers.arrows = el('g', {}, board);

  CELLS.forEach(({ r, c, name }, i) => {
    const [x, y] = px(r, c);
    const hit = el('circle', { cx: x, cy: y, r: S / 2, class: 'hit', 'data-i': i }, layers.hits);
    hit.setAttribute('aria-label', name);
  });

  // 조작 방식별 제스처 (control()):
  // - swipe: 내 구슬에서 시작해 쓸면 지나간 줄(최대 3개)을 고른다. 쓸지 않고 떼면 탭.
  // - tap: 내 구슬을 누를 때마다 선택에 넣거나 뺀다. 쓸기는 없다.
  layers.hits.addEventListener('pointerdown', (e) => {
    const i = e.target.getAttribute('data-i');
    if (i === null) return;
    const start = Number(i);
    const own = isHumanTurn() && !ui.busy && ui.state.board[start] === ownChar();
    const mode = control();
    if (own && (mode === 'tap' || mode === 'ends')) {
      ui.gesture = { type: 'tap', start, id: e.pointerId, own };
      return;
    }
    ui.gesture = { type: 'swipe', start, id: e.pointerId, own, cells: [start], moved: false };
    if (own) {
      ui.dragging = true;
      ui.wasOnlyThis = ui.selection.length === 1 && ui.selection[0] === start;
      showSelection([start]);
    }
  });
  window.addEventListener('pointermove', (e) => {
    const g = ui.gesture;
    if (!g || !g.own || e.pointerId !== g.id || g.type === 'tap') return;
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
    if (g.type === 'tap') {
      if (e.type === 'pointerup') (control() === 'ends' ? endsTap(g.start) : tapToggle(g.start));
      return;
    }
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
  // 판 위에서 손가락을 움직이면 화면이 스크롤되지 않게 한다. CSS touch-action: none만으로는
  // SVG 위에서 일부 브라우저(특히 iOS Safari)가 위아래 쓸기를 스크롤로 가져가서, touchmove의 기본 동작을 직접 막는다.
  // touchstart는 막지 않는다(막으면 화살표의 click이 사라진다).
  board.addEventListener('touchmove', (e) => e.cancelable && e.preventDefault(), { passive: false });
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
  if (ui.mode === 'review') {
    renderReviewBoard();
  } else {
    renderMarbles(st.board);
    renderLast(st);
    renderNotes();
    renderSelection();
  }
  renderPanel(st);
}

/** 수 하나를 화살표로: 움직이는 구슬에 고리, 구슬들의 가운데에서 도착 칸 가운데로 화살표 */
function drawMoveArrow(layer, m, cls) {
  const [dr, dc] = DIRS[m.dir];
  const from = m.cells.map((i) => px(CELLS[i].r, CELLS[i].c));
  for (const [x, y] of from) el('circle', { cx: x, cy: y, r: R_MARBLE + 6, class: `mv-from ${cls}` }, layer);
  const fx = from.reduce((a, p) => a + p[0], 0) / from.length;
  const fy = from.reduce((a, p) => a + p[1], 0) / from.length;
  const [ux, uy] = dirVec(m.dir);
  const head = m.inline ? m.cells.find((i) => !m.cells.includes(step(i, m.dir))) : null;
  let [sx, sy] = [fx, fy];
  if (head !== null) [sx, sy] = px(CELLS[head].r, CELLS[head].c);
  const len = S * 0.95;
  const [tx, ty] = [sx + ux * len, sy + uy * len];
  const g = el('g', { class: `mv-arrow ${cls}` }, layer);
  el('line', { x1: sx, y1: sy, x2: tx - ux * 22, y2: ty - uy * 22 }, g);
  const [nx, ny] = [-uy, ux];
  el('polygon', { points: `${tx},${ty} ${tx - ux * 34 + nx * 22},${ty - uy * 34 + ny * 22} ${tx - ux * 34 - nx * 22},${ty - uy * 34 - ny * 22}` }, g);
  void dr; void dc;
}

function renderNotes() {
  layers.notes.replaceChildren();
  if (ui.hint && isHumanTurn() && !ui.selection.length && ui.endsFirst === null) drawMoveArrow(layers.notes, ui.hint, 'best');
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

  if (control() === 'ends' && ui.endsFirst !== null) {
    renderEnds();
    return;
  }
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

  const mode = control();
  for (const o of opts) {
    const [ux, uy] = dirVec(o.dir);
    const m = o.move;
    const kind = !m ? 'blocked' : m.push_off ? 'off' : pushedCells(m).length ? 'push' : 'go';
    if (mode === 'tap' && m && !(ui.only && ui.only.includes(o.dir))) {
      // 누르기 조작: 화살표 대신 갈 수 있는 칸에 점. 칸은 아래 hits 층이 누름을 받는다
      for (const t of destinations(m)) {
        const [x, y] = px(CELLS[t].r, CELLS[t].c);
        el('circle', { cx: x, cy: y, r: R_MARBLE * 0.42, class: `tgt ${kind}` }, layers.sel);
      }
      continue;
    }
    const [ax, ay] = arrowAnchor(o, pts);
    const angle = (Math.atan2(uy, ux) * 180) / Math.PI;
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
    $(`tag-${color}`).textContent = ui.human === 'both' ? '' : ui.human === color ? '나' : `컴퓨터 · ${LEVEL_KO[$('level').value]}`;
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
      const a = ui.grades[j];
      if (a) {
        const dot = document.createElement('i');
        dot.className = `g grade ${a.grade}`;
        dot.title = a.grade_ko;
        s.appendChild(dot);
      }
      li.appendChild(s);
    }
    list.appendChild(li);
  }
  list.scrollTop = list.scrollHeight;

  $('undo').disabled = st.history.length === 0 || (ui.human !== 'both' && !st.history.some((_, j) => moverOf(j) === ui.human));
  $('clear').disabled = ui.selection.length === 0;
  $('hint').hidden = !friendly();
  $('hint').disabled = !isHumanTurn();
  $('review-btn').disabled = st.history.length === 0;
  $('record-btn').hidden = ui.mode === 'review';
  $('stats').hidden = ui.mode === 'review';
  $('review-btn').hidden = ui.mode === 'review';
  $('play-only').hidden = ui.mode === 'review';
  $('review').hidden = ui.mode !== 'review';
  document.querySelector('.actions').hidden = ui.mode === 'review';
  document.querySelector('.help').hidden = ui.mode === 'review';
  $('engine').hidden = ui.mode === 'review';
  if (ui.mode === 'review') $('feedback').hidden = true;
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
  push_moves: '밀기 기회 (미는 수)',
  triples: '3개 줄',
  groups: '덩어리 수',
  mobility: '움직일 수 있는 수',
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
    case 'own_in_way': return `앞에 내 구슬이 있어요. 그 구슬까지 같이 ${control() === 'tap' ? '눌러서' : '쓸어서'} 골라요.`;
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
    const me = `내 구슬(${KO[st.side]})`;
    say(
      {
        swipe: `움직일 ${me}을 손가락으로 쓱 쓸어서 골라요. 하나만 움직이려면 그 구슬을 눌러요.`,
        tap: `움직일 ${me}을 하나씩 눌러서 골라요. 한 줄로 붙은 구슬을 3개까지 고를 수 있어요.`,
        ends: `움직일 줄의 첫 ${me}을 누르고, 끝점(점 찍힌 칸)을 눌러요.`,
      }[control()],
    );
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
  const off = ok.some((o) => o.move.push_off);
  const what = n === 1 ? '구슬 하나를' : `구슬 ${n}개를`;
  const mode = control();
  if (mode === 'tap') {
    say(`${what} 골랐어요. 점이 찍힌 칸을 누르면 그쪽으로 움직여요. 다른 내 구슬을 누르면 같이 골라져요.${off ? ' 빨간 점은 상대 구슬을 판 밖으로 밀어내요!' : ''}`);
  } else {
    say(`${what} 골랐어요. 큰 화살표를 눌러 움직여요.${off ? ' 빨간 화살표는 상대 구슬을 판 밖으로 밀어내요!' : ''}`);
  }
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

/** 칸들이 붙은 한 줄(최대 3개)이면 줄 순서대로, 아니면 null */
function lineOf(cells) {
  if (cells.length <= 1) return [...cells];
  for (const a of cells) {
    for (const b of cells) {
      const seg = a !== b && segment(a, b);
      if (seg && seg.length === cells.length && cells.every((c) => seg.includes(c))) return seg;
    }
  }
  return null;
}

/** 누르기 조작: 내 구슬을 선택에 넣거나 뺀다. 줄이 안 되면 그 구슬부터 새로 고른다. */
function tapToggle(i) {
  const sel = ui.selection;
  let next;
  if (sel.includes(i)) {
    next = lineOf(sel.filter((c) => c !== i)) || []; // 가운데를 빼면 줄이 끊기므로 전부 해제
  } else {
    next = (sel.length < 3 && lineOf([...sel, i])) || [i];
  }
  select(next);
}

// ---------------------------------------------------------------- 첫 돌 · 끝점 조작
// 수 하나를 (첫 돌, 끝점) 두 칸으로 나타낸다.
// - 줄 방향(1개 포함): 첫 돌 = 맨 뒤 돌, 끝점 = 맨 앞 돌이 갈 칸 (상대를 밀면 지금 상대 돌이 있는 칸)
// - 옆걸음: 첫 돌 = 한쪽 끝 돌, 끝점 = 반대쪽 끝 돌이 갈 칸 (두 끝 어느 쪽에서 시작해도 된다)

/** 수 m을 나타내는 (첫 돌, 끝점) 쌍들 */
function endPairs(m) {
  const head = (d) => m.cells.find((c) => !m.cells.includes(step(c, d)));
  if (m.inline) return [[head((m.dir + 3) % 6), step(head(m.dir), m.dir)]];
  const a = m.cells[0], b = m.cells[m.cells.length - 1];
  return [[a, step(b, m.dir)], [b, step(a, m.dir)]];
}

/** 첫 돌 f에서 시작하는 수들: 끝점 → 수 목록 */
function endsFrom(f) {
  const out = new Map();
  for (const m of ui.legal) {
    for (const [first, end] of endPairs(m)) {
      if (first !== f || end === undefined) continue;
      if (!out.has(end)) out.set(end, []);
      if (!out.get(end).includes(m)) out.get(end).push(m);
    }
  }
  // 같은 두 칸으로 여러 수가 되면 줄 방향 수, 그다음 구슬이 적은 수로 친다(구슬 하나 옆 칸 누르기가 가장 흔해서).
  // 밀려난 수도 반대쪽 끝에서 시작하면 둘 수 있다. 무작위 국면 3.6만 개(합법수 239만 개)에서 모든 수가
  // 어떤 (첫 돌, 끝점)으로 하나로 정해지는지 확인했다. 그래도 여럿이 남으면 renderEnds가 화살표로 고르게 한다.
  for (const [end, ms] of out) {
    let keep = ms.some((m) => m.inline) ? ms.filter((m) => m.inline) : ms;
    const n = Math.min(...keep.map((m) => m.cells.length));
    keep = keep.filter((m) => m.cells.length === n);
    out.set(end, keep);
  }
  return out;
}

function endsReset() {
  ui.endsFirst = null;
  ui.endsChoice = null;
}

function endsTap(i) {
  if (!isHumanTurn() || ui.busy) return;
  const own = ui.state.board[i] === ownChar();
  if (ui.endsFirst !== null && !ui.endsChoice) {
    const hit = endsFrom(ui.endsFirst).get(i);
    if (hit && hit.length === 1) {
      const m = hit[0];
      endsReset();
      playHuman(m);
      return;
    }
    if (hit) {
      ui.endsChoice = hit;
      render();
      say('그 칸으로 가는 방법이 두 가지예요. 화살표 중에 골라요.');
      return;
    }
  }
  if (own && i !== ui.endsFirst) {
    endsReset();
    if (!endsFrom(i).size) {
      render();
      say('이 구슬로 시작하는 수가 없어요. 다른 구슬을 눌러요.', 'warn');
      shake();
      return;
    }
    ui.endsFirst = i;
    render();
    say('끝점을 눌러요. 줄 방향이면 맨 앞 구슬이 갈 칸, 옆으로 가면 반대쪽 끝 구슬이 갈 칸이에요.');
    return;
  }
  // 첫 돌을 다시 누르거나 빈 곳을 누르면 처음부터
  endsReset();
  render();
  idleMessage();
}

function renderEnds() {
  const f = ui.endsFirst;
  const [fx, fy] = px(CELLS[f].r, CELLS[f].c);
  el('circle', { cx: fx, cy: fy, r: R_MARBLE + 4, class: 'sel-ring' }, layers.sel);
  if (ui.endsChoice) {
    // 같은 두 칸으로 갈 수 있는 수가 여럿: 각 수를 움직일 구슬 고리 + 누를 수 있는 화살표로
    for (const m of ui.endsChoice) {
      const dest = destinations(m);
      const pts = (dest.length ? dest : m.cells).map((i) => px(CELLS[i].r, CELLS[i].c));
      const ax = pts.reduce((a, p) => a + p[0], 0) / pts.length;
      const ay = pts.reduce((a, p) => a + p[1], 0) / pts.length;
      const [ux, uy] = dirVec(m.dir);
      const kind = m.push_off ? 'off' : pushedCells(m).length ? 'push' : 'go';
      const g = el('g', { class: `arrow ${kind}`, transform: `translate(${ax} ${ay})`, role: 'button', tabindex: 0 }, layers.arrows);
      g.setAttribute('aria-label', m.text);
      el('circle', { r: 38 }, g);
      el('path', { d: 'M -14 -16 L 19 0 L -14 16 Z', transform: `rotate(${(Math.atan2(uy, ux) * 180) / Math.PI})` }, g);
      g.addEventListener('pointerenter', () => endsPreview(m));
      g.addEventListener('pointerleave', () => endsPreview(null));
      const go = (e) => {
        e.stopPropagation();
        endsReset();
        playHuman(m);
      };
      g.addEventListener('click', go);
      g.addEventListener('keydown', (e) => (e.key === 'Enter' || e.key === ' ') && go(e));
    }
    return;
  }
  for (const [end, ms] of endsFrom(f)) {
    const m = ms[0];
    const kind = ms.some((x) => x.push_off) ? 'off' : ms.some((x) => pushedCells(x).length) ? 'push' : 'go';
    const [x, y] = px(CELLS[end].r, CELLS[end].c);
    const dot = el('circle', { cx: x, cy: y, r: R_MARBLE * 0.42, class: `tgt ${kind}` }, layers.sel);
    if (ms.length === 1) dot.dataset.text = m.text;
  }
}

/** 그 수로 움직일 구슬과 도착 칸을 도형처럼 보여준다 */
function endsPreview(m) {
  layers.sel.querySelectorAll('.preview').forEach((e) => e.remove());
  if (!m) return;
  for (const i of m.cells) {
    const [x, y] = px(CELLS[i].r, CELLS[i].c);
    el('circle', { cx: x, cy: y, r: R_MARBLE + 4, class: 'sel-ring preview' }, layers.sel);
  }
  for (const t of destinations(m)) {
    const [x, y] = px(CELLS[t].r, CELLS[t].c);
    el('circle', { cx: x, cy: y, r: R_MARBLE * 0.5, class: 'target preview' }, layers.sel);
  }
}

async function select(cells) {
  ui.selection = cells;
  ui.only = null;
  endsReset();
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
  if (control() === 'ends') {
    endsTap(i);
    return;
  }
  const v = ui.state.board[i];
  // 빈칸이나 상대 구슬: 고른 구슬이 갈 칸(또는 밀릴 상대 구슬)이면 그 수를 둔다
  if (ui.selection.length && ui.options) {
    const hit = ui.options.filter((o) => o.move && (destinations(o.move).includes(i) || pushedCells(o.move).includes(i)));
    if (hit.length === 1) {
      choose(hit[0]);
      return;
    }
    if (hit.length > 1) {
      // 옆걸음 두 방향의 도착 칸이 겹치는 칸: 화살표로 고르게 한다
      ui.only = hit.map((o) => o.dir);
      render();
      say('그 칸으로는 두 가지로 갈 수 있어요. 화살표 중에 골라요.');
      return;
    }
    // 못 가는 방향의 바로 옆 칸을 누르면 이유를 알려준다
    const blocked = ui.options.find((o) => !o.move && ui.selection.some((c) => step(c, o.dir) === i && !ui.selection.includes(i)));
    if (blocked) {
      choose(blocked);
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
  noteResult();
  if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    await animateLast(prev, res.state);
  }
  render();
}

async function playHuman(m) {
  if (!isHumanTurn() || ui.busy) return;
  ui.busy = true;
  let analysis = null;
  try {
    if (friendly()) {
      say('생각해 보는 중…');
      analysis = (await call('analyzeCandidate', { text: m.text, ms: CANDIDATE_MS })).analysis;
      // 큰 실수이거나, 실수 등급이면서 최선 수보다 구슬을 잃는(덜 얻는) 수면 두기 전에 묻는다.
      // 구슬을 내주고 자리를 얻는 수는 엔진 손실이 작으면 경고하지 않는다.
      if (analysis && (analysis.grade === 'blunder' || (analysis.grade === 'mistake' && analysis.material_drop >= 1))) {
        const choice = await confirmBlunder(analysis);
        if (choice !== 'play') {
          ui.busy = false;
          ui.selection = [];
          ui.options = null;
          if (choice === 'hint') {
            await showHint();
          } else {
            render();
            say('좋아요, 다시 생각해 봐요.');
          }
          return;
        }
      }
    }
    const ply = ui.state.history.length;
    const res = await call('play', { text: m.text });
    if (analysis) ui.grades[ply] = analysis;
    showFeedback(analysis);
    await applyMove(res);
    if (m.push_off) say('밀어냈어요! 상대 구슬 하나가 판 밖으로 나갔어요.', 'good');
    else if (!analysis) idleMessage();
    render();
  } finally {
    ui.busy = false;
  }
  maybeAi();
}

function maybeAi() {
  if (ui.mode !== 'play') return;
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
      if (token !== ui.aiToken || !r.move || ui.mode !== 'play') return;
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
  $('feedback').hidden = true;
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
  ui.grades = {};
  ui.review = { cache: {}, cur: -1, token: ui.review.token + 1, board: null };
  ui.mode = 'play';
  ui.loadedTags = null;
  ui.counted = false;
  $('feedback').hidden = true;
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

// ---------------------------------------------------------------- 친절 모드: 피드백, 경고, 힌트

function fillList(ul, sentences) {
  ul.replaceChildren();
  for (const t of sentences) {
    const li = document.createElement('li');
    li.textContent = t;
    ul.appendChild(li);
  }
}

function showFeedback(a) {
  const box = $('feedback');
  if (!a) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  const g = $('fb-grade');
  g.className = `grade ${a.grade}`;
  g.textContent = a.grade_ko;
  fillList($('fb-sentences'), a.sentences);
  const kind = a.grade === 'best' || a.grade === 'good' ? 'good' : a.grade === 'inaccuracy' ? 'info' : 'warn';
  say(a.sentences[0], kind);
}

function confirmBlunder(a) {
  const dlg = $('warn');
  $('warn-grade').textContent =
    a.material_drop >= 1 && a.grade !== 'blunder'
      ? `이 수로는 구슬을 ${a.material_drop}개 손해 볼 수 있어요.`
      : `이 수는 ${a.grade_ko}일 수 있어요.`;
  fillList($('warn-sentences'), a.sentences.slice(1).length ? a.sentences.slice(1) : a.sentences);
  return new Promise((resolve) => {
    const done = (v) => {
      dlg.close();
      resolve(v);
    };
    $('warn-cancel').onclick = () => done('cancel');
    $('warn-play').onclick = () => done('play');
    $('warn-hint').onclick = () => done('hint');
    dlg.oncancel = (e) => {
      e.preventDefault();
      done('cancel');
    };
    dlg.showModal();
  });
}

async function showHint() {
  if (!isHumanTurn() || ui.busy) return;
  ui.busy = true;
  try {
    say('좋은 수를 찾는 중…');
    const h = (await call('hint', { ms: HINT_MS })).hint;
    if (!h) return;
    ui.hint = h.best.move;
    ui.selection = [];
    ui.options = null;
    say(h.sentences.join(' '), 'good');
  } finally {
    ui.busy = false;
    render();
  }
}

// ---------------------------------------------------------------- 복기

const GRADE_SHORT = { best: '최선', good: '좋음', inaccuracy: '아쉬움', mistake: '실수', blunder: '큰 실수' };

async function enterReview() {
  if (!ui.state || !ui.state.history.length) return;
  ui.aiToken++; // 진행 중인 컴퓨터 수 취소
  ui.mode = 'review';
  ui.selection = [];
  ui.options = null;
  const token = ++ui.review.token;
  const n = ui.state.history.length;
  renderReviewList();
  await selectReview(ui.review.cur >= 0 && ui.review.cur < n ? ui.review.cur : 0);
  // 아직 분석하지 않은 수를 차례로 분석한다
  for (let k = 0; k < n; k++) {
    if (token !== ui.review.token) return;
    if (!ui.review.cache[k]) {
      $('rv-progress').textContent = `분석 중 ${k + 1} / ${n}`;
      const a = (await call('analyzePly', { ply: k, ms: REVIEW_MS })).analysis;
      if (token !== ui.review.token) return;
      if (a) ui.review.cache[k] = a;
      renderReviewList();
      if (k === ui.review.cur) renderReviewDetail();
    }
  }
  $('rv-progress').textContent = summaryText();
}

function summaryText() {
  const counts = {};
  for (const a of Object.values(ui.review.cache)) {
    const who = a.mover === 'black' ? '흑' : '백';
    const key = `${who} ${GRADE_SHORT[a.grade]}`;
    if (a.grade === 'mistake' || a.grade === 'blunder') counts[key] = (counts[key] || 0) + 1;
  }
  const parts = Object.entries(counts).map(([k, v]) => `${k} ${v}`);
  return parts.length ? `분석 끝 · ${parts.join(', ')}` : '분석 끝 · 실수 없음';
}

function exitReview() {
  ui.review.token++;
  ui.mode = 'play';
  render();
  idleMessage();
  maybeAi();
}

function renderReviewList() {
  const list = $('rv-list');
  list.replaceChildren();
  const hist = ui.state.history;
  hist.forEach((t, k) => {
    const li = document.createElement('li');
    if (k === ui.review.cur) li.classList.add('cur');
    const no = document.createElement('span');
    no.className = 'no';
    no.textContent = `${k + 1}.`;
    const who = document.createElement('i');
    who.className = `dot ${moverOf(k)}`;
    const mv = document.createElement('span');
    mv.textContent = t;
    li.append(no, who, mv);
    const a = ui.review.cache[k];
    const tag = document.createElement('span');
    if (a) {
      tag.className = `grade ${a.grade}`;
      tag.textContent = GRADE_SHORT[a.grade];
    } else {
      tag.className = 'pending';
      tag.textContent = '분석 대기';
    }
    li.appendChild(tag);
    li.addEventListener('click', () => selectReview(k));
    list.appendChild(li);
  });
  const cur = list.children[ui.review.cur];
  if (cur) cur.scrollIntoView({ block: 'nearest' });
}

async function selectReview(k) {
  const n = ui.state.history.length;
  if (k < 0 || k >= n) return;
  ui.review.cur = k;
  const st = (await call('stateAt', { ply: k })).state;
  if (ui.review.cur !== k) return;
  ui.review.board = st;
  renderReviewList();
  renderReviewDetail();
  render();
}

function renderReviewBoard() {
  const st = ui.review.board;
  layers.marks.replaceChildren();
  layers.notes.replaceChildren();
  layers.sel.replaceChildren();
  layers.arrows.replaceChildren();
  layers.glow.replaceChildren();
  if (!st) return;
  renderMarbles(st.board);
  const a = ui.review.cache[ui.review.cur];
  if (a) {
    if (a.best.move.text !== a.played.move.text) drawMoveArrow(layers.notes, a.best.move, 'best');
    drawMoveArrow(layers.notes, a.played.move, 'played');
  }
}

function renderReviewDetail() {
  const k = ui.review.cur;
  const a = ui.review.cache[k];
  const who = KO[moverOf(k)];
  const text = ui.state.history[k];
  if (!a) {
    fillList($('rv-sentences'), [`${k + 1}. ${who} ${text} — 분석하는 중이에요…`]);
    $('rv-engine').textContent = '';
    say(`${k + 1}번째 수: ${who} ${text}`);
    return;
  }
  fillList($('rv-sentences'), a.sentences);
  const same = a.best.move.text === a.played.move.text;
  say(`${k + 1}. ${who} ${text} · ${a.grade_ko}` + (same ? '' : ` (주황 = 둔 수, 초록 = 최선 수 ${a.best.move.text})`), a.grade === 'mistake' || a.grade === 'blunder' ? 'warn' : 'info');
  const rows = a.contributions
    ? Object.entries(a.contributions)
        .filter(([, v]) => v !== 0)
        .sort((x, y) => y[1] - x[1])
        .map(([key, v]) => `<tr><td>${FEATURE_LABELS[key] || key}</td><td>${signed(v)}</td></tr>`)
        .join('')
    : '';
  $('rv-engine').innerHTML =
    `<div>등급 ${a.grade_ko} · 손실 ${a.loss}점 · 탐색 깊이 ${a.depth}</div>` +
    `<div>평가(흑 기준): 둔 수 ${signed(a.played.score_black)} / 최선 수 ${signed(a.best.score_black)}</div>` +
    `<div>둔 수 수순: <span class="pv">${a.played.pv.join(' ')}</span></div>` +
    (same ? '' : `<div>최선 수순: <span class="pv">${a.best.pv.join(' ')}</span></div>`) +
    (rows ? `<div>기여도 차이 (최선 − 둔 수, ${who} 기준. 합 = 손실)</div><table>${rows}</table>` : '');
}

// ---------------------------------------------------------------- 기보 (6단계)

const LAYOUT_KO = { belgian: '벨지안 데이지', standard: '표준', german: '저먼 데이지' };
const LEVEL_KO = { random: '아주 쉬움', easy: '쉬움', normal: '보통', hard: '어려움', expert: '아주 어려움' };

const RECORD_ERR = {
  unknown_layout: (r) => `배치 이름 "${r.value}"을 모르겠어요. belgian, standard, german 중 하나로 적어 주세요.`,
  bad_move_limit: (r) => `수 제한 "${r.value}"이 숫자가 아니에요.`,
  bad_token: (r) => `${r.index + 1}번째 수 "${r.token}"를 칸 이름으로 읽을 수 없어요.`,
  illegal: (r) => `${r.index + 1}번째 수 "${r.token}"는 그 자리에서 둘 수 없는 수예요.`,
  ambiguous: (r) => `${r.index + 1}번째 수 "${r.token}"는 두 가지 이상으로 읽혀서 정할 수 없어요.`,
  after_end: (r) => `${r.index}수 만에 대국이 끝났는데 뒤에 수("${r.token}")가 더 있어요.`,
  empty: () => '기보에 수가 없어요.',
};

const today = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};
const playerName = (color) => (ui.human === 'both' || ui.human === color ? '사람' : `컴퓨터(${LEVEL_KO[$('level').value]})`);

function setRecordMsg(text, kind = '') {
  const p = $('record-msg');
  p.textContent = text;
  p.className = `record-msg ${kind}`;
}

async function openRecord() {
  const tag = (k) => {
    const t = ui.loadedTags && ui.loadedTags.find(([n]) => n.toLowerCase() === k.toLowerCase());
    return t ? t[1] : undefined;
  };
  const r = await call('exportRecord', {
    black: tag('Black') ?? playerName('black'),
    white: tag('White') ?? playerName('white'),
    date: tag('Date') ?? today(),
  });
  $('record-text').value = r.text;
  setRecordMsg('');
  $('record').showModal();
}

async function loadRecord() {
  ui.aiToken++; // 진행 중인 컴퓨터 수 취소
  ui.busy = true;
  let res;
  try {
    res = await call('loadRecord', { text: $('record-text').value });
  } finally {
    ui.busy = false;
  }
  const r = res.result;
  if (!r.ok) {
    setRecordMsg(RECORD_ERR[r.error] ? RECORD_ERR[r.error](r) : r.error, 'warn');
    maybeAi(); // 대국은 그대로다
    return;
  }
  ui.loadedTags = r.tags;
  ui.counted = true;
  ui.lastSearch = null;
  ui.grades = {};
  ui.review = { cache: {}, cur: -1, token: ui.review.token + 1, board: null };
  ui.mode = 'play';
  $('feedback').hidden = true;
  $('layout').value = res.state.layout;
  $('limit').value = res.state.move_limit;
  setState(res);
  render();
  $('record').close();
  maybeAi();
  const conv = r.converted ? ` 다른 표기로 적힌 수 ${r.converted}개는 이 사이트 표기로 바꿨어요.` : '';
  say(
    res.state.outcome.status === 'ongoing'
      ? `기보를 불러왔어요(${r.plies}수). 이어서 둘 수 있어요.${conv}`
      : `기보를 불러왔어요(${r.plies}수, 끝난 대국). "복기하기"를 누르면 수마다 설명을 볼 수 있어요.${conv}`,
    'good',
  );
}

function saveRecordFile() {
  const blob = new Blob([$('record-text').value], { type: 'text/plain;charset=utf-8' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `abalone-${today()}.txt`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  setRecordMsg('파일로 저장했어요.', 'good');
}

async function copyRecord() {
  try {
    await navigator.clipboard.writeText($('record-text').value);
    setRecordMsg('복사했어요.', 'good');
  } catch {
    $('record-text').select();
    setRecordMsg('자동 복사가 막혀 있어요. 선택된 글자를 직접 복사해 주세요.', 'warn');
  }
}

// ---------------------------------------------------------------- 배치별 승률 (6단계)

const MY_KEY = 'abalone.myStats.v1';
const readMine = () => {
  try {
    return JSON.parse(localStorage.getItem(MY_KEY)) || {};
  } catch {
    return {};
  }
};
const writeMine = (m) => {
  try {
    localStorage.setItem(MY_KEY, JSON.stringify(m));
  } catch {
    // 저장이 막힌 브라우저(사생활 보호 창 등)에서는 기록하지 않는다
  }
};

// 컴퓨터와 둔 대국이 끝나면 한 번만 내 기록에 넣는다. 끝난 뒤 무르고 다시 둬도 처음 결과만 남는다.
function noteResult() {
  const st = ui.state;
  if (ui.counted || ui.human === 'both' || st.outcome.status === 'ongoing') return;
  ui.counted = true;
  const key = `${st.layout}|${$('level').value}`;
  const m = readMine();
  const e = m[key] || { w: 0, d: 0, l: 0 };
  if (st.outcome.status === 'draw') e.d++;
  else if (st.outcome.winner === ui.human) e.w++;
  else e.l++;
  m[key] = e;
  writeMine(m);
  renderMine();
}

function renderMine() {
  const box = $('stats-mine');
  const m = readMine();
  const keys = Object.keys(m).filter((k) => m[k].w + m[k].d + m[k].l > 0);
  const order = (k) => {
    const [l, v] = k.split('|');
    return Object.keys(LAYOUT_KO).indexOf(l) * 10 + Object.keys(LEVEL_KO).indexOf(v);
  };
  keys.sort((a, b) => order(a) - order(b));
  $('stats-reset').hidden = keys.length === 0;
  if (!keys.length) {
    box.innerHTML = '<p class="muted">아직 컴퓨터와 끝까지 둔 대국이 없어요.</p>';
    return;
  }
  const rows = keys.map((k) => {
    const [l, v] = k.split('|');
    const e = m[k];
    const n = e.w + e.d + e.l;
    return `<tr><td>${LAYOUT_KO[l] || l}</td><td>${LEVEL_KO[v] || v}</td><td>${e.w}</td><td>${e.d}</td><td>${e.l}</td><td>${Math.round(((e.w + e.d / 2) / n) * 100)}%</td></tr>`;
  });
  box.innerHTML = `<table><tr><th>배치</th><th>컴퓨터</th><th>승</th><th>무</th><th>패</th><th>승률</th></tr>${rows.join('')}</table>`;
}

// 흑 점수율의 95% 구간이 50%를 벗어날 때만 "유리해 보여요"라고 말한다
function advantageText(t) {
  const n = t.black + t.draw + t.white;
  const s = (t.black + t.draw / 2) / n;
  const se = Math.max(Math.sqrt((s * (1 - s)) / n), 0.5 / n);
  if (s - 1.96 * se > 0.5) return '먼저 두는 흑이 유리해 보여요';
  if (s + 1.96 * se < 0.5) return '나중에 두는 백이 유리해 보여요';
  return '흑백 차이가 뚜렷하지 않아요';
}

async function renderEngineStats() {
  let data;
  try {
    const r = await fetch('data/layout_stats.json');
    if (!r.ok) throw new Error(r.status);
    data = await r.json();
  } catch {
    $('stats-note').textContent = '통계 파일을 불러오지 못했어요.';
    return;
  }
  $('stats-note').textContent = data.note;
  const pct = (x, n) => Math.round((x / n) * 100);
  const box = $('stats-engine');
  box.replaceChildren();
  for (const run of data.runs) {
    const h = document.createElement('p');
    h.className = 'st-run';
    h.textContent = run.label;
    box.appendChild(h);
    for (const [layout, t] of Object.entries(run.layouts)) {
      const n = t.games;
      const [b, d, w] = [pct(t.black, n), pct(t.draw, n), pct(t.white, n)];
      const off = t.ends.pushed_off || 0;
      const row = document.createElement('div');
      row.className = 'st-row';
      row.innerHTML =
        `<div class="st-head"><b>${LAYOUT_KO[layout]}</b><span>${n}판</span></div>` +
        `<div class="st-bar" role="img" aria-label="흑 승 ${b}%, 무승부 ${d}%, 백 승 ${w}%">` +
        `<span class="b" style="flex:${t.black}">${b >= 12 ? `흑 ${b}%` : ''}</span>` +
        `<span class="d" style="flex:${t.draw}">${d >= 12 ? `무 ${d}%` : ''}</span>` +
        `<span class="w" style="flex:${t.white}">${w >= 12 ? `백 ${w}%` : ''}</span></div>` +
        `<div class="st-sub">흑 ${t.black}승 · 무 ${t.draw} · 백 ${t.white}승 · ${advantageText(t)} · 평균 ${t.avg_plies}수 · 6개 밀어내기로 끝난 판 ${pct(off, n)}%</div>`;
      box.appendChild(row);
    }
  }
}

$('record-btn').addEventListener('click', openRecord);
$('record-close').addEventListener('click', () => $('record').close());
$('record-save').addEventListener('click', saveRecordFile);
$('record-copy').addEventListener('click', copyRecord);
$('record-load').addEventListener('click', loadRecord);
$('record-file').addEventListener('change', async (e) => {
  const f = e.target.files[0];
  e.target.value = '';
  if (!f) return;
  $('record-text').value = await f.text();
  await loadRecord();
});
$('stats-reset').addEventListener('click', () => {
  if (!confirm('이 기기에 저장된 내 기록을 모두 지울까요?')) return;
  writeMine({});
  renderMine();
});

// ---------------------------------------------------------------- 조작 방식

const HELP = {
  swipe: [
    '움직일 내 구슬을 손가락(마우스)으로 쓱 쓸어서 골라요. 한 줄로 붙은 구슬을 3개까지 고를 수 있어요.',
    '하나만 움직이려면 그 구슬을 톡 눌러요. 다시 누르면 선택이 풀려요.',
    '손을 떼면 갈 수 있는 쪽에 큰 화살표가 나와요. 화살표를 누르면 고른 구슬이 같이 움직여요.',
  ],
  tap: [
    '움직일 내 구슬을 하나씩 눌러요. 누를 때마다 골라지고, 고른 구슬을 다시 누르면 빠져요.',
    '한 줄로 붙은 구슬을 3개까지 고를 수 있어요. 줄이 안 되는 구슬을 누르면 그 구슬부터 새로 골라요.',
    '갈 수 있는 칸에 점이 찍혀요. 그 칸을 누르면 고른 구슬이 같이 움직여요.',
  ],
  ends: [
    '움직일 줄의 첫 구슬을 눌러요. 줄 방향으로 갈 때는 맨 뒤 구슬, 옆으로 갈 때는 한쪽 끝 구슬이에요.',
    '그다음 끝점을 눌러요. 줄 방향이면 맨 앞 구슬이 갈 칸, 옆으로 가면 반대쪽 끝 구슬이 갈 칸이에요. 끝점이 될 수 있는 칸에 점이 찍혀요.',
    '같은 두 칸으로 여러 수가 되면 줄 방향 수, 그다음 구슬이 적은 수로 둬요. 다른 수를 두고 싶으면 반대쪽 끝 구슬부터 눌러요.',
    '첫 구슬을 다시 누르면 처음부터예요.',
  ],
};
const HELP_COMMON = [
  '상대 구슬보다 내 구슬이 더 많으면 줄 방향으로 밀 수 있어요. 판 밖으로 6개 밀어내면 이겨요.',
  '작은 회색 화살표를 누르면 왜 못 가는지 알려 줘요.',
];
function renderHelp() {
  const ul = $('help-list');
  ul.replaceChildren();
  for (const t of [...HELP[control()], ...HELP_COMMON]) {
    const li = document.createElement('li');
    li.textContent = t;
    ul.appendChild(li);
  }
}

const CONTROL_KEY = 'abalone.control';
try {
  const saved = localStorage.getItem(CONTROL_KEY);
  if (saved && HELP[saved]) $('control').value = saved;
} catch {
  // 저장이 막힌 브라우저에서는 기본값(쓸기)
}
$('control').addEventListener('change', () => {
  try {
    localStorage.setItem(CONTROL_KEY, control());
  } catch {
    // 무시
  }
  renderHelp();
  select([]);
});

$('setup').addEventListener('submit', (e) => {
  e.preventDefault();
  newGame();
});
$('undo').addEventListener('click', undo);
$('hint').addEventListener('click', showHint);
$('review-btn').addEventListener('click', enterReview);
$('rv-exit').addEventListener('click', exitReview);
$('rv-prev').addEventListener('click', () => selectReview(ui.review.cur - 1));
$('rv-next').addEventListener('click', () => selectReview(ui.review.cur + 1));
$('friendly').addEventListener('change', () => {
  if (!friendly()) $('feedback').hidden = true;
  ui.hint = null;
  render();
});
$('level').addEventListener('change', () => ui.state && renderPanel(ui.state)); // 다음 컴퓨터 수부터 적용
$('clear').addEventListener('click', () => select([]));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') select([]);
});

buildBoard();
renderHelp();
renderMine();
renderEngineStats();
newGame();
