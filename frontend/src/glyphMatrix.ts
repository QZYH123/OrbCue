const MATRIX_N = 11;
const MATRIX_CX = 5;
const MATRIX_DISC2 = 29;
const MATRIX_BADGE = [1 * MATRIX_N + 8, 2 * MATRIX_N + 9];

function discCell(r: number, c: number) {
  const dr = r - MATRIX_CX;
  const dc = c - MATRIX_CX;
  return dr * dr + dc * dc <= MATRIX_DISC2;
}

const MATRIX_RING: number[] = [];
{
  const cells: { i: number; r: number; c: number }[] = [];
  for (let r = 0; r < MATRIX_N; r++) {
    for (let c = 0; c < MATRIX_N; c++) {
      if (!discCell(r, c)) continue;
      if (discCell(r - 1, c) && discCell(r + 1, c) && discCell(r, c - 1) && discCell(r, c + 1)) continue;
      cells.push({ i: r * MATRIX_N + c, r, c });
    }
  }
  cells.sort(
    (a, b) =>
      Math.atan2(a.r - MATRIX_CX, a.c - MATRIX_CX) - Math.atan2(b.r - MATRIX_CX, b.c - MATRIX_CX),
  );
  const top = cells.findIndex((cell) => cell.r === 0 && cell.c === 5);
  const ordered = top > 0 ? [...cells.slice(top), ...cells.slice(0, top)] : cells;
  for (const cell of ordered) MATRIX_RING.push(cell.i);
}

function markDot(mark: string) {
  if (mark === '!') return 'mark-fail';
  if (mark === '?') return 'mark-wait';
  if (mark === '*') return 'mark-done';
  if (mark === 'x') return 'mark-cancel';
  return mark ? 'mark-idle' : '';
}

export function barTones(working: number, tracked: number, n: number) {
  const t = Math.min(Math.max(tracked, 0), n);
  const w = Math.min(Math.max(working, 0), t);
  const tones: string[] = [];
  for (let i = 0; i < n; i++) tones.push(i < w ? 'lit' : i < t ? 'dim' : 'ghost');
  return tones;
}

/** Ring + attention texture only. Digits are overlaid as real numbers on the ball. */
export function matrixTones(working: number, tracked: number, kind: string, mark: string) {
  const ringAt = new Map(MATRIX_RING.map((i, ord) => [i, ord]));
  const trackedDots = Math.min(Math.max(tracked, 0), MATRIX_RING.length);
  const workingDots = Math.min(Math.max(working, 0), trackedDots);
  const markTone = markDot(mark);
  const tones: string[] = [];
  for (let i = 0; i < MATRIX_N * MATRIX_N; i++) {
    const r = Math.floor(i / MATRIX_N);
    const c = i % MATRIX_N;
    if (!discCell(r, c)) {
      tones.push('void');
      continue;
    }
    if (markTone && MATRIX_BADGE.includes(i) && kind !== 'wait') {
      tones.push(markTone);
      continue;
    }
    if (kind === 'wait') {
      tones.push('wait');
      continue;
    }
    if (kind === 'fail') {
      tones.push(r === MATRIX_CX || c === MATRIX_CX ? 'fail' : 'off');
      continue;
    }
    const ringOrd = ringAt.get(i);
    if (ringOrd !== undefined) {
      tones.push(ringOrd < workingDots ? 'lit' : ringOrd < trackedDots ? 'dim' : 'ghost');
      continue;
    }
    tones.push('off');
  }
  return tones;
}
