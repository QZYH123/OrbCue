import { describe, expect, it } from 'vitest';
import {
  isDockTerminalId,
  JUMP_WINDOW_LEVEL,
  JUMP_WINDOW_MISSING,
  jumpFeedback,
} from './jumpBack';

describe('isDockTerminalId', () => {
  it('accepts dock markers and rejects tty ids', () => {
    expect(isDockTerminalId('dock:ab12cd')).toBe(true);
    expect(isDockTerminalId('dock:AB12CD')).toBe(true);
    expect(isDockTerminalId('dock:abc')).toBe(false);
    expect(isDockTerminalId('/dev/pts/3')).toBe(false);
    expect(isDockTerminalId(null)).toBe(false);
  });
});

describe('jumpFeedback', () => {
  it('stays silent for a precise hit', () => {
    expect(jumpFeedback({ focused: true, precise: true, reason: null })).toEqual({
      kind: 'silent',
      text: null,
    });
  });

  it('labels a captured-window hit as window-level', () => {
    expect(jumpFeedback({ focused: true, precise: false, reason: null })).toEqual({
      kind: 'note',
      text: JUMP_WINDOW_LEVEL,
    });
  });

  it('uses the honest missing-window copy when the backend sends none', () => {
    expect(jumpFeedback({ focused: false, precise: false, reason: null })).toEqual({
      kind: 'error',
      text: JUMP_WINDOW_MISSING,
    });
  });

  it('keeps a specific backend failure such as a closed tab', () => {
    expect(
      jumpFeedback({ focused: false, precise: false, reason: '该标签已关闭' }),
    ).toEqual({ kind: 'error', text: '该标签已关闭' });
  });
});
