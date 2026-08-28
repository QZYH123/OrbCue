import { describe, expect, it } from 'vitest';
import {
  clampToWorkArea,
  dockHitPx,
  dockPeekPx,
  dockSnapPx,
  dockedPosition,
  edgeExpandedPosition,
  fullyInWorkArea,
  nearestWorkAreaEdge,
  panelPositionNearBall,
  shouldHidePanelOnBallDrag,
  shouldSnapToEdge,
} from './placement';

const workArea = { x: 0, y: 0, width: 1000, height: 800 };

describe('clampToWorkArea', () => {
  it('clamps past the right edge to max x', () => {
    expect(clampToWorkArea({ x: 960, y: 40, width: 80, height: 80 }, workArea)).toEqual({
      x: 920,
      y: 40,
    });
  });

  it('clamps past the top edge to min y', () => {
    expect(clampToWorkArea({ x: 40, y: -30, width: 80, height: 80 }, workArea)).toEqual({
      x: 40,
      y: 0,
    });
  });

  it('leaves a window already inside the work area unchanged', () => {
    expect(clampToWorkArea({ x: 120, y: 160, width: 80, height: 80 }, workArea)).toEqual({
      x: 120,
      y: 160,
    });
  });
});

describe('panelPositionNearBall', () => {
  const panel = { width: 420, height: 580 };

  it('opens the panel to the right when the ball is in the left half', () => {
    const ball = { x: 40, y: 80, width: 112, height: 112 };
    const position = panelPositionNearBall({ ball, panel, workArea, gap: 12 });
    expect(position.x).toBeGreaterThan(ball.x);
    expect(position.x).toBe(40 + 112 + 12);
    expect(position.y).toBe(80);
  });

  it('opens the panel to the left when the ball is in the right half', () => {
    const ball = { x: 820, y: 90, width: 112, height: 112 };
    const position = panelPositionNearBall({ ball, panel, workArea, gap: 12 });
    expect(position.x).toBeLessThan(ball.x);
    expect(position.x).toBe(820 - 420 - 12);
  });

  it('keeps the panel on the same work area as the ball', () => {
    const ball = { x: 900, y: 700, width: 112, height: 112 };
    const position = panelPositionNearBall({ ball, panel, workArea, gap: 12 });
    expect(position.x + panel.width).toBeLessThanOrEqual(workArea.x + workArea.width);
    expect(position.y + panel.height).toBeLessThanOrEqual(workArea.y + workArea.height);
  });
});

describe('panel follow', () => {
  it('hides the panel when the ball drag starts so it cannot stay behind', () => {
    expect(shouldHidePanelOnBallDrag(true)).toBe(true);
    expect(shouldHidePanelOnBallDrag(false)).toBe(false);
  });
});

describe('side dock', () => {
  const ball = { x: 40, y: 200, width: 56, height: 56 };

  it('picks the nearest work-area edge', () => {
    expect(nearestWorkAreaEdge(ball, workArea)).toBe('left');
    expect(nearestWorkAreaEdge({ ...ball, x: 920 }, workArea)).toBe('right');
    expect(nearestWorkAreaEdge({ ...ball, x: 400, y: 10 }, workArea)).toBe('top');
    expect(nearestWorkAreaEdge({ ...ball, x: 400, y: 740 }, workArea)).toBe('bottom');
  });

  it('uses about one ball-width as the snap magnet', () => {
    expect(dockSnapPx(ball)).toBe(56);
    expect(dockSnapPx({ width: 112, height: 112 })).toBe(112);
  });

  it('snaps when the ball is within one ball-width of an edge', () => {
    const snap = dockSnapPx(ball);
    expect(shouldSnapToEdge({ ...ball, x: 200 }, workArea, snap)).toBe(false);
    expect(shouldSnapToEdge(ball, workArea, snap)).toBe(true);
    expect(shouldSnapToEdge({ ...ball, x: 8 }, workArea, snap)).toBe(true);
  });

  it('tucks to a semicircle on the work-area edge', () => {
    const peek = dockPeekPx(ball);
    expect(peek).toBe(28);
    expect(dockHitPx(ball)).toBe(peek);
    const docked = dockedPosition(ball, workArea, 'left', peek);
    expect(docked.x).toBe(0 - (56 - peek));
    expect(docked.y).toBe(200);
    expect(docked.x + 56).toBe(peek);
  });

  it('tucks past the right edge', () => {
    const peek = 24;
    const docked = dockedPosition({ ...ball, x: 900 }, workArea, 'right', peek);
    expect(docked.x).toBe(1000 - peek);
  });

  it('does not treat a clamped-to-edge window as tucked', () => {
    const onLeftEdge = { x: 0, y: 200, width: 56, height: 56 };
    expect(fullyInWorkArea(onLeftEdge, workArea)).toBe(true);
    expect(fullyInWorkArea({ ...onLeftEdge, ...dockedPosition(onLeftEdge, workArea, 'left', 28) }, workArea)).toBe(
      false,
    );
  });

  it('uses the work-area origin, not screen (0, 0)', () => {
    const sidebar = { x: 62, y: 0, width: 1858, height: 1080 };
    const sittingOnWorkLeft = { x: 62, y: 200, width: 56, height: 56 };
    expect(fullyInWorkArea(sittingOnWorkLeft, sidebar)).toBe(true);
    expect(fullyInWorkArea({ x: 0, y: 200, width: 56, height: 56 }, sidebar)).toBe(false);
    expect(fullyInWorkArea({ x: 34, y: 200, width: 56, height: 56 }, sidebar)).toBe(false);

    const secondScreen = { x: 1920, y: 0, width: 1920, height: 1080 };
    const sittingOnSecondLeft = { x: 1920, y: 80, width: 56, height: 56 };
    expect(fullyInWorkArea(sittingOnSecondLeft, secondScreen)).toBe(true);
    const tucked = dockedPosition(sittingOnSecondLeft, secondScreen, 'left', 28);
    expect(fullyInWorkArea({ ...sittingOnSecondLeft, ...tucked }, secondScreen)).toBe(false);
  });

  it('slides out along the same edge so the peek stays under the pointer', () => {
    const peek = dockPeekPx(ball);
    const dockedRight = dockedPosition({ ...ball, x: 900 }, workArea, 'right', peek);
    expect(edgeExpandedPosition({ ...ball, ...dockedRight }, workArea, 'right')).toEqual({
      x: 1000 - 56,
      y: 200,
    });
    const dockedLeft = dockedPosition(ball, workArea, 'left', peek);
    expect(edgeExpandedPosition({ ...ball, ...dockedLeft }, workArea, 'left')).toEqual({
      x: 0,
      y: 200,
    });
  });
});
