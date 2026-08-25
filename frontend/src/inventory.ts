import { displayAgent } from './sessionIdentity';
import type { AgentInventory, AgentSide } from './types';

export function inventoryHasRows(inventory: AgentInventory): boolean {
  return inventory.discovered.length > 0 || inventory.connected.length > 0;
}

export function showDetectingPlaceholder(
  inventory: AgentInventory,
  refreshing: boolean,
): boolean {
  return refreshing && !inventoryHasRows(inventory);
}

export function sideLabel(side: AgentSide): string {
  return side === 'wsl' ? 'WSL' : 'Windows';
}

export function wslDockErrorBanner(inventory: AgentInventory): string | null {
  const raw = inventory.wsl_error?.trim();
  if (!raw) {
    return null;
  }
  return `WSL 侧 dock 未就绪：${raw}`;
}

/** Agents whose CLI command differs from the agent name shown in the UI. */
const AGENT_COMMANDS: Record<string, string> = { cursor: 'cursor-agent' };

export function connectSuccessNotice(name: string, side: AgentSide): string {
  const command = AGENT_COMMANDS[name.trim().toLowerCase()] ?? name;
  return `已连接 ${displayAgent(name)}（${sideLabel(side)}）。正在运行的 ${command} 不受影响；新开一个终端重新启动它，任务才会出现在小球上。`;
}
