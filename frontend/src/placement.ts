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

export function panelFollowStrategy(): 'hide-on-drag' {
  return 'hide-on-drag';
}

export function shouldHidePanelOnBallDrag(dragStarted: boolean): boolean {
  return dragStarted && panelFollowStrategy() === 'hide-on-drag';
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
