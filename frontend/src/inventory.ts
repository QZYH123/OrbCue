import type { AgentInventory, DiscoveredAgent } from './types';

export function inventoryHasRows(inventory: AgentInventory): boolean {
  return inventory.discovered.length > 0 || inventory.connected.length > 0;
}

export function showDetectingPlaceholder(
  inventory: AgentInventory,
  refreshing: boolean,
): boolean {
  return refreshing && !inventoryHasRows(inventory);
}

export function agentConnectable(agent: DiscoveredAgent): boolean {
  if (agent.connectable === false) return false;
  return agent.origin !== 'windows';
}
