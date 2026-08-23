export async function ensureNotificationPermission(
  isGranted: () => Promise<boolean>,
  request: () => Promise<string>,
): Promise<boolean> {
  try {
    if (await isGranted()) return true;
    return (await request()) === 'granted';
  } catch {
    return false;
  }
}
