import { describe, expect, it } from 'vitest';
import { parseTheme, THEMES } from './theme';

describe('parseTheme', () => {
  it('keeps known theme ids', () => {
    for (const theme of THEMES) expect(parseTheme(theme)).toBe(theme);
  });

  it('falls back to prototype', () => {
    expect(parseTheme(null)).toBe('prototype');
    expect(parseTheme('')).toBe('prototype');
    expect(parseTheme('island')).toBe('prototype');
  });
});
