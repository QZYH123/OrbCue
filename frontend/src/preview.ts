import type { AgentInventory, AuditEntry, SessionSnapshot, Snapshot } from './types';

export function tauriAvailable(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function previewLabel(): 'ball' | 'panel' {
  const requested = new URLSearchParams(window.location.search).get('label');
  return requested === 'ball' ? 'ball' : 'panel';
}

export function applyPreviewDocument(label: 'ball' | 'panel') {
  document.documentElement.classList.add('preview');
  document.documentElement.classList.toggle('preview-ball', label === 'ball');
  const demo = new URLSearchParams(window.location.search).get('demo');
  if (demo !== 'ball' && demo !== 'panel') return;
  const style = document.createElement('style');
  style.textContent =
    demo === 'ball'
      ? `html, body { width: 480px !important; height: 448px !important; }
         html.preview.preview-ball #app { transform: scale(2.8); }`
      : `html, body { width: 480px !important; height: 448px !important; }
         html.preview body {
           display: flex !important;
           align-items: center !important;
           justify-content: flex-end !important;
           padding: 24px 24px 24px 0 !important;
         }`;
  document.head.appendChild(style);
}

export function previewSnapshot(): Snapshot {
  const cue = new URLSearchParams(window.location.search).get('cue');
  if (cue !== 'working') return demoSnapshot;
  return { ...demoSnapshot, pending_count: 0, pending_mark: '' };
}

const PROJECT = '/home/qingz/projects/agent-activity-dock';
const AT = '2026-08-22T';

function session(
  source: SessionSnapshot['source'],
  session_id: string,
  state: SessionSnapshot['state'],
  extra: Partial<SessionSnapshot> = {},
): SessionSnapshot {
  return {
    source,
    session_id,
    state,
    mark: '',
    attention_reason: null,
    deep_link: null,
    project_path: PROJECT,
    terminal_id: null,
    acknowledged: true,
    occurred_at: `${AT}10:00:00Z`,
    ...extra,
  };
}

function audit(
  source: AuditEntry['source'],
  session_id: string,
  state: AuditEntry['state'],
  occurred_at: string,
  extra: Partial<AuditEntry> = {},
): AuditEntry {
  return { source, session_id, state, attention_reason: null, occurred_at, project_path: PROJECT, ...extra };
}

export const demoSnapshot: Snapshot = {
  working_count: 2,
  tracked_count: 5,
  pending_count: 3,
  pending_mark: '?',
  count_label: '2/5',
  sessions: [
    session('claude', 'a1b2c3d4-e5f6-7890-abcd-ef1234567890', 'working', {
      terminal_id: 'orb:ab12cd',
    }),
    session('grok', 'b2c3d4e5-f6a7-8901-bcde-f12345678901', 'needs_attention', {
      mark: '?',
      attention_reason: 'input',
      terminal_id: 'orb:ff00aa',
      acknowledged: false,
      occurred_at: `${AT}10:04:00Z`,
    }),
    session('claude', 'c3d4e5f6-a7b8-9012-cdef-123456789012', 'working', {
      acknowledged: false,
      occurred_at: `${AT}09:40:00Z`,
    }),
    session('codex', 'docs-pass', 'failed', {
      mark: '!',
      project_path: '/home/qingz/projects/docs-site',
      acknowledged: false,
      occurred_at: `${AT}09:12:00Z`,
    }),
    session('cursor', 'notes-1', 'idle', {
      mark: 'o',
      project_path: null,
      occurred_at: `${AT}08:50:00Z`,
    }),
  ],
  audit: [
    audit('codex', 'docs-pass', 'failed', `${AT}09:12:00Z`, { project_path: '/home/qingz/projects/docs-site' }),
    audit('claude', 'a1b2c3d4-e5f6-7890-abcd-ef1234567890', 'completed', `${AT}10:00:00Z`),
    audit('grok', 'b2c3d4e5-f6a7-8901-bcde-f12345678901', 'needs_attention', `${AT}10:04:00Z`, { attention_reason: 'input' }),
    audit('grok', '01a02d1d-7c3d-7401-ad5b-0b33cdba9b0d', 'closed', `${AT}10:08:00Z`),
    ...Array.from({ length: 12 }, (_, index) => {
      const source = (['claude', 'grok', 'codex', 'cursor'] as const)[index % 4];
      const state = (['completed', 'failed', 'needs_attention', 'closed'] as const)[index % 4];
      return audit(source, `audit-row-${index}`, state, `${AT}10:${String(12 + index).padStart(2, '0')}:00Z`, {
        attention_reason: state === 'needs_attention' ? 'input' : null,
      });
    }),
  ],
};

export const demoInventory: AgentInventory = {
  discovered: [
    { name: 'grok', path: '/home/qingz/.local/bin/grok', side: 'wsl' },
    { name: 'codex', path: 'C:\\Users\\qingz\\AppData\\Local\\codex.exe', side: 'windows' },
  ],
  connected: [
    {
      name: 'claude',
      original: '/home/qingz/.local/bin/claude',
      method: 'ClaudeHook',
      wrapper: null,
      hook_script: '/home/qingz/.claude/hooks/orbcue.sh',
      limitation: 'Hook 只转发明确的生命周期事件',
      side: 'wsl',
    },
  ],
};
