import { copyOf } from './copy';
import type { Lang } from './locale';
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

const WSL_BANNER_LIMIT = 160;

export function wslDockErrorBanner(inventory: AgentInventory, lang: Lang = 'zh'): string | null {
  const raw = inventory.wsl_error?.trim();
  if (!raw) {
    return null;
  }
  const brief = raw.length > WSL_BANNER_LIMIT ? `${raw.slice(0, WSL_BANNER_LIMIT - 1)}…` : raw;
  return copyOf(lang).wslOrbNotReady(brief);
}

/** Agents whose CLI command differs from the agent name shown in the UI. */
const AGENT_COMMANDS: Record<string, string> = { cursor: 'agent' };

export function connectSuccessNotice(name: string, side: AgentSide, lang: Lang = 'zh'): string {
  const command = AGENT_COMMANDS[name.trim().toLowerCase()] ?? name;
  return copyOf(lang).connectSuccess(displayAgent(name), sideLabel(side), command);
}
