import { execFileSync } from 'node:child_process';
import { closeSync, copyFileSync, existsSync, mkdirSync, openSync, readSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const debug = process.argv.includes('--debug');
const target = process.env.TAURI_ENV_TARGET_TRIPLE || '';
const profile = debug ? 'debug' : 'release';
const windowsTarget = /windows/i.test(target) || (!target && process.platform === 'win32');
const useXwin = windowsTarget && process.platform !== 'win32';
const skipWslDockBuild = process.env.AGENT_ACTIVITY_DOCK_SKIP_WSL_DOCK_BUILD === '1';
const wslDockDest = join(root, 'src-tauri', 'resources', 'dock-wsl');

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
prepareWslDock(source);

function prepareWslDock(sidecarSource) {
  mkdirSync(dirname(wslDockDest), { recursive: true });
  if (isLinuxElf(wslDockDest)) {
    console.log(`Using existing WSL dock resource: ${wslDockDest}`);
    return;
  }
  if (skipWslDockBuild) {
    throw new Error(
      `Missing ${wslDockDest}. Place a Linux dock binary there (CI linux-cli artifact) before the Windows bundle.`,
    );
  }
  if (process.platform === 'win32') {
    console.warn(
      `WSL dock resource missing at ${wslDockDest}; WSL auto-install will be unavailable`,
    );
    return;
  }

  let linuxSource = sidecarSource;
  if (windowsTarget || linuxSource.endsWith('.exe')) {
    execFileSync('cargo', ['build', '-p', 'agent-activity-dock-cli', '--release'], {
      cwd: root,
      stdio: 'inherit',
    });
    linuxSource = join(root, 'target', 'release', 'dock');
  }
  if (!existsSync(linuxSource)) {
    throw new Error(`Linux dock binary not found: ${linuxSource}`);
  }
  rmSync(wslDockDest, { force: true });
  copyFileSync(linuxSource, wslDockDest);
  console.log(`Prepared WSL dock resource: ${wslDockDest}`);
}

function isLinuxElf(path) {
  if (!existsSync(path)) {
    return false;
  }
  const fd = openSync(path, 'r');
  try {
    const buf = Buffer.alloc(4);
    const n = readSync(fd, buf, 0, 4, 0);
    return n === 4 && buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46;
  } finally {
    closeSync(fd);
  }
}

function hostTarget() {
  try {
    const rustc = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
    return rustc.match(/^host:\s*(.+)$/m)?.[1] || `${process.arch}-${process.platform}`;
  } catch {
    return `${process.arch}-${process.platform}`;
  }
}
