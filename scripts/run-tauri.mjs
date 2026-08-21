import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const result = spawnSync(
  npm,
  ['--prefix', 'frontend', 'exec', '--', 'tauri', ...process.argv.slice(2)],
  {
    cwd: root,
    env: { ...process.env, TAURI_FRONTEND_PATH: resolve(root, 'frontend') },
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
process.exit(result.status ?? 1);
