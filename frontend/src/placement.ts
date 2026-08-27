export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

export function clampToWorkArea(
  window: { x: number; y: number; width: number; height: number },
  workArea: Rect,
): Point {
  const minX = workArea.x;
  const minY = workArea.y;
  const maxX = workArea.x + workArea.width - window.width;
  const maxY = workArea.y + workArea.height - window.height;
  return {
    x: clamp(window.x, minX, Math.max(minX, maxX)),
    y: clamp(window.y, minY, Math.max(minY, maxY)),
  };
}

export function panelPositionNearBall(input: {
  ball: Rect;
  panel: { width: number; height: number };
  workArea: Rect;
  gap: number;
}): Point {
  const ballCenterX = input.ball.x + input.ball.width / 2;
  const workCenterX = input.workArea.x + input.workArea.width / 2;
  const x =
    ballCenterX < workCenterX
      ? input.ball.x + input.ball.width + input.gap
      : input.ball.x - input.panel.width - input.gap;
  return clampToWorkArea(
    {
      x,
      y: input.ball.y,
      width: input.panel.width,
      height: input.panel.height,
    },
    input.workArea,
  );
}

export function shouldHidePanelOnBallDrag(dragStarted: boolean): boolean {
  return dragStarted;
}

export type WorkAreaEdge = 'left' | 'right' | 'top' | 'bottom';

export function nearestWorkAreaEdge(
  window: { x: number; y: number; width: number; height: number },
  workArea: Rect,
): WorkAreaEdge {
  const distances: [WorkAreaEdge, number][] = [
    ['left', window.x - workArea.x],
    ['right', workArea.x + workArea.width - (window.x + window.width)],
    ['top', window.y - workArea.y],
    ['bottom', workArea.y + workArea.height - (window.y + window.height)],
  ];
  distances.sort((a, b) => a[1] - b[1]);
  return distances[0][0];
}

export function dockSnapPx(size: { width: number; height: number }): number {
  return Math.max(48, Math.round(Math.min(size.width, size.height)));
}

export function dockPeekPx(size: { width: number; height: number }): number {
  return Math.max(24, Math.round(Math.min(size.width, size.height) * 0.5));
}

/** On-screen amount when docked. Same as the visual semicircle. */
export function dockHitPx(size: { width: number; height: number }): number {
  return dockPeekPx(size);
}

export function shouldSnapToEdge(
  window: { x: number; y: number; width: number; height: number },
  workArea: Rect,
  snapPx: number,
): boolean {
  const edge = nearestWorkAreaEdge(window, workArea);
  return edgeGap(window, workArea, edge) <= snapPx;
}

/** Fully on-screen, still hugging the same work-area edge the ball peeked from. */
export function edgeExpandedPosition(
  window: { x: number; y: number; width: number; height: number },
  workArea: Rect,
  edge: WorkAreaEdge,
): Point {
  let x = window.x;
  let y = window.y;
  if (edge === 'left') x = workArea.x;
  else if (edge === 'right') x = workArea.x + workArea.width - window.width;
  else if (edge === 'top') y = workArea.y;
  else y = workArea.y + workArea.height - window.height;
  return clampToWorkArea({ ...window, x, y }, workArea);
}

export function dockedPosition(
  window: { x: number; y: number; width: number; height: number },
  workArea: Rect,
  edge: WorkAreaEdge,
  peek: number,
): Point {
  const maxPeek = Math.min(window.width, window.height) - 8;
  const visible = clamp(peek, 8, Math.max(8, maxPeek));
  let x = window.x;
  let y = window.y;
  if (edge === 'left') x = workArea.x - (window.width - visible);
  else if (edge === 'right') x = workArea.x + workArea.width - visible;
  else if (edge === 'top') y = workArea.y - (window.height - visible);
  else y = workArea.y + workArea.height - visible;
  if (edge === 'left' || edge === 'right') {
    y = clamp(
      window.y,
      workArea.y,
      Math.max(workArea.y, workArea.y + workArea.height - window.height),
    );
  } else {
    x = clamp(
      window.x,
      workArea.x,
      Math.max(workArea.x, workArea.x + workArea.width - window.width),
    );
  }
  return { x, y };
}

function edgeGap(
  window: { x: number; y: number; width: number; height: number },
  workArea: Rect,
  edge: WorkAreaEdge,
): number {
  if (edge === 'left') return window.x - workArea.x;
  if (edge === 'right') return workArea.x + workArea.width - (window.x + window.width);
  if (edge === 'top') return window.y - workArea.y;
  return workArea.y + workArea.height - (window.y + window.height);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
