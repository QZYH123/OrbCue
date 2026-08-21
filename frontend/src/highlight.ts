export function sessionHighlightKey(source: string, sessionId: string): string {
  return `${source}\0${sessionId}`;
}

export function highlightFromNotificationExtra(
  extra: Record<string, unknown> | undefined,
): { source: string; session_id: string } | null {
  const source = extra?.source;
  const sessionId = extra?.session_id;
  if (typeof source === 'string' && source && typeof sessionId === 'string' && sessionId) {
    return { source, session_id: sessionId };
  }
  return null;
}
