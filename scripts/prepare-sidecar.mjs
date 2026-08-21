import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const debug = process.argv.includes('--debug');
const target = process.env.TAURI_ENV_TARGET_TRIPLE || '';
const profile = debug ? 'debug' : 'release';
const windowsTarget = /windows/i.test(target) || (!target && process.platform === 'win32');
const useXwin = windowsTarget && process.platform !== 'win32';

const cargoArgs = useXwin ? ['xwin', 'build'] : ['build'];
cargoArgs.push('-p', 'agent-activity-dock-cli');
if (!debug) cargoArgs.push('--release');
if (target) cargoArgs.push('--target', target);

execFileSync('cargo', cargoArgs, { cwd: root, stdio: 'inherit' });

const exe = windowsTarget ? '.exe' : '';
const binaryName = `dock${exe}`;
const binaryRoot = target ? join(root, 'target', target, profile) : join(root, 'target', profile);
const source = join(binaryRoot, binaryName);
const sidecarDir = join(root, 'src-tauri', 'binaries');
const suffix = target || hostTarget();
const destination = join(sidecarDir, `dock-${suffix}${exe}`);

mkdirSync(sidecarDir, { recursive: true });
rmSync(destination, { force: true });
copyFileSync(source, destination);
console.log(`Prepared Dock CLI sidecar: ${destination}`);

function hostTarget() {
  try {
    const rustc = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
    return rustc.match(/^host:\s*(.+)$/m)?.[1] || `${process.arch}-${process.platform}`;
  } catch {
    return `${process.arch}-${process.platform}`;
  }
}
