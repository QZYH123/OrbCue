import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
execFileSync(npm, ['--prefix', 'frontend', 'run', 'build'], { cwd: root, stdio: 'inherit' });
const sidecarArgs = ['scripts/prepare-sidecar.mjs'];
if (process.env.TAURI_ENV_DEBUG === 'true') sidecarArgs.push('--debug');
execFileSync(process.execPath, sidecarArgs, { cwd: root, stdio: 'inherit' });
