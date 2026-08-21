import { describe, expect, it } from 'vitest';
import { groupSessionsByProject, shortenProjectPath } from './projectPath';

describe('shortenProjectPath', () => {
  it('folds a POSIX home prefix into a tilde', () => {
    expect(shortenProjectPath('/home/qingz/projects/dock', '/home/qingz')).toBe(
      '~/projects/dock',
    );
  });

  it('folds a Windows home prefix and keeps backslashes', () => {
    expect(shortenProjectPath('C:\\Users\\qingz\\projects\\dock', 'C:\\Users\\qingz')).toBe(
      '~\\projects\\dock',
    );
  });

  it('matches a Windows home when separators differ', () => {
    expect(shortenProjectPath('C:/Users/qingz/projects/dock', 'C:\\Users\\qingz')).toBe(
      '~/projects/dock',
    );
  });

  it('ellipsizes the middle of a long path and keeps the last segment', () => {
    const shortened = shortenProjectPath(
      '/home/qingz/very/long/directory/structure/that/exceeds/limit/project',
      '/home/qingz',
    );
    expect(shortened.startsWith('~')).toBe(true);
    expect(shortened.endsWith('/project')).toBe(true);
    expect(shortened.includes('…')).toBe(true);
    expect(shortened.length).toBeLessThanOrEqual(36);
  });

  it('does not invent a home fold when the prefix does not match', () => {
    expect(shortenProjectPath('/tmp/dock', '/home/qingz')).toBe('/tmp/dock');
  });
});

describe('groupSessionsByProject', () => {
  it('groups by raw project_path, sorts those groups, and keeps 其他 last', () => {
    const sessions = [
      { id: 'z1', project_path: '/z/proj' },
      { id: 'none', project_path: null },
      { id: 'a1', project_path: '/a/proj' },
      { id: 'z2', project_path: '/z/proj' },
      { id: 'empty', project_path: '' },
    ];
    const groups = groupSessionsByProject(sessions);
    expect(groups.map((group) => group.key)).toEqual(['/a/proj', '/z/proj', '']);
    expect(groups[0]?.label).toBe('/a/proj');
    expect(groups[1]?.sessions.map((session) => session.id)).toEqual(['z1', 'z2']);
    expect(groups.at(-1)?.label).toBe('其他');
    expect(groups.at(-1)?.sessions.map((session) => session.id)).toEqual(['none', 'empty']);
  });

  it('does not invent a path for sessions without one', () => {
    const groups = groupSessionsByProject([{ id: 'x', project_path: null }]);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.label).toBe('其他');
    expect(groups[0]?.sessions[0]?.project_path).toBeNull();
  });
});
