import { describe, expect, it } from 'vitest';
import { physicalCircleRegion } from './circleRegion';

describe('physicalCircleRegion', () => {
  it('maps a 64 logical square to a full-window ellipse', () => {
    expect(physicalCircleRegion(64, 64)).toEqual({ left: 0, top: 0, right: 64, bottom: 64 });
  });

  it('maps a 1.5x DPI square to physical pixels', () => {
    expect(physicalCircleRegion(96, 96)).toEqual({ left: 0, top: 0, right: 96, bottom: 96 });
  });

  it('rejects zero or negative sizes so a bad region is never applied', () => {
    expect(physicalCircleRegion(0, 64)).toBeNull();
    expect(physicalCircleRegion(64, 0)).toBeNull();
    expect(physicalCircleRegion(-8, 64)).toBeNull();
  });
});
