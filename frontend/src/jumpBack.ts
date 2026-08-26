export const DOCK_TERMINAL_ID = /^orb:[0-9a-fA-F]{6}$/;

export const JUMP_WINDOW_MISSING =
  '找不到该会话的窗口。用 orb run 启动可获得精确跳回';
export const JUMP_WINDOW_LEVEL = '已回到最近交互的窗口';
export const EMPTY_TRACKING_HINT =
  'Agent 发出事件后会显示在这里。想精确跳回终端时用 orb run 启动';
export const CONNECTIONS_INTRO =
  '只连接本机已有的工具，不下载、不替换、也不读取工作内容。没有 WSL 时只显示 Windows 上的工具。想精确跳回终端，用 orb run 启动。';

export interface JumpResult {
  focused: boolean;
  precise?: boolean;
  reason: string | null;
}

export type JumpFeedback =
  | { kind: 'silent'; text: null }
  | { kind: 'note'; text: string }
  | { kind: 'error'; text: string };

export function isDockTerminalId(terminalId: string | null | undefined): boolean {
  return DOCK_TERMINAL_ID.test(terminalId ?? '');
}

export function jumpFeedback(result: JumpResult): JumpFeedback {
  if (result.focused) {
    if (result.precise) {
      return { kind: 'silent', text: null };
    }
    return { kind: 'note', text: JUMP_WINDOW_LEVEL };
  }
  const reason = result.reason?.trim();
  return { kind: 'error', text: reason || JUMP_WINDOW_MISSING };
}
