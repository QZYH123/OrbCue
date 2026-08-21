import { describe, expect, it } from 'vitest';
import { agentConnectable, inventoryHasRows, showDetectingPlaceholder } from './inventory';
import type { AgentInventory, DiscoveredAgent } from './types';

const empty: AgentInventory = { discovered: [], connected: [] };
const cached: AgentInventory = {
  discovered: [{ name: 'claude', path: '/home/u/.local/bin/claude', origin: 'wsl', connectable: true }],
  connected: [],
};

describe('inventory display', () => {
  it('keeps the cached list visible while a refresh is in flight', () => {
    expect(inventoryHasRows(cached)).toBe(true);
    expect(showDetectingPlaceholder(cached, true)).toBe(false);
    expect(showDetectingPlaceholder(empty, true)).toBe(true);
    expect(showDetectingPlaceholder(empty, false)).toBe(false);
  });

  it('does not offer connect for Windows PATH entries', () => {
    const windows: DiscoveredAgent = {
      name: 'claude',
      path: '/mnt/c/Users/u/AppData/Roaming/npm/claude',
      origin: 'windows',
      connectable: false,
    };
    expect(agentConnectable(windows)).toBe(false);
    expect(agentConnectable({ name: 'claude', path: '/home/u/.local/bin/claude' })).toBe(true);
  });
});
