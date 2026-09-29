import type { Lang } from './locale';
import { uiLang } from './locale';

export interface Copy {
  close: string;
  dismiss: string;
  panelLabel: string;
  appearance: string;
  filterSessions: string;
  pages: string;
  firstRun: string;
  other: string;
  idle: string;
  working: string;
  failed: string;
  done: string;
  closed: string;
  cancelled: string;
  needsInput: string;
  needsApproval: string;
  heroFailed: string;
  heroNeedsYou: string;
  navActivity: string;
  navAudit: string;
  navConnect: string;
  navSettings: string;
  filterAll: string;
  filterWorking: string;
  filterNotWorking: string;
  refresh: string;
  addFolder: string;
  checking: string;
  connected: string;
  available: string;
  disconnect: string;
  connect: string;
  confirmConnect: string;
  cancel: string;
  buildingPreview: string;
  skip: string;
  next: string;
  doneSetup: string;
  turnOn: string;
  dontAsk: string;
  apply: string;
  markRead: string;
  clear: string;
  markAllRead: string;
  clearAll: string;
  jumpExact: string;
  jumpWindow: string;
  jumpWindowTitle: string;
  emptyAll: string;
  emptyFiltered: string;
  checkingTools: string;
  noTools: string;
  noToolsOnboarding: string;
  noToolsConnect: string;
  checkingAgents: string;
  auditIntro: string;
  auditEmpty: string;
  auditEmptyHint: string;
  settingsIntro: string;
  runAlias: string;
  runAliasHint: string;
  originalCommand: string;
  originalCommandHint: string;
  replaceTab: string;
  replaceTabHint: string;
  hideBadge: string;
  hideBadgeHint: string;
  snapEdge: string;
  snapEdgeHint: string;
  doneSound: string;
  doneSoundHint: string;
  waitingSound: string;
  waitingSoundHint: string;
  failureSound: string;
  failureSoundHint: string;
  systemNotifications: string;
  systemNotificationsHint: string;
  completionAlerts: string;
  completionAlertsHint: string;
  phoneAlerts: string;
  phoneAlertsHint: string;
  phonePlaceholder: string;
  phoneLabel: string;
  startAtLogin: string;
  startAtLoginHint: string;
  globalShortcut: string;
  autostartTitle: string;
  autostartBody: string;
  privacyTitle: string;
  privacyBody: string;
  onboardingLook: string;
  onboardingLookBody: string;
  onboardingConnect: string;
  onboardingConnectBody: string;
  onboardingRun: string;
  onboardingRunBody: string;
  previewOff: string;
  phoneOff: string;
  phoneTestSent: string;
  aliasCleared: string;
  aliasRemoved: string;
  previewLimitation: string;
  ballToggle: (count: string, state: string) => string;
  connectTitle: (name: string) => string;
  connectUses: (side: string) => string;
  onboardingAlias: (alias: string) => string;
  previewAlias: (name: string) => string;
  aliasNext: (alias: string) => string;
  phoneSavedTestFailed: (error: string) => string;
  shortcutHint: (shortcut: string) => string;
  connectSuccess: (agent: string, side: string, command: string) => string;
  wslOrbNotReady: (detail: string) => string;
}

const zh: Copy = {
  close: '关闭',
  dismiss: '关闭提示',
  panelLabel: 'OrbCue 任务列表',
  appearance: '外观',
  filterSessions: '筛选任务',
  pages: 'OrbCue 页面',
  firstRun: '初次设置',
  other: '其他',
  idle: '空闲',
  working: '工作中',
  failed: '失败',
  done: '已完成',
  closed: '已关闭',
  cancelled: '已取消',
  needsInput: '等待输入',
  needsApproval: '等待授权',
  heroFailed: '有失败',
  heroNeedsYou: '需要你',
  navActivity: '动态',
  navAudit: '审计',
  navConnect: '连接',
  navSettings: '设置',
  filterAll: '全部',
  filterWorking: '工作中',
  filterNotWorking: '未工作',
  refresh: '刷新',
  addFolder: '从文件夹添加',
  checking: '正在检测',
  connected: '已连接',
  available: '可连接',
  disconnect: '断开',
  connect: '连接',
  confirmConnect: '确认连接',
  cancel: '取消',
  buildingPreview: '正在生成预览',
  skip: '跳过',
  next: '下一步',
  doneSetup: '完成',
  turnOn: '开启',
  dontAsk: '不再提示',
  apply: '应用',
  markRead: '已读',
  clear: '清除',
  markAllRead: '全部已读',
  clearAll: '清除全部',
  jumpExact: '精确跳回',
  jumpWindow: '回到最近交互的窗口',
  jumpWindowTitle: '回到最近交互的窗口（不保证精确）',
  emptyAll: '还没有追踪中的任务',
  emptyFiltered: '没有符合条件的任务',
  checkingTools: '正在检测本机工具',
  noTools: '没有检测到支持的工具',
  noToolsOnboarding: '可点「从文件夹添加」，或先跳过、稍后在连接页再接。',
  noToolsConnect: '目前支持 Claude、Grok、Codex 和 Cursor。可点「从文件夹添加」。没有 WSL 也可以只连 Windows 上的工具',
  checkingAgents: '正在检测本机 Agent',
  auditIntro: '完成、失败、等待和关闭。',
  auditEmpty: '还没有审计记录',
  auditEmptyHint: '完成、失败、等待或关闭后会显示在这里',
  settingsIntro: '默认保持安静，只在任务真正需要你回来时提醒一次。',
  runAlias: '启动别名',
  runAliasHint: '把 orb run 收成短命令，空则删除',
  originalCommand: '原命令启动',
  originalCommandHint: '新开终端后敲 grok 等于 orb run grok。command grok 仍直接跑',
  replaceTab: '启动时替换当前标签页',
  replaceTabHint: 'orb run 开出新标签后关掉当前这个',
  hideBadge: '隐藏圆标',
  hideBadgeHint: '小球右上角的 ? / ! 不再显示',
  snapEdge: '收到侧边',
  snapEdgeHint: '拖到屏幕边缘贴成半圆，悬停展开',
  doneSound: '完成提示音',
  doneSoundHint: '任务正常完成时播放短音',
  waitingSound: '等待提示音',
  waitingSoundHint: '等待输入或授权时播放短音',
  failureSound: '失败提示音',
  failureSoundHint: '任务失败时播放较低音调',
  systemNotifications: '系统通知',
  systemNotificationsHint: '等待输入、授权或失败时弹出一次',
  completionAlerts: '完成通知',
  completionAlertsHint: '任务正常完成时，电脑和手机都通知一次。系统通知关闭时，电脑上不弹',
  phoneAlerts: '手机提醒',
  phoneAlertsHint: '贴上 ntfy 话题网址，留空则不发',
  phonePlaceholder: 'https://ntfy.sh/话题名',
  phoneLabel: '手机提醒地址',
  startAtLogin: '开机自启',
  startAtLoginHint: '登录 Windows 后自动打开 OrbCue，不必先手动启动才能接收 Agent 状态',
  globalShortcut: '全局快捷键',
  autostartTitle: '建议开启开机自启',
  autostartBody: 'OrbCue 保持运行才能收到任务状态；开启后登录 Windows 即自动待命',
  privacyTitle: '本地与隐私优先',
  privacyBody:
    'OrbCue 默认不联网，不读取 transcript、prompt、命令或代码；持久化状态也不包含摘要。填了手机提醒网址后，只把那句提示、工具名和项目文件夹名发到该地址，不发完整路径。',
  onboardingLook: '选一个外观',
  onboardingLookBody: '点一下即可预览，之后仍可在设置里改。',
  onboardingConnect: '连接一个工具',
  onboardingConnectBody: '只接本机已经装好的。确认前会列出将要改的文件，每一步都可以跳过。',
  onboardingRun: '建议用 orb run 启动',
  onboardingRunBody: '在新的 Windows Terminal 标签里运行，点返回箭头才能精确回到那个标签。',
  previewOff: '预览不会发送',
  phoneOff: '已关闭手机提醒',
  phoneTestSent: '已发一条测试到手机',
  aliasCleared: '已清除别名',
  aliasRemoved: '已删除别名',
  previewLimitation: 'Hook 只转发明确的生命周期事件',
  ballToggle: (count, state) => `${count}，${state}。点击展开或收起面板`,
  connectTitle: (name) => `连接 ${name}`,
  connectUses: (side) => `OrbCue 将在 ${side} 侧使用现有可执行文件：`,
  onboardingAlias: (alias) =>
    `claude、codex 同理。设置里打开「原命令启动」后可以直接敲 grok，也可以起短命令${alias ? `，比如 ${alias} grok` : '，例如 or grok'}。`,
  previewAlias: (name) => `预览：${name} grok 等于 orb run grok`,
  aliasNext: (alias) => `之后在新终端输入 ${alias} grok`,
  phoneSavedTestFailed: (error) => `已保存，测试没发出去：${error}`,
  shortcutHint: (shortcut) => `${shortcut} 打开或收起任务面板`,
  connectSuccess: (agent, side, command) =>
    `已连接 ${agent}（${side}）。正在运行的 ${command} 不受影响；新开一个终端重新启动它，任务才会出现在小球上。`,
  wslOrbNotReady: (detail) => `WSL 侧 orb 未就绪：${detail}`,
};

const en: Copy = {
  close: 'Close',
  dismiss: 'Dismiss',
  panelLabel: 'OrbCue sessions',
  appearance: 'Appearance',
  filterSessions: 'Filter sessions',
  pages: 'OrbCue pages',
  firstRun: 'First-time setup',
  other: 'Other',
  idle: 'Idle',
  working: 'Working',
  failed: 'Failed',
  done: 'Done',
  closed: 'Closed',
  cancelled: 'Cancelled',
  needsInput: 'Needs input',
  needsApproval: 'Needs approval',
  heroFailed: 'Failed',
  heroNeedsYou: 'Needs you',
  navActivity: 'Activity',
  navAudit: 'Audit',
  navConnect: 'Connect',
  navSettings: 'Settings',
  filterAll: 'All',
  filterWorking: 'Working',
  filterNotWorking: 'Not working',
  refresh: 'Refresh',
  addFolder: 'Add folder',
  checking: 'Checking…',
  connected: 'Connected',
  available: 'Available',
  disconnect: 'Disconnect',
  connect: 'Connect',
  confirmConnect: 'Connect',
  cancel: 'Cancel',
  buildingPreview: 'Building the preview…',
  skip: 'Skip',
  next: 'Next',
  doneSetup: 'Done',
  turnOn: 'Turn on',
  dontAsk: "Don't ask again",
  apply: 'Apply',
  markRead: 'Mark read',
  clear: 'Clear',
  markAllRead: 'Mark all read',
  clearAll: 'Clear all',
  jumpExact: 'Exact tab',
  jumpWindow: 'Last window',
  jumpWindowTitle: 'Last window you used. It may not be the exact tab.',
  emptyAll: 'No sessions yet',
  emptyFiltered: 'Nothing matches this filter',
  checkingTools: 'Checking this machine',
  noTools: 'No supported tools found',
  noToolsOnboarding: 'Use Add folder, or skip and connect later.',
  noToolsConnect:
    'Claude, Grok, Codex, and Cursor are supported. Use Add folder. Without WSL you can still connect Windows tools.',
  checkingAgents: 'Checking this machine',
  auditIntro: 'Completions, failures, waits, and closes.',
  auditEmpty: 'No audit entries yet',
  auditEmptyHint: 'Completions, failures, waits, and closes show up here',
  settingsIntro: 'Quiet by default. It speaks once, when a task actually needs you back.',
  runAlias: 'Run alias',
  runAliasHint: 'A short command for orb run. Empty removes it.',
  originalCommand: 'Original command',
  originalCommandHint: 'In a new terminal, grok means orb run grok. command grok still runs grok.',
  replaceTab: 'Replace this tab',
  replaceTabHint: 'After orb run opens a new tab, close this one.',
  hideBadge: 'Hide badge',
  hideBadgeHint: 'Hide the ? and ! on the orb.',
  snapEdge: 'Snap to edge',
  snapEdgeHint: 'Drag to a screen edge to rest as a semicircle. Hover to slide it out.',
  doneSound: 'Done sound',
  doneSoundHint: 'A short tone when a task finishes.',
  waitingSound: 'Waiting sound',
  waitingSoundHint: 'A short tone when input or approval is needed.',
  failureSound: 'Failure sound',
  failureSoundHint: 'A lower tone when a task fails.',
  systemNotifications: 'System notifications',
  systemNotificationsHint: 'One toast for input, approval, or failure.',
  completionAlerts: 'Completion alerts',
  completionAlertsHint:
    'On a normal finish, the PC and the phone are both notified once. If system notifications are off, the PC stays quiet.',
  phoneAlerts: 'Phone alerts',
  phoneAlertsHint: 'Paste an ntfy topic URL. Leave it empty to stay offline.',
  phonePlaceholder: 'https://ntfy.sh/topic',
  phoneLabel: 'Phone alert URL',
  startAtLogin: 'Start at login',
  startAtLoginHint: 'Open OrbCue when you sign in to Windows, so it can hear agents without a manual start.',
  globalShortcut: 'Global shortcut',
  autostartTitle: 'Start OrbCue when you sign in',
  autostartBody: 'OrbCue has to be running to hear agents. After this, it waits when you sign in to Windows.',
  privacyTitle: 'Local and private',
  privacyBody:
    'Stays offline. It does not read transcripts, prompts, commands, or code, and saved state has no summary. A phone URL sends only the cue, tool, and folder name.',
  onboardingLook: 'Pick a look',
  onboardingLookBody: 'Click to preview. You can change it later in Settings.',
  onboardingConnect: 'Connect a tool',
  onboardingConnectBody: 'Only tools already installed. You will see which files would change. Any step can be skipped.',
  onboardingRun: 'Start with orb run',
  onboardingRunBody: 'Runs in a new Windows Terminal tab, so the back arrow can jump to that exact tab.',
  previewOff: 'Preview does not send',
  phoneOff: 'Phone alerts are off',
  phoneTestSent: 'Sent a test to your phone',
  aliasCleared: 'Alias cleared',
  aliasRemoved: 'Alias removed',
  previewLimitation: 'The hook only forwards explicit lifecycle events',
  ballToggle: (count, state) => `${count}, ${state}. Click to show or hide the panel`,
  connectTitle: (name) => `Connect ${name}`,
  connectUses: (side) => `OrbCue will use the existing program on ${side}:`,
  onboardingAlias: (alias) =>
    `claude and codex work the same way. Turn on Original command in Settings to type grok, or set a short command${alias ? `, such as ${alias} grok` : ', such as or grok'}.`,
  previewAlias: (name) => `Preview: ${name} grok means orb run grok`,
  aliasNext: (alias) => `In a new terminal, type ${alias} grok`,
  phoneSavedTestFailed: (error) => `Saved, but the test was not sent: ${error}`,
  shortcutHint: (shortcut) => `${shortcut} shows or hides the panel`,
  connectSuccess: (agent, side, command) =>
    `Connected ${agent} (${side}). ${command} that is already running stays as it is. Open a new terminal and start it again before it shows on the orb.`,
  wslOrbNotReady: (detail) => `WSL orb is not ready: ${detail}`,
};

const COPY: Record<Lang, Copy> = { zh, en };

export function copyOf(lang: Lang = uiLang()): Copy {
  return COPY[lang];
}
