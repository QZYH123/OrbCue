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
