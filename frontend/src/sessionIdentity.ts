import { groupSessionsByProject, shortenProjectPath } from './projectPath';

export interface SessionLike {
  source: string;
  session_id: string;
  project_path?: string | null;
  window_title?: string | null;
  terminal_id?: string | null;
}

export interface SessionRow {
  title: string;
  index: string;
  agent: string;
}

export interface SessionSection<T extends SessionLike> {
  key: string;
  label: string;
  rows: Array<{ session: T } & SessionRow>;
}

const AGENT_NAMES: Record<string, string> = {
  claude: 'Claude',
  grok: 'Grok',
  'grok-build': 'Grok',
  grok_build: 'Grok',
  codex: 'Codex',
  dsh: 'DSH',
};

const DOCK_MARKER = /^dock:[0-9a-f]{6}\s*[·•\-–—]\s*/i;
const WT_PREFIX = /^Windows Terminal\s*[-–—]\s*/i;
const OTHER_LABEL = '其他';

export function folderName(path: string | null | undefined): string | null {
  if (!path) return null;
  const trimmed = path.trim().replace(/[\\/]+$/, '');
  if (!trimmed) return null;
  const segment = trimmed.split(/[\\/]/).filter(Boolean).at(-1);
  return segment || null;
}

export function displayAgent(source: string): string {
  const key = source.trim().toLowerCase();
  if (!key) return 'Agent';
  if (AGENT_NAMES[key]) return AGENT_NAMES[key];
  return source.charAt(0).toUpperCase() + source.slice(1);
}

export function shortSessionId(id: string): string {
  const trimmed = id.trim();
  if (trimmed.length <= 10) return trimmed;
  const hex = trimmed.replace(/-/g, '');
  if (/^[0-9a-f]+$/i.test(hex) && hex.length >= 12) {
    return hex.slice(0, 8);
  }
  return `${trimmed.slice(0, 8)}…`;
}

export function cleanWindowTitle(title: string): string {
  return title.replace(WT_PREFIX, '').replace(DOCK_MARKER, '').trim();
}

export function sessionDetail(session: SessionLike): string {
  const terminal = session.terminal_id?.trim() ?? '';
  const dock = /^dock:([0-9a-fA-F]{6})$/.exec(terminal);
  if (dock?.[1]) return dock[1].toLowerCase();
  return shortSessionId(session.session_id);
}

export function presentSessionSections<T extends SessionLike>(
  sessions: T[],
  home?: string,
): SessionSection<T>[] {
  const groups = groupSessionsByProject(sessions, home);
  const folderCounts = new Map<string, number>();
  for (const group of groups) {
    if (!group.key) continue;
    const folder = folderName(group.key);
    if (!folder) continue;
    folderCounts.set(folder, (folderCounts.get(folder) ?? 0) + 1);
  }

  return groups.map((group) => {
    const folder = folderName(group.key);
    const label =
      !group.key || !folder
        ? OTHER_LABEL
        : (folderCounts.get(folder) ?? 0) > 1
          ? shortenProjectPath(group.key, home)
          : folder;

    const sourceIndex = new Map<string, number>();
    const rows = group.sessions.map((session) => {
      const agent = displayAgent(session.source);
      const next = (sourceIndex.get(session.source) ?? 0) + 1;
      sourceIndex.set(session.source, next);
      return {
        session,
        title: agent,
        index: String(next).padStart(2, '0'),
        agent,
      };
    });

    return { key: group.key, label, rows };
  });
}

export function filterSessionSections<T extends SessionLike>(
  sections: SessionSection<T>[],
  visible: T[],
): SessionSection<T>[] {
  const allow = new Set(visible);
  return sections
    .map((section) => ({
      ...section,
      rows: section.rows.filter((row) => allow.has(row.session)),
    }))
    .filter((section) => section.rows.length > 0);
}
