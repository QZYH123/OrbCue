import { describe, expect, it } from 'vitest';
import {
  auditAttentionNote,
  auditProjectLabel,
  cleanWindowTitle,
  displayAgent,
  folderName,
  filterSessionSections,
  formatAuditTime,
  presentSessionSections,
  sessionDetail,
  shortSessionId,
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
    expect(displayAgent('dsh')).toBe('DSH');
    expect(displayAgent('my-bot')).toBe('My-bot');
    expect(displayAgent('')).toBe('Agent');
  });
});

describe('shortSessionId', () => {
  it('keeps short ids and shortens UUIDs and long names', () => {
    expect(shortSessionId('task-1')).toBe('task-1');
    expect(shortSessionId('a1b2c3d4-e5f6-7890-abcd-ef1234567890')).toBe('a1b2c3d4');
    expect(shortSessionId('very-long-custom-session-name')).toBe('very-lon…');
  });
});

describe('auditAttentionNote', () => {
  it('only labels waiting-for-user on needs_attention rows', () => {
    expect(
      auditAttentionNote({ state: 'completed', attention_reason: 'completed' }),
    ).toBeNull();
    expect(auditAttentionNote({ state: 'failed', attention_reason: 'failed' })).toBeNull();
    expect(auditAttentionNote({ state: 'working', attention_reason: null })).toBeNull();
    expect(
      auditAttentionNote({ state: 'needs_attention', attention_reason: 'input' }),
    ).toBe('需要输入');
    expect(
      auditAttentionNote({ state: 'needs_attention', attention_reason: 'permission' }),
    ).toBe('授权请求');
  });
});

describe('formatAuditTime', () => {
  it('uses a compact local timestamp instead of a locale long form', () => {
    expect(formatAuditTime('2026-08-22T10:04:00.000Z')).toMatch(
      /^\d{1,2}\/\d{1,2} \d{2}:\d{2}:\d{2}$/,
    );
    expect(formatAuditTime('2026-08-22T10:04:00.000Z')).not.toMatch(/,/);
    expect(formatAuditTime('not-a-date')).toBe('not-a-date');
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

describe('cleanWindowTitle', () => {
  it('strips terminal chrome and dock markers', () => {
    expect(cleanWindowTitle('Windows Terminal - dock:ab12cd · grok · dock')).toBe(
      'grok · dock',
    );
    expect(cleanWindowTitle('dock:ff00aa · claude · app')).toBe('claude · app');
    expect(
      cleanWindowTitle('Windows Terminal - agent-activity-dock · grok · dock:ab12cd'),
    ).toBe('agent-activity-dock · grok');
    expect(cleanWindowTitle('app · claude · dock:ff00aa')).toBe('app · claude');
    expect(cleanWindowTitle('just a title')).toBe('just a title');
  });
});

describe('sessionDetail', () => {
  it('prefers a dock tab suffix over the raw session id', () => {
    expect(
      sessionDetail({
        source: 'claude',
        session_id: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890',
        terminal_id: 'dock:ab12cd',
      }),
    ).toBe('ab12cd');
    expect(
      sessionDetail({
        source: 'claude',
        session_id: 'task-1',
        terminal_id: null,
      }),
    ).toBe('task-1');
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
        terminal_id: 'dock:ab12cd',
      },
      {
        source: 'claude',
        session_id: 'bbb-222',
        project_path: '/proj/dock',
        terminal_id: 'dock:ff00aa',
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
        source: 'dsh',
        session_id: 'x1',
        project_path: null,
        window_title: 'Windows Terminal - notes',
      },
    ]);
    expect(sections[0]?.label).toBe('其他');
    expect(sections[0]?.rows[0]?.title).toBe('DSH');
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
