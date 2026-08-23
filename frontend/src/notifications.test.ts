import { describe, expect, it } from 'vitest';
import { ensureNotificationPermission } from './notifications';

describe('ensureNotificationPermission', () => {
  it('returns true when already granted', async () => {
    expect(
      await ensureNotificationPermission(
        async () => true,
        async () => 'denied',
      ),
    ).toBe(true);
  });

  it('requests when not granted and accepts granted', async () => {
    expect(
      await ensureNotificationPermission(
        async () => false,
        async () => 'granted',
      ),
    ).toBe(true);
  });

  it('stays off when the user denies the prompt', async () => {
    expect(
      await ensureNotificationPermission(
        async () => false,
        async () => 'denied',
      ),
    ).toBe(false);
  });

  it('fails open to off when the permission APIs throw', async () => {
    expect(
      await ensureNotificationPermission(
        async () => {
          throw new Error('unavailable');
        },
        async () => 'granted',
      ),
    ).toBe(false);
  });
});
