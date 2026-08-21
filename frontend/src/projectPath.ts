export const PROJECT_PATH_DISPLAY_LIMIT = 36;
const OTHER_KEY = '';
const OTHER_LABEL = '其他';

export interface ProjectGroup<T> {
  key: string;
  label: string;
  sessions: T[];
}

export function shortenProjectPath(path: string, home?: string): string {
  const folded = foldHome(path, home ?? envHome());
  if (folded.length <= PROJECT_PATH_DISPLAY_LIMIT) {
    return folded;
  }
  return ellipsizeMiddle(folded, PROJECT_PATH_DISPLAY_LIMIT);
}

export function groupSessionsByProject<T extends { project_path?: string | null }>(
  sessions: T[],
  home?: string,
): ProjectGroup<T>[] {
  const groups = new Map<string, T[]>();
  const other: T[] = [];

  for (const session of sessions) {
    const path = session.project_path;
    if (path) {
      const existing = groups.get(path);
      if (existing) {
        existing.push(session);
      } else {
        groups.set(path, [session]);
      }
    } else {
      other.push(session);
    }
  }

  const result = [...groups.keys()]
    .sort((left, right) => (left < right ? -1 : left > right ? 1 : 0))
    .map((key) => ({
      key,
      label: shortenProjectPath(key, home),
      sessions: groups.get(key) ?? [],
    }));

  if (other.length > 0) {
    result.push({ key: OTHER_KEY, label: OTHER_LABEL, sessions: other });
  }
  return result;
}

function foldHome(path: string, home?: string): string {
  if (!home) {
    return path;
  }
  const normalizedPath = normalizeSeparators(path);
  const normalizedHome = normalizeSeparators(home).replace(/\/+$/, '');
  if (normalizedPath === normalizedHome) {
    return '~';
  }
  if (!normalizedPath.startsWith(`${normalizedHome}/`)) {
    return path;
  }
  const rest = normalizedPath.slice(normalizedHome.length);
  return usesBackslashStyle(path, home) ? `~${toBackslash(rest)}` : `~${rest}`;
}

function ellipsizeMiddle(path: string, max: number): string {
  const ellipsis = '…';
  if (path.length <= max) {
    return path;
  }
  const lastSep = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (lastSep > 0) {
    const tail = path.slice(lastSep);
    const headBudget = max - ellipsis.length - tail.length;
    if (headBudget >= 1) {
      return `${path.slice(0, headBudget)}${ellipsis}${tail}`;
    }
  }
  const keep = Math.max(max - ellipsis.length, 1);
  const headLen = Math.ceil(keep / 2);
  const tailLen = keep - headLen;
  return `${path.slice(0, headLen)}${ellipsis}${path.slice(path.length - tailLen)}`;
}

function envHome(): string | undefined {
  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
    ?.env;
  return env?.HOME || env?.USERPROFILE;
}

function normalizeSeparators(path: string): string {
  return path.replace(/\\/g, '/');
}

function toBackslash(path: string): string {
  return path.replace(/\//g, '\\');
}

function usesBackslashStyle(path: string, home: string): boolean {
  return (path.includes('\\') || home.includes('\\')) && !path.includes('/');
}
