<script lang="ts">
  import type { AgentInventory, AgentSide, ConnectionPreview, ConnectionRecord, DiscoveredAgent, SessionSnapshot, Snapshot } from './types';
  import type { DockTheme } from './theme';
  import { THEMES, THEME_META } from './theme';
  import { CONNECTIONS_INTRO, EMPTY_TRACKING_HINT, isDockTerminalId } from './jumpBack';
  import { displayAgent, formatAuditTime, sessionDomKey, type AuditRow, type SessionSection } from './sessionIdentity';
  import { sessionHighlightKey } from './highlight';
  import { showDetectingPlaceholder, sideLabel, wslDockErrorBanner } from './inventory';
  import { onboardingStepIndex, ONBOARDING_STEPS, type OnboardingStep } from './onboarding';

  export let snapshot: Snapshot;
  export let ballKind: string;
  export let theme: DockTheme;
  export let heroBar: string[];
  export let closePanel: () => void;
  export let setTheme: (theme: DockTheme) => void;
  export let refreshAgents: () => void;
  export let inventoryRefreshing: boolean;
  export let addFromFolder: () => void;
  export let connected: (name: string, side: AgentSide) => ConnectionRecord | undefined;
  export let disconnectAgent: (name: string, side: AgentSide) => void;
  export let connectAgent: (agent: DiscoveredAgent) => void;
  export let connectionAgents: DiscoveredAgent[];
  export let onboardingComplete: boolean;
  export let onboardingStep: OnboardingStep;
  export let skipOnboardingStep: () => void;
  export let finishOnboarding: () => void;
  export let inventory: AgentInventory;
  export let connectionError: string;
  export let runAlias: string;
  export let page: 'activity' | 'audit' | 'connections' | 'settings';
  export let filter: 'all' | 'attention' | 'working';
  export let unread: number;
  export let showAutostartHint: boolean;
  export let acceptAutostartHint: () => void;
  export let dismissAutostartHint: () => void;
  export let sessionGroups: SessionSection<SessionSnapshot>[];
  export let collapsedGroups: Record<string, boolean>;
  export let toggleGroup: (key: string) => void;
  export let highlightedKey: string;
  export let acknowledge: (source: string, sessionId: string, terminalId?: string | null) => void;
  export let resetSession: (source: string, sessionId: string, terminalId?: string | null) => void;
  export let stateLabel: (item: { state: SessionSnapshot['state']; attention_reason: string | null }) => string;
  export let focusNotes: Record<string, string>;
  export let focusErrors: Record<string, string>;
  export let jumpBack: (session: SessionSnapshot) => void;
  export let auditRows: AuditRow[];
  export let connectSuccess: string;
  export let pendingAgent: DiscoveredAgent | null;
  export let closeConnectDialog: () => void;
  export let previewLoading: boolean;
  export let previewError: string;
  export let connectionPreview: ConnectionPreview | null;
  export let confirmConnect: () => void;
  export let runAliasDraft: string;
  export let saveRunAlias: (event: SubmitEvent) => void;
  export let runAliasError: string;
  export let runAliasHint: string;
  export let replaceTabOnRun: boolean;
  export let toggleReplaceTab: () => void;
  export let hideBallBadge: boolean;
  export let toggleHideBallBadge: () => void;
  export let sideDockEnabled: boolean;
  export let toggleSideDock: () => void;
  export let soundEnabled: { completion: boolean; attention: boolean; failure: boolean };
  export let toggleSound: (channel: 'completion' | 'attention' | 'failure') => void;
  export let notificationsEnabled: boolean;
  export let toggleNotifications: () => void;
  export let autostartEnabled: boolean;
  export let toggleAutostart: () => void;
  export let shortcutEnabled: boolean;
  export let toggleShortcut: () => void;
  export let shortcut: string;
  export let selectPage: (page: 'activity' | 'audit' | 'connections' | 'settings') => void;
  export let visibleSessions: SessionSnapshot[];
</script>

  <main class="panel tone-{ballKind}" aria-label="OrbCue 任务列表">
    <header class="hero">
      <div class="hero-main">
        <div class="hero-count lcd" aria-live="polite">
          <span class="lcd-digits">
            <span class="hero-work lcd-work">{snapshot.working_count}</span>
            <span class="hero-slash lcd-slash">/</span>
            <span class="hero-track lcd-track">{snapshot.tracked_count}</span>
          </span>
          <span class="hero-rest">
            <span class="hero-meta lcd-meta">{ballKind === 'fail' ? '有失败' : ballKind === 'wait' ? '需要你' : ballKind === 'working' ? '工作中' : '空闲'}</span>
          </span>
        </div>
        {#if theme === 'glyph'}
          <div class="hero-sub">
            <span class="hero-matrix" aria-hidden="true">
              {#each heroBar as tone, i (i)}<i class="dot {tone}"></i>{/each}
            </span>
          </div>
        {/if}
      </div>
      <button class="icon-button key-round" onclick={closePanel} aria-label="关闭">×</button>
    </header>
    {#snippet themePicker()}
      <div class="theme-picker" role="radiogroup" aria-label="外观">
        {#each THEMES as item (item)}
          <button type="button" role="radio" aria-checked={theme === item} class:active={theme === item} title={THEME_META[item].note} onclick={() => setTheme(item)}>
            {THEME_META[item].name}
          </button>
        {/each}
      </div>
    {/snippet}
    {#snippet connectionsToolbar()}
      <div class="connections-toolbar">
        <button class="text-button" onclick={() => void refreshAgents()} disabled={inventoryRefreshing}>刷新</button>
        <button class="text-button" onclick={() => void addFromFolder()} disabled={inventoryRefreshing}>从文件夹添加</button>
        {#if inventoryRefreshing}<span class="refresh-hint" aria-live="polite">正在检测</span>{/if}
      </div>
    {/snippet}
    {#snippet connectionCard(agent: DiscoveredAgent)}
      {@const record = connected(agent.name, agent.side)}
      <article class="connection-card">
        <div class="connection-content">
          <div class="connection-title">
            <strong>{displayAgent(agent.name)}</strong>
            <span class="side-badge side-{agent.side}">{sideLabel(agent.side)}</span>
          </div>
          <div class="connection-path" title={agent.path}>{agent.path}</div>
          {#if record?.limitation}<p class="connection-note">{record.limitation}</p>{/if}
        </div>
        <div class="connection-aside">
          <span class:connected={!!record} class="connection-state">{record ? '已连接' : '可连接'}</span>
          {#if record}
            <button class="secondary-button" onclick={() => disconnectAgent(agent.name, agent.side)}>断开</button>
          {:else}
            <button class="primary-button" onclick={() => connectAgent(agent)}>连接</button>
          {/if}
        </div>
      </article>
    {/snippet}
    {#snippet connectionList()}
      <div class="connection-list">
        {#each connectionAgents as agent (agent.side + ':' + agent.name)}
          {@render connectionCard(agent)}
        {/each}
      </div>
    {/snippet}
    {#if !onboardingComplete}
      <section class="panel-body onboarding" aria-label="初次设置">
        <p class="onboarding-step">{onboardingStepIndex(onboardingStep)} / {ONBOARDING_STEPS.length}</p>
        {#if onboardingStep === 'theme'}
          <div class="onboarding-copy">
            <h2>选一个外观</h2>
            <p>点一下即可预览，之后仍可在设置里改。</p>
          </div>
          {@render themePicker()}
        {:else if onboardingStep === 'connect'}
          <div class="onboarding-copy">
            <h2>连接一个工具</h2>
            <p>只接本机已经装好的。确认前会列出将要改的文件，每一步都可以跳过。</p>
          </div>
          {@render connectionsToolbar()}
          <div class="onboarding-main">
            {#if showDetectingPlaceholder(inventory, inventoryRefreshing)}
              <div class="empty compact"><span>…</span><p>正在检测本机工具</p></div>
            {:else if connectionAgents.length === 0}
              <div class="empty compact"><span>○</span><p>没有检测到支持的工具</p><small>可点「从文件夹添加」，或先跳过、稍后在连接页再接。</small></div>
            {:else}
              {@render connectionList()}
            {/if}
            {#if connectionError}<p class="error-message">{connectionError}</p>{/if}
          </div>
        {:else}
          <div class="onboarding-copy">
            <h2>建议用 orb run 启动</h2>
            <p>在新的 Windows Terminal 标签里运行，点返回箭头才能精确回到那个标签。</p>
          </div>
          <code class="onboarding-code">orb run grok</code>
          <p class="onboarding-note">claude、codex 同理。也可以在设置里起短命令{runAlias ? `，比如 ${runAlias} grok` : '，例如 or grok'}。</p>
        {/if}
        <div class="onboarding-actions">
          <button class="text-button" onclick={skipOnboardingStep}>跳过</button>
          {#if onboardingStep === 'run'}
            <button class="primary-button" onclick={finishOnboarding}>完成</button>
          {:else}
            <button class="primary-button" onclick={skipOnboardingStep}>下一步</button>
          {/if}
        </div>
      </section>
    {:else if page === 'activity'}
      <nav class="filters" aria-label="筛选任务">
        <button aria-pressed={filter === 'all'} class:active={filter === 'all'} onclick={() => (filter = 'all')}>全部 <span>{snapshot.tracked_count}</span></button>
        <button aria-pressed={filter === 'working'} class:active={filter === 'working'} onclick={() => (filter = 'working')}>工作中 <span>{snapshot.working_count}</span></button>
        <button aria-pressed={filter === 'attention'} class:active={filter === 'attention'} onclick={() => (filter = 'attention')}>未工作 <span>{snapshot.pending_count}</span></button>
      </nav>
      <div class="panel-body">
      {#if showAutostartHint}
        <div class="hint-banner" role="note">
          <p><strong>建议开启开机自启</strong><small>OrbCue 保持运行才能收到任务状态；开启后登录 Windows 即自动待命</small></p>
          <div class="hint-actions">
            <button class="primary-button" onclick={() => void acceptAutostartHint()}>开启</button>
            <button class="text-button" onclick={dismissAutostartHint}>不再提示</button>
          </div>
        </div>
      {/if}
      <div class="sessions">
        {#if visibleSessions.length === 0}
          <div class="empty"><span>✓</span><p>{filter === 'all' ? '还没有追踪中的任务' : '没有符合条件的任务'}</p><small>{EMPTY_TRACKING_HINT}</small></div>
        {:else}
          {#each sessionGroups as group (group.key)}
            <section class="project-group">
              <button
                class="project-heading"
                class:collapsed={collapsedGroups[group.key]}
                title={group.key || undefined}
                aria-expanded={!collapsedGroups[group.key]}
                onclick={() => toggleGroup(group.key)}
              >
                <span>{group.label}</span>
                <span class="chevron" aria-hidden="true"></span>
              </button>
              {#if !collapsedGroups[group.key]}
              {#each group.rows as row (sessionDomKey(row.session))}
                {@const session = row.session}
                <article
                  class:unread={session.mark === '?' || session.mark === '!'}
                  class:highlighted={highlightedKey === sessionHighlightKey(session.source, session.session_id)}
                  class="session-card {session.state}"
                  title={session.session_id}
                >
                  <div class="ticket-rail {session.state}" aria-hidden="true"></div>
                  <i class="led {session.state}" aria-hidden="true"></i>
                  <div class="session-content">
                    <div class="session-topline">
                      <span class="session-heading">
                        <strong>{row.title}</strong>
                        <span class="session-index">{row.index}</span>
                      </span>
                      <span class="state-chip {session.state}">{stateLabel(session)}</span>
                    </div>
                    <div class="session-actions">
                      {#if !session.acknowledged}<button onclick={() => acknowledge(session.source, session.session_id, session.terminal_id)}>已读</button>{/if}
                      <button onclick={() => resetSession(session.source, session.session_id, session.terminal_id)}>清除</button>
                    </div>
                    {#if focusNotes[sessionDomKey(session)]}<p class="session-focus-note">{focusNotes[sessionDomKey(session)]}</p>{/if}
                    {#if focusErrors[sessionDomKey(session)]}<p class="session-focus-error">{focusErrors[sessionDomKey(session)]}</p>{/if}
                  </div>
                  <button
                    class="jump-btn"
                    class:precise={isDockTerminalId(session.terminal_id)}
                    onclick={() => jumpBack(session)}
                    aria-label={isDockTerminalId(session.terminal_id) ? '精确跳回' : '回到最近交互的窗口'}
                    title={isDockTerminalId(session.terminal_id) ? '精确跳回' : '回到最近交互的窗口（不保证精确）'}
                  >
                    <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
                      <path d="M5.5 4.5 2 8l3.5 3.5M2.5 8H9a4 4 0 0 0 4-4V3.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
                    </svg>
                  </button>
                </article>
              {/each}
              {/if}
            </section>
          {/each}
        {/if}
      </div>
      </div>
      <footer>
        <button class="text-button" onclick={() => acknowledge('*', '*')} disabled={unread === 0}>全部已读</button>
        <button class="text-button danger" onclick={() => resetSession('*', '*')} disabled={snapshot.tracked_count === 0}>清除全部</button>
      </footer>
    {:else if page === 'audit'}
      <p class="section-intro">完成、失败、等待和关闭。</p>
      <div class="panel-body">
      <div class="audit-list">
        {#if auditRows.length === 0}
          <div class="empty compact"><span>✓</span><p>还没有审计记录</p><small>完成、失败、等待或关闭后会显示在这里</small></div>
        {:else}
          {#each auditRows as row, index (row.entry.source + ':' + row.entry.session_id + ':' + row.entry.occurred_at + ':' + index)}
            <article class="audit-card">
              <div class="ticket-rail {row.entry.state}" aria-hidden="true"></div>
              <i class="led {row.entry.state}" aria-hidden="true"></i>
              <div class="audit-content" title={`${row.entry.session_id} ${row.entry.occurred_at}`}>
                <div class="session-topline">
                  <span class="session-heading">
                    <strong>{row.title}</strong>
                    {#if row.index}<span class="session-index">{row.index}</span>{/if}
                  </span>
                  <time datetime={row.entry.occurred_at}>{formatAuditTime(row.entry.occurred_at)}</time>
                </div>
                <div class="audit-meta">
                  <span>{stateLabel(row.entry)}</span>
                  {#if row.project}<span class="audit-project" title={row.entry.project_path}>{row.project}</span>{/if}
                </div>
              </div>
            </article>
          {/each}
        {/if}
      </div>
      </div>
    {:else if page === 'connections'}
      <p class="section-intro">{CONNECTIONS_INTRO}</p>
      {@render connectionsToolbar()}
      {#if wslDockErrorBanner(inventory)}
        <p class="error-message">{wslDockErrorBanner(inventory)}</p>
      {/if}
      {#if connectSuccess}
        <div class="hint-banner" role="status">
          <p>{connectSuccess}</p>
          <div class="hint-actions">
            <button class="text-button" onclick={() => (connectSuccess = '')} aria-label="关闭提示">×</button>
          </div>
        </div>
      {/if}
      {#if showDetectingPlaceholder(inventory, inventoryRefreshing)}
        <div class="empty compact"><span>…</span><p>正在检测本机 Agent</p></div>
      {:else if connectionAgents.length === 0}
        <div class="empty compact"><span>○</span><p>没有检测到支持的工具</p><small>目前支持 Claude、Grok、Codex 和 Cursor。可点「从文件夹添加」。没有 WSL 也可以只连 Windows 上的工具</small></div>
      {:else}
        <div class="panel-body">
        {@render connectionList()}
        </div>
      {/if}
      {#if connectionError}<p class="error-message">{connectionError}</p>{/if}
    {:else}
      <p class="section-intro">默认保持安静，只在任务真正需要你回来时提醒一次。</p>
      <div class="panel-body">
      {@render themePicker()}
      <div class="settings-list">
        <div class="setting-row alias-row">
          <span>
            <strong>启动别名</strong>
            <small>把 orb run 收成短命令，空则删除</small>
          </span>
          <form onsubmit={saveRunAlias}>
            <input bind:value={runAliasDraft} maxlength="24" spellcheck="false" autocapitalize="off" autocomplete="off" placeholder="or" aria-label="启动别名" />
            <button type="submit" class="secondary-button">应用</button>
          </form>
        </div>
        {#if runAliasError}<p class="alias-hint error">{runAliasError}</p>
        {:else if runAliasHint}<p class="alias-hint">{runAliasHint}</p>{/if}
        {#snippet switchRow(pressed: boolean, title: string, hint: string, onclick: () => void)}
          <button class="setting-row" aria-pressed={pressed} {onclick}>
            <span><strong>{title}</strong><small>{hint}</small></span><span class:enabled={pressed} class="switch"><i></i></span>
          </button>
        {/snippet}
        {@render switchRow(replaceTabOnRun, '启动时替换当前标签页', 'orb run 开出新标签后关掉当前这个', () => void toggleReplaceTab())}
        {@render switchRow(hideBallBadge, '隐藏圆标', '小球右上角的 ? / ! 不再显示', toggleHideBallBadge)}
        {@render switchRow(sideDockEnabled, '收到侧边', '拖到屏幕边缘贴成半圆，悬停展开', toggleSideDock)}
        {@render switchRow(soundEnabled.completion, '完成提示音', '任务正常完成时播放短音', () => toggleSound('completion'))}
        {@render switchRow(soundEnabled.attention, '等待提示音', '等待输入或授权时播放短音', () => toggleSound('attention'))}
        {@render switchRow(soundEnabled.failure, '失败提示音', '任务失败时播放较低音调', () => toggleSound('failure'))}
        {@render switchRow(notificationsEnabled, '系统通知', '等待输入、授权或失败时弹出一次；已完成只走提示音', () => void toggleNotifications())}
        {@render switchRow(autostartEnabled, '开机自启', '登录 Windows 后自动打开 OrbCue，不必先手动启动才能接收 Agent 状态', () => void toggleAutostart())}
        {@render switchRow(shortcutEnabled, '全局快捷键', `${shortcut} 打开或收起任务面板`, () => void toggleShortcut())}
      </div>
      </div>
      <div class="privacy-note"><strong>本地与隐私优先</strong><p>OrbCue 默认不联网，不读取 transcript、prompt、命令或代码；持久化状态也不包含摘要。</p></div>
    {/if}
    {#if onboardingComplete}
    <nav class="dock-nav" aria-label="OrbCue 页面">
      <button aria-pressed={page === 'activity'} class:active={page === 'activity'} onclick={() => selectPage('activity')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M1.5 8.5h2.3l1.5-4.2 2.6 8.4L10 8.5h4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </span>
        动态
      </button>
      <button aria-pressed={page === 'audit'} class:active={page === 'audit'} onclick={() => selectPage('audit')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M3.5 4.5h9M3.5 8h9M3.5 11.5h6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
        </span>
        审计
      </button>
      <button aria-pressed={page === 'connections'} class:active={page === 'connections'} onclick={() => selectPage('connections')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M6.7 9.3 4.6 11.4a2 2 0 0 0 2.8 2.8l2.1-2.1M9.3 6.7l2.1-2.1a2 2 0 0 0-2.8-2.8L6.5 3.9M6.4 9.6l3.2-3.2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        </span>
        连接
      </button>
      <button aria-pressed={page === 'settings'} class:active={page === 'settings'} onclick={() => selectPage('settings')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M3 4.5h10M3 8h10M3 11.5h10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><circle cx="6.2" cy="4.5" r="1.45" fill="currentColor"/><circle cx="10.2" cy="8" r="1.45" fill="currentColor"/><circle cx="7.4" cy="11.5" r="1.45" fill="currentColor"/></svg>
        </span>
        设置
      </button>
    </nav>
    {/if}
    {#if pendingAgent}
      <div class="modal-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && closeConnectDialog()}>
        <dialog open class="confirm-dialog" aria-labelledby="connect-title">
          <h2 id="connect-title">连接 {displayAgent(pendingAgent.name)}<span class="side-badge side-{pendingAgent.side}">{sideLabel(pendingAgent.side)}</span></h2>
          <p>OrbCue 将在 {sideLabel(pendingAgent.side)} 侧使用现有可执行文件：</p>
          <code>{pendingAgent.path}</code>
          {#if previewLoading}
            <p class="dialog-note">正在生成预览</p>
          {:else if previewError}
            <p class="error-message">{previewError}</p>
          {:else if connectionPreview}
            <div class="preview-block">
              {#each connectionPreview.files as file (file.path)}
                <div class="preview-file">
                  <strong title={file.path}>{file.action} {file.path}</strong>
                  {#if file.entries.length > 0}
                    <ul class="preview-entries">
                      {#each file.entries as entry (entry)}<li>{entry}</li>{/each}
                    </ul>
                  {/if}
                </div>
              {/each}
              <ul class="preview-will-not">
                {#each connectionPreview.will_not as line (line)}<li>{line}</li>{/each}
              </ul>
              {#each connectionPreview.warnings ?? [] as warning (warning)}
                <p class="dialog-warning">{warning}</p>
              {/each}
              {#each connectionPreview.notes as note (note)}
                <p class="dialog-note">{note}</p>
              {/each}
            </div>
          {/if}
          <div class="dialog-actions">
            <button class="secondary-button" onclick={closeConnectDialog}>取消</button>
            <button class="primary-button" onclick={confirmConnect} disabled={previewLoading || !connectionPreview}>确认连接</button>
          </div>
        </dialog>
      </div>
    {/if}
  </main>
