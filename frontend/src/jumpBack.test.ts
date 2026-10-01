import { describe, expect, it } from 'vitest';
import {
  CONNECTIONS_INTRO,
  EMPTY_TRACKING_HINT,
  isDesktopTerminalId,
  isDockTerminalId,
  JUMP_WINDOW_LEVEL,
  JUMP_WINDOW_MISSING,
  jumpFeedback,
  jumpPhrases,
} from './jumpBack';

describe('isDockTerminalId', () => {
  it('accepts dock markers and rejects tty ids', () => {
    expect(isDockTerminalId('orb:ab12cd')).toBe(true);
    expect(isDockTerminalId('orb:AB12CD')).toBe(true);
    expect(isDockTerminalId('orb:abc')).toBe(false);
    expect(isDockTerminalId('/dev/pts/3')).toBe(false);
    expect(isDockTerminalId(null)).toBe(false);
    expect(isDesktopTerminalId('app:claude:ab12')).toBe(true);
    expect(isDesktopTerminalId('app:cursor:')).toBe(false);
    expect(isDesktopTerminalId('orb:ab12cd')).toBe(false);
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

  it('labels a desktop-app hit separately from a terminal window', () => {
    expect(jumpFeedback({ focused: true, precise: false, app: true, reason: null })).toEqual({
      kind: 'note',
      text: '已把这个程序调到前面',
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

describe('orb run copy', () => {
  it('recommends orb run on empty activity and the connections page', () => {
    expect(EMPTY_TRACKING_HINT).toContain('orb run');
    expect(CONNECTIONS_INTRO).toContain('orb run');
    expect(JUMP_WINDOW_MISSING).toContain('orb run');
  });

  it('says Windows-only users can connect without WSL', () => {
    expect(CONNECTIONS_INTRO).toContain('没有 WSL');
  });

  it('keeps the same facts in English', () => {
    const english = jumpPhrases('en');
    expect(english.intro).toContain('orb run');
    expect(english.intro).toContain('Without WSL');
    expect(english.missing).toContain('orb run');
    expect(english.empty).toContain('orb run');
  });
});
