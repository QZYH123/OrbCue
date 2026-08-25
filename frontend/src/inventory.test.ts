import { describe, expect, it } from 'vitest';
import {
  connectSuccessNotice,
  inventoryHasRows,
  showDetectingPlaceholder,
  sideLabel,
  wslDockErrorBanner,
} from './inventory';
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

  it('tells users to restart their terminal after a connect, in plain words', () => {
    const notice = connectSuccessNotice('cursor', 'wsl');
    expect(notice).toContain('已连接 Cursor（WSL）');
    expect(notice).toContain('cursor-agent');
    expect(notice).toContain('新开一个终端');
    expect(connectSuccessNotice('claude', 'windows')).toContain('（Windows）');
    expect(connectSuccessNotice('claude', 'windows')).toContain('claude 不受影响');
  });

  it('shows a WSL dock error banner when inventory carries wsl_error', () => {
    const withError: AgentInventory = {
      discovered: [],
      connected: [],
      wsl_error: 'wslpath failed (exit status: 1)',
    };
    expect(wslDockErrorBanner(withError)).toBe(
      'WSL 侧 dock 未就绪：wslpath failed (exit status: 1)',
    );
    expect(wslDockErrorBanner(empty)).toBeNull();
    expect(wslDockErrorBanner({ discovered: [], connected: [], wsl_error: '  ' })).toBeNull();
  });
});
