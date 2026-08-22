import type { AgentInventory, Snapshot } from './types';

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
}

export const demoSnapshot: Snapshot = {
  working_count: 2,
  tracked_count: 5,
  pending_count: 3,
  pending_mark: '?',
  count_label: '2/5',
  border_state: 'working',
  sessions: [
    {
      source: 'claude',
      session_id: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890',
      state: 'working',
      mark: '',
      attention_reason: null,
      summary: '正在重做小球与展开面板',
      deep_link: null,
      project_path: '/home/qingz/projects/agent-activity-dock',
      window_title: 'dock:ab12cd · claude · agent-activity-dock',
      terminal_id: 'dock:ab12cd',
      requires_user_action: false,
      acknowledged: true,
      occurred_at: '2026-08-22T10:00:00Z',
    },
    {
      source: 'grok',
      session_id: 'b2c3d4e5-f6a7-8901-bcde-f12345678901',
      state: 'needs_attention',
      mark: '?',
      attention_reason: 'input',
      summary: '等待你确认新的会话标题规则',
      deep_link: null,
      project_path: '/home/qingz/projects/agent-activity-dock',
      window_title: 'dock:ff00aa · grok · agent-activity-dock',
      terminal_id: 'dock:ff00aa',
      requires_user_action: true,
      acknowledged: false,
      occurred_at: '2026-08-22T10:04:00Z',
    },
    {
      source: 'claude',
      session_id: 'c3d4e5f6-a7b8-9012-cdef-123456789012',
      state: 'working',
      mark: '',
      attention_reason: null,
      summary: '已写完 jump-back 测试',
      deep_link: null,
      project_path: '/home/qingz/projects/agent-activity-dock',
      window_title: null,
      terminal_id: null,
      requires_user_action: false,
      acknowledged: false,
      occurred_at: '2026-08-22T09:40:00Z',
    },
    {
      source: 'codex',
      session_id: 'docs-pass',
      state: 'failed',
      mark: '!',
      attention_reason: null,
      summary: '生成 API 草稿时失败',
      deep_link: null,
      project_path: '/home/qingz/projects/docs-site',
      window_title: null,
      terminal_id: null,
      requires_user_action: false,
      acknowledged: false,
      occurred_at: '2026-08-22T09:12:00Z',
    },
    {
      source: 'dsh',
      session_id: 'notes-1',
      state: 'idle',
      mark: 'o',
      attention_reason: null,
      summary: null,
      deep_link: null,
      project_path: null,
      window_title: 'Windows Terminal - scratch notes',
      terminal_id: null,
      requires_user_action: false,
      acknowledged: true,
      occurred_at: '2026-08-22T08:50:00Z',
    },
  ],
  audit: [
    {
      source: 'codex',
      session_id: 'docs-pass',
      state: 'failed',
      attention_reason: null,
      occurred_at: '2026-08-22T09:12:00Z',
    },
    {
      source: 'claude',
      session_id: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890',
      state: 'working',
      attention_reason: null,
      occurred_at: '2026-08-22T10:00:00Z',
    },
    {
      source: 'grok',
      session_id: 'b2c3d4e5-f6a7-8901-bcde-f12345678901',
      state: 'needs_attention',
      attention_reason: 'input',
      occurred_at: '2026-08-22T10:04:00Z',
    },
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
      hook_script: '/home/qingz/.claude/hooks/dock.sh',
      settings_backup: '/home/qingz/.claude/settings.json.agent-activity-dock.bak',
      capabilities: ['start', 'complete', 'failed', 'waiting'],
      limitation: 'Hook 只转发明确的生命周期事件',
      installed_at: '2026-08-01T00:00:00Z',
      side: 'wsl',
    },
  ],
};
