import { execFileSync, spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const built = spawnSync(npm, ['--prefix', 'frontend', 'run', 'build'], {
  cwd: root,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
if (built.error) throw built.error;
if (built.status) process.exit(built.status);
const sidecarArgs = ['scripts/prepare-sidecar.mjs'];
if (process.env.TAURI_ENV_DEBUG === 'true') sidecarArgs.push('--debug');
execFileSync(process.execPath, sidecarArgs, { cwd: root, stdio: 'inherit' });
