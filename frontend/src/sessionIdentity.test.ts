import { describe, expect, it } from 'vitest';
import {
  auditProjectLabel,
  displayAgent,
  folderName,
  filterSessionSections,
  formatAuditTime,
  isAuditVisible,
  presentAuditRows,
  presentSessionSections,
  sessionDomKey,
} from './sessionIdentity';

describe('folderName', () => {
  it('returns the last path segment', () => {
    expect(folderName('/home/qingz/projects/dock')).toBe('dock');
    expect(folderName('C:\\Users\\qingz\\work\\repo\\')).toBe('repo');
  });

  it('does not invent a name for empty paths', () => {
    expect(folderName(null)).toBeNull();
    expect(folderName('')).toBeNull();
    expect(folderName('///')).toBeNull();
  });
});

describe('displayAgent', () => {
  it('pretty-prints known agents and title-cases the rest', () => {
    expect(displayAgent('claude')).toBe('Claude');
    expect(displayAgent('GROK')).toBe('Grok');
    expect(displayAgent('cursor')).toBe('Cursor');
    expect(displayAgent('my-bot')).toBe('My-bot');
    expect(displayAgent('')).toBe('Agent');
  });
});

describe('sessionDomKey', () => {
  it('keeps two resumes of the same session distinct', () => {
    const first = { source: 'grok', session_id: 'resume-id', terminal_id: 'term-a' };
    const second = { source: 'grok', session_id: 'resume-id', terminal_id: 'term-b' };
    expect(sessionDomKey(first)).not.toBe(sessionDomKey(second));
  });
});

describe('formatAuditTime', () => {
  it('uses a compact local timestamp without seconds', () => {
    expect(formatAuditTime('2026-08-22T10:04:00')).toMatch(/^\d{1,2}\/\d{1,2} \d{2}:\d{2}$/);
    expect(formatAuditTime('2026-08-22T10:04:00')).not.toMatch(/,/);
    expect(formatAuditTime('not-a-date')).toBe('not-a-date');
  });
});

describe('presentAuditRows', () => {
  it('hides working and idle, and reuses live agent numbers', () => {
    const rows = presentAuditRows(
      [
        {
          source: 'claude',
          session_id: 'live-1',
          state: 'working',
          attention_reason: null,
          occurred_at: '2026-08-24T10:00:00Z',
          project_path: '/proj/dock',
        },
        {
          source: 'claude',
          session_id: 'live-1',
          state: 'completed',
          attention_reason: null,
          occurred_at: '2026-08-24T10:01:00Z',
          project_path: '/proj/dock',
        },
        {
          source: 'grok',
          session_id: 'gone',
          state: 'closed',
          attention_reason: null,
          occurred_at: '2026-08-24T10:02:00Z',
          project_path: '/proj/dock',
        },
      ],
      [
        {
          source: 'claude',
          session_id: 'live-1',
          project_path: '/proj/dock',
        },
      ],
    );
    expect(rows.map((row) => `${row.title} ${row.index ?? '—'} ${row.entry.state}`)).toEqual([
      'Grok — closed',
      'Claude 01 completed',
    ]);
    expect(rows[0]?.project).toBe('dock');
  });

  it('treats working and idle as not visible', () => {
    expect(isAuditVisible('working')).toBe(false);
    expect(isAuditVisible('idle')).toBe(false);
    expect(isAuditVisible('completed')).toBe(true);
    expect(isAuditVisible('failed')).toBe(true);
  });
});

describe('auditProjectLabel', () => {
  it('shows the project folder when the path is present', () => {
    expect(auditProjectLabel({ project_path: '/home/qingz/projects/agent-activity-dock/' })).toBe(
      'agent-activity-dock',
    );
    expect(auditProjectLabel({ project_path: null })).toBeNull();
  });
});

describe('presentSessionSections', () => {
  it('uses the project folder as the section and the agent as the row title', () => {
    const sections = presentSessionSections(
      [
        {
          source: 'claude',
          session_id: 'one',
          project_path: '/home/qingz/projects/dock',
        },
        {
          source: 'grok',
          session_id: 'two',
          project_path: '/home/qingz/projects/dock',
        },
      ],
      '/home/qingz',
    );
    expect(sections).toHaveLength(1);
    expect(sections[0]?.label).toBe('dock');
    expect(sections[0]?.rows.map((row) => `${row.title} ${row.index}`)).toEqual([
      'Claude 01',
      'Grok 01',
    ]);
  });

  it('numbers two of the same agent in one project 01, 02', () => {
    const sections = presentSessionSections([
      {
        source: 'claude',
        session_id: 'aaa-111',
        project_path: '/proj/dock',
        terminal_id: 'orb:ab12cd',
      },
      {
        source: 'claude',
        session_id: 'bbb-222',
        project_path: '/proj/dock',
        terminal_id: 'orb:ff00aa',
      },
    ]);
    expect(sections[0]?.rows.map((row) => `${row.title} ${row.index}`)).toEqual([
      'Claude 01',
      'Claude 02',
    ]);
  });

  it('keeps colliding folder names distinguishable via the shortened path', () => {
    const sections = presentSessionSections(
      [
        { source: 'claude', session_id: 'a', project_path: '/home/qingz/a/dock' },
        { source: 'grok', session_id: 'b', project_path: '/home/qingz/b/dock' },
      ],
      '/home/qingz',
    );
    expect(sections.map((section) => section.label)).toEqual(['~/a/dock', '~/b/dock']);
  });

  it('still uses the agent name when there is no project', () => {
    const sections = presentSessionSections([
      {
        source: 'cursor',
        session_id: 'x1',
        project_path: null,
        window_title: 'Windows Terminal - notes',
      },
    ]);
    expect(sections[0]?.label).toBe('其他');
    expect(sections[0]?.rows[0]?.title).toBe('Cursor');
    expect(sections[0]?.rows[0]?.index).toBe('01');
  });

  it('keeps the original index when a filter hides the sibling', () => {
    const claudeA = {
      source: 'claude',
      session_id: 'aaa',
      project_path: '/proj/dock',
    };
    const claudeB = {
      source: 'claude',
      session_id: 'bbb',
      project_path: '/proj/dock',
    };
    const sections = presentSessionSections([claudeA, claudeB]);
    const filtered = filterSessionSections(sections, [claudeB]);
    expect(filtered[0]?.rows).toHaveLength(1);
    expect(filtered[0]?.rows[0]?.index).toBe('02');
  });
});
