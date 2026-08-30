import { describe, expect, it } from 'vitest';
import {
  clampToWorkArea,
  dockHitPx,
  dockSnapPx,
  edgeExpandedPosition,
  nearestWorkAreaEdge,
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

  it('exposes half the ball as the docked hit size', () => {
    expect(dockHitPx(ball)).toBe(28);
  });

  it('slides out along the same edge so the peek stays under the pointer', () => {
    const peek = dockHitPx(ball);
    const dockedRight = { ...ball, x: 1000 - peek, y: 200 };
    expect(edgeExpandedPosition(dockedRight, workArea, 'right')).toEqual({
      x: 1000 - 56,
      y: 200,
    });
    const dockedLeft = { ...ball, x: 0 - (56 - peek), y: 200 };
    expect(edgeExpandedPosition(dockedLeft, workArea, 'left')).toEqual({
      x: 0,
      y: 200,
    });
  });
});
