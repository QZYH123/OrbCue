import { describe, expect, it } from 'vitest';
import {
  clampToWorkArea,
  panelFollowStrategy,
  panelPositionNearBall,
  shouldHidePanelOnBallDrag,
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
    expect(panelFollowStrategy()).toBe('hide-on-drag');
    expect(shouldHidePanelOnBallDrag(true)).toBe(true);
    expect(shouldHidePanelOnBallDrag(false)).toBe(false);
  });
});
