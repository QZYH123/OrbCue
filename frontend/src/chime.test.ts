import { describe, expect, it } from 'vitest';
import { shouldPlayChime } from './chime';

const allOn = { completion: true, attention: true, failure: true };
const allOff = { completion: false, attention: false, failure: false };

describe('shouldPlayChime', () => {
  it('respects each settings channel', () => {
    expect(shouldPlayChime('info', allOn)).toBe(true);
    expect(shouldPlayChime('attention', allOn)).toBe(true);
    expect(shouldPlayChime('error', allOn)).toBe(true);
    expect(shouldPlayChime('info', allOff)).toBe(false);
    expect(shouldPlayChime('attention', allOff)).toBe(false);
    expect(shouldPlayChime('error', allOff)).toBe(false);
  });

  it('maps completion to info, waiting to attention, and failure to error', () => {
    expect(shouldPlayChime('info', { ...allOff, completion: true })).toBe(true);
    expect(shouldPlayChime('attention', { ...allOff, attention: true })).toBe(true);
    expect(shouldPlayChime('error', { ...allOff, failure: true })).toBe(true);
  });
});
