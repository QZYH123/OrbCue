export type SessionState =
  | 'idle'
  | 'working'
  | 'needs_attention'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'closed';

export interface SessionSnapshot {
  source: string;
  session_id: string;
  state: SessionState;
  mark: string;
  attention_reason: string | null;
  deep_link: string | null;
  project_path: string | null;
  terminal_id: string | null;
  acknowledged: boolean;
  occurred_at: string;
}

export interface AuditEntry {
  source: string;
  session_id: string;
  state: SessionState;
  attention_reason: string | null;
  occurred_at: string;
  project_path?: string | null;
}

export interface Snapshot {
  working_count: number;
  tracked_count: number;
  pending_count: number;
  pending_mark: string;
  count_label: string;
  sessions: SessionSnapshot[];
  audit: AuditEntry[];
}

export interface FocusResult {
  focused: boolean;
  precise: boolean;
  reason: string | null;
}

export interface Attention {
  source: string;
  session_id: string;
  reason: string;
  severity: 'info' | 'attention' | 'error';
}

export interface SnapshotMessage {
  type: 'subscribed' | 'snapshot';
  snapshot: Snapshot;
  attention: Attention | null;
}

export type AgentSide = 'wsl' | 'windows';

export interface DiscoveredAgent {
  name: string;
  path: string;
  side: AgentSide;
}

export type ConnectionMethod = 'Wrapper' | 'ClaudeHook' | 'GrokHook' | 'CodexHook' | 'CursorHook';

export interface ConnectionRecord {
  name: string;
  original: string;
  method: ConnectionMethod;
  wrapper: string | null;
  hook_script: string | null;
  limitation: string;
  side: AgentSide;
}

export interface PreviewFile {
  path: string;
  action: 'create' | 'modify';
  entries: string[];
}

export interface ConnectionPreview {
  name: string;
  original: string;
  method: ConnectionMethod;
  files: PreviewFile[];
  will_not: string[];
  notes: string[];
  warnings?: string[];
}

export interface AgentInventory {
  discovered: DiscoveredAgent[];
  connected: ConnectionRecord[];
  wsl_error?: string | null;
}

export const emptySnapshot: Snapshot = {
  working_count: 0,
  tracked_count: 0,
  pending_count: 0,
  pending_mark: '',
  count_label: '0/0',
  sessions: [],
  audit: [],
};
