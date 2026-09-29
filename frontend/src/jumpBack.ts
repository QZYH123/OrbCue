import type { Lang } from './locale';

export const DOCK_TERMINAL_ID = /^orb:[0-9a-fA-F]{6}$/;

const JUMP = {
  zh: {
    missing: '找不到该会话的窗口。用 orb run 启动可获得精确跳回',
    level: '已回到最近交互的窗口',
    empty: 'Agent 发出事件后会显示在这里。想精确跳回终端时用 orb run 启动',
    intro:
      '只连接本机已有的工具，不下载、不替换、也不读取工作内容。没有 WSL 时只显示 Windows 上的工具。想精确跳回终端，用 orb run 启动。',
  },
  en: {
    missing:
      "Can't find that session's window. Start it with orb run to jump back to the exact tab.",
    level: 'Back at the last window you used.',
    empty: 'Sessions show up here after an agent sends an event. Use orb run to jump back to the exact tab.',
    intro:
      'Only installed tools. Nothing downloaded, replaced, or read. Without WSL, Windows tools only. Use orb run to jump to the exact tab.',
  },
} as const;

export const JUMP_WINDOW_MISSING = JUMP.zh.missing;
export const JUMP_WINDOW_LEVEL = JUMP.zh.level;
export const EMPTY_TRACKING_HINT = JUMP.zh.empty;
export const CONNECTIONS_INTRO = JUMP.zh.intro;

export function jumpPhrases(lang: Lang = 'zh') {
  return JUMP[lang];
}

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

export function jumpFeedback(result: JumpResult, lang: Lang = 'zh'): JumpFeedback {
  const phrases = jumpPhrases(lang);
  if (result.focused) {
    if (result.precise) {
      return { kind: 'silent', text: null };
    }
    return { kind: 'note', text: phrases.level };
  }
  const reason = result.reason?.trim();
  return { kind: 'error', text: reason || phrases.missing };
}
