import { describe, expect, it } from 'vitest';
import {
  highlightFromNotificationExtra,
  projectGroupKey,
  revealHighlightedGroup,
  sessionHighlightKey,
} from './highlight';

describe('highlightFromNotificationExtra', () => {
  it('parses source and session_id from toast extra', () => {
    expect(highlightFromNotificationExtra({ source: 'claude', session_id: 's1' })).toEqual({
      source: 'claude',
      session_id: 's1',
    });
    expect(sessionHighlightKey('claude', 's1')).toBe('claude\0s1');
  });

  it('ignores incomplete extra so a missing session only opens the panel', () => {
    expect(highlightFromNotificationExtra({ source: 'claude' })).toBeNull();
    expect(highlightFromNotificationExtra({})).toBeNull();
    expect(highlightFromNotificationExtra(undefined)).toBeNull();
  });
});

describe('revealHighlightedGroup', () => {
  it('opens a collapsed project so the highlighted card is visible', () => {
    expect(projectGroupKey('/home/qingz/dock')).toBe('/home/qingz/dock');
    expect(projectGroupKey(null)).toBe('');
    expect(
      revealHighlightedGroup({ '/home/qingz/dock': true, '/other': true }, '/home/qingz/dock'),
    ).toEqual({ '/home/qingz/dock': false, '/other': true });
  });

  it('leaves already open groups unchanged', () => {
    const open = { '/other': true };
    expect(revealHighlightedGroup(open, '/home/qingz/dock')).toBe(open);
  });
});
