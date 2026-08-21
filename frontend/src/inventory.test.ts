import { describe, expect, it } from 'vitest';
import { inventoryHasRows, showDetectingPlaceholder, sideLabel } from './inventory';
import type { AgentInventory } from './types';

const empty: AgentInventory = { discovered: [], connected: [] };
const cached: AgentInventory = {
  discovered: [{ name: 'claude', path: '/home/u/.local/bin/claude', side: 'wsl' }],
  connected: [],
};

describe('inventory display', () => {
  it('keeps the cached list visible while a refresh is in flight', () => {
    expect(inventoryHasRows(cached)).toBe(true);
    expect(showDetectingPlaceholder(cached, true)).toBe(false);
    expect(showDetectingPlaceholder(empty, true)).toBe(true);
    expect(showDetectingPlaceholder(empty, false)).toBe(false);
  });

  it('labels both sides for the connection cards', () => {
    expect(sideLabel('wsl')).toBe('WSL');
    expect(sideLabel('windows')).toBe('Windows');
  });
});
