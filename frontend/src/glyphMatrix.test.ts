import { describe, expect, it } from 'vitest';
import { barTones, matrixTones } from './glyphMatrix';

describe('glyph matrix', () => {
  it('fills a working bar then dim tracked remainder', () => {
    expect(barTones(2, 5, 8)).toEqual(['lit', 'lit', 'dim', 'dim', 'dim', 'ghost', 'ghost', 'ghost']);
  });

  it('always returns 121 cells and never draws a digit block', () => {
    const tones = matrixTones(2, 5, 'working');
    expect(tones).toHaveLength(121);
    expect(tones.some((tone) => tone === 'void')).toBe(true);
    expect(tones.filter((tone) => tone === 'lit').length).toBeGreaterThan(0);
  });
});
