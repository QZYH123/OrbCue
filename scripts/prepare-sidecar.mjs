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
const skipWslOrbBuild = process.env.ORBCUE_SKIP_WSL_ORB_BUILD === '1';
const wslOrbDest = join(root, 'src-tauri', 'resources', 'orb-wsl');

const cargoArgs = useXwin ? ['xwin', 'build'] : ['build'];
cargoArgs.push('-p', 'orbcue-cli');
if (!debug) cargoArgs.push('--release');
if (target) cargoArgs.push('--target', target);

execFileSync('cargo', cargoArgs, { cwd: root, stdio: 'inherit' });

const exe = windowsTarget ? '.exe' : '';
const binaryName = `orb${exe}`;
const binaryRoot = target ? join(root, 'target', target, profile) : join(root, 'target', profile);
const source = join(binaryRoot, binaryName);
const sidecarDir = join(root, 'src-tauri', 'binaries');
const suffix = target || hostTarget();
const destination = join(sidecarDir, `orb-${suffix}${exe}`);

mkdirSync(sidecarDir, { recursive: true });
rmSync(destination, { force: true });
copyFileSync(source, destination);
console.log(`Prepared OrbCue CLI sidecar: ${destination}`);
prepareWslOrb(source);

function prepareWslOrb(sidecarSource) {
  mkdirSync(dirname(wslOrbDest), { recursive: true });
  if (isLinuxElf(wslOrbDest)) {
    console.log(`Using existing WSL orb resource: ${wslOrbDest}`);
    return;
  }
  if (skipWslOrbBuild) {
    throw new Error(
      `Missing ${wslOrbDest}. Place a Linux orb binary there (CI linux-cli artifact) before the Windows bundle.`,
    );
  }
  if (process.platform === 'win32') {
    console.warn(
      `WSL orb resource missing at ${wslOrbDest}; WSL auto-install will be unavailable`,
    );
    return;
  }

  let linuxSource = sidecarSource;
  if (windowsTarget || linuxSource.endsWith('.exe')) {
    execFileSync('cargo', ['build', '-p', 'orbcue-cli', '--release'], {
      cwd: root,
      stdio: 'inherit',
    });
    linuxSource = join(root, 'target', 'release', 'orb');
  }
  if (!existsSync(linuxSource)) {
    throw new Error(`Linux orb binary not found: ${linuxSource}`);
  }
  rmSync(wslOrbDest, { force: true });
  copyFileSync(linuxSource, wslOrbDest);
  console.log(`Prepared WSL orb resource: ${wslOrbDest}`);
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
