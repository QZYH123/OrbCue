const { app, BrowserWindow, ipcMain, screen, shell } = require('electron');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');
const SRC = path.join(ROOT, 'src');
const BALL_SIZE = 44;
const LIST_WIDTH = 400;
const LIST_MAX_ROWS = 12;

let daemonProcess = null;
let spawnedByUs = false;
let subscriber = null;
let socketPath = null;
let ballWindow = null;
let listWindow = null;
let lineBuffer = '';

function defaultSocketPath() {
  if (process.env.AGENT_ACTIVITY_DOCK_SOCKET) return process.env.AGENT_ACTIVITY_DOCK_SOCKET;
  const runtime = process.env.XDG_RUNTIME_DIR;
  if (runtime && fs.existsSync(runtime)) return path.join(runtime, 'agent-activity-dock', 'agent-activity-dock.sock');
  return path.join(os.homedir(), '.local', 'state', 'agent-activity-dock', 'agent-activity-dock.sock');
}

function sleep(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }

async function waitForFile(file, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (fs.existsSync(file)) return true;
    if (daemonProcess && daemonProcess.exitCode !== null) return false;
    await sleep(40);
  }
  return false;
}

function isSocketLive(socket) {
  return new Promise((resolve) => {
    const probe = net.createConnection(socket);
    probe.once('connect', () => { probe.destroy(); resolve(true); });
    probe.once('error', () => { probe.destroy(); resolve(false); });
    probe.setTimeout(500, () => { probe.destroy(); resolve(false); });
  });
}

async function ensureDaemon() {
  socketPath = defaultSocketPath();
  const parent = path.dirname(socketPath);
  fs.mkdirSync(parent, { recursive: true, mode: 0o700 });
  if (fs.existsSync(socketPath) && await isSocketLive(socketPath)) return false;

  if (fs.existsSync(socketPath)) {
    try { fs.unlinkSync(socketPath); } catch {}
  }

  const readyFile = path.join(os.tmpdir(), `agent-activity-dock-ready-${process.pid}.json`);
  const env = { ...process.env, PYTHONPATH: SRC + path.delimiter + (process.env.PYTHONPATH || '') };
  daemonProcess = spawn(process.env.AADOCK_PYTHON || 'python3', [
    '-m', 'agent_activity_dock.daemon',
    '--socket', socketPath,
    '--ready-file', readyFile,
  ], { cwd: ROOT, env, stdio: ['ignore', 'pipe', 'pipe'] });
  spawnedByUs = true;

  if (!(await waitForFile(readyFile))) {
    const err = daemonProcess.stderr ? daemonProcess.stderr.read() : '';
    throw new Error(`daemon did not start: ${err}`);
  }
  try { fs.unlinkSync(readyFile); } catch {}
  return true;
}

function parseLines(chunk) {
  lineBuffer += chunk.toString('utf8');
  const messages = [];
  let idx;
  while ((idx = lineBuffer.indexOf('\n')) >= 0) {
    const line = lineBuffer.slice(0, idx).trim();
    lineBuffer = lineBuffer.slice(idx + 1);
    if (!line) continue;
    try { messages.push(JSON.parse(line)); } catch {}
  }
  return messages;
}

function broadcastToWindows(message) {
  for (const win of [ballWindow, listWindow]) {
    if (win && !win.isDestroyed()) win.webContents.send('dock:snapshot', message);
  }
}

async function connectSubscription() {
  subscriber = net.createConnection(socketPath);
  subscriber.setEncoding('utf8');
  subscriber.on('data', (chunk) => {
    for (const message of parseLines(chunk)) broadcastToWindows(message);
  });
  subscriber.on('error', () => {});
  subscriber.on('close', () => {
    if (!app.isQuitting) setTimeout(() => connectSubscription().catch(() => {}), 500);
  });
  await new Promise((resolve, reject) => {
    subscriber.once('connect', resolve);
    subscriber.once('error', reject);
  });
  subscriber.write('{"query":"subscribe"}\n');
}

function sendQuery(payload) {
  return new Promise((resolve, reject) => {
    const conn = net.createConnection(socketPath);
    let buf = '';
    conn.setEncoding('utf8');
    conn.on('connect', () => conn.write(JSON.stringify(payload) + '\n'));
    conn.on('data', (chunk) => {
      buf += chunk;
      const idx = buf.indexOf('\n');
      if (idx >= 0) {
        try { resolve(JSON.parse(buf.slice(0, idx))); } catch (e) { reject(e); }
        conn.destroy();
      }
    });
    conn.on('error', reject);
    setTimeout(() => { conn.destroy(); reject(new Error('timeout')); }, 3000);
  });
}

function createBallWindow() {
  const display = screen.getPrimaryDisplay();
  const work = display.workArea;
  ballWindow = new BrowserWindow({
    width: BALL_SIZE,
    height: BALL_SIZE,
    x: work.x + work.width - BALL_SIZE - 12,
    y: work.y + 12,
    frame: false,
    transparent: true,
    resizable: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    hasShadow: false,
    show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  ballWindow.loadFile(path.join(__dirname, 'renderer', 'ball.html'));
  ballWindow.once('ready-to-show', () => ballWindow.show());
  ballWindow.on('closed', () => { ballWindow = null; });
}

function createListWindow() {
  listWindow = new BrowserWindow({
    width: LIST_WIDTH,
    height: 330,
    frame: false,
    transparent: true,
    resizable: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    hasShadow: true,
    show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  listWindow.loadFile(path.join(__dirname, 'renderer', 'list.html'));
  listWindow.on('closed', () => { listWindow = null; });
  listWindow.on('blur', () => {
    // Keep it open while clicking rows; hide only when focus truly leaves.
    setTimeout(() => { if (listWindow && !listWindow.webContents.isFocused()) listWindow.hide(); }, 80);
  });
}

function placeListWindow() {
  if (!listWindow || !ballWindow) return;
  const [bx, by] = ballWindow.getPosition();
  const { width } = listWindow.getBounds();
  const display = screen.getPrimaryDisplay();
  const work = display.workArea;
  let x = bx + BALL_SIZE - width;
  x = Math.max(work.x, Math.min(x, work.x + work.width - width));
  let y = by + BALL_SIZE + 8;
  if (y + listWindow.getBounds().height > work.y + work.height) y = by - listWindow.getBounds().height - 8;
  listWindow.setPosition(x, y);
}

ipcMain.on('dock:ball-clicked', async () => {
  try { await sendQuery({ query: 'acknowledge', task_id: '*' }); } catch {}
  if (listWindow) {
    if (listWindow.isVisible()) listWindow.hide();
    else { placeListWindow(); listWindow.show(); }
  }
});

ipcMain.on('dock:ack-task', async (_event, taskId) => {
  try { await sendQuery({ query: 'acknowledge', task_id: String(taskId) }); } catch {}
});

ipcMain.on('dock:list-close', () => { if (listWindow) listWindow.hide(); });

app.whenReady().then(async () => {
  try {
    spawnedByUs = await ensureDaemon();
    await connectSubscription();
    createBallWindow();
    createListWindow();
  } catch (error) {
    console.error('[dock] startup failed:', error);
    app.quit();
  }
});

app.on('window-all-closed', () => app.quit());

app.on('before-quit', () => { app.isQuitting = true; });
app.on('quit', () => {
  if (subscriber) { try { subscriber.destroy(); } catch {} }
  if (spawnedByUs && daemonProcess && daemonProcess.exitCode === null) {
    daemonProcess.kill('SIGTERM');
  }
});
