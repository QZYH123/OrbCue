<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { currentMonitor, getCurrentWindow, PhysicalPosition, primaryMonitor } from '@tauri-apps/api/window';
  import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { invoke } from '@tauri-apps/api/core';
  import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutostartEnabled } from '@tauri-apps/plugin-autostart';
  import { isRegistered, register, unregister } from '@tauri-apps/plugin-global-shortcut';
  import { onMount } from 'svelte';
  import type { AgentInventory, AuditEntry, DiscoveredAgent, FocusResult, SessionSnapshot, Snapshot, SnapshotMessage } from './types';
  import { emptySnapshot } from './types';
  import { groupSessionsByProject, shortenProjectPath } from './projectPath';

  let label = 'ball';
  let snapshot: Snapshot = emptySnapshot;
  let pulse = false;
  let filter: 'all' | 'attention' | 'working' = 'all';
  let page: 'activity' | 'audit' | 'connections' | 'settings' = 'activity';
  let inventory: AgentInventory = { discovered: [], connected: [] };
  let inventoryLoading = false;
  let connectionError = '';
  let pendingAgent: DiscoveredAgent | null = null;
  let onboardingComplete = localStorage.getItem('onboarding-complete') === 'true';
  let completionSoundEnabled = localStorage.getItem('completion-sound-enabled') !== 'false';
  let attentionSoundEnabled = localStorage.getItem('attention-sound-enabled') !== 'false';
  let failureSoundEnabled = localStorage.getItem('failure-sound-enabled') !== 'false';
  let autostartEnabled = false;
  let shortcutEnabled = localStorage.getItem('shortcut-enabled') !== 'false';
  const shortcut = 'CommandOrControl+Shift+Space';
  let unsubscribe: (() => void) | undefined;
  let dragging = false;
  let suppressClick = false;
  let dragArmed = false;
  let dragStart = { x: 0, y: 0 };
  let snapTimer: number | undefined;
  let focusErrors: Record<string, string> = {};

  $: isBall = label === 'ball';
  $: visibleSessions = snapshot.sessions.filter((session) => {
    if (filter === 'attention') return session.state !== 'working';
    if (filter === 'working') return session.state === 'working';
    return true;
  });
  $: sessionGroups = groupSessionsByProject(visibleSessions);
  $: connectionAgents = [
    ...inventory.discovered,
    ...inventory.connected
      .filter((record) => !inventory.discovered.some((agent) => agent.name === record.name))
      .map((record) => ({ name: record.name, path: record.original })),
  ];

  onMount(() => {
    label = getCurrentWindow().label;
    let active = true;
    void (async () => {
      try {
        snapshot = await invoke<Snapshot>('snapshot');
      } catch (error) {
        console.warn('Dock snapshot unavailable during startup', error);
      }
      try {
        autostartEnabled = await isAutostartEnabled();
        // Both windows mount this component. Register the process-wide
        // shortcut from the persistent ball window only; the panel can still
        // toggle it explicitly from Settings.
        if (label === 'ball') {
          if (shortcutEnabled && !(await isRegistered(shortcut))) {
            await register(shortcut, onShortcut);
          } else if (!shortcutEnabled && (await isRegistered(shortcut))) {
            await unregister(shortcut);
          }
        } else {
          shortcutEnabled = await isRegistered(shortcut);
        }
      } catch (error) {
        console.warn('Desktop preferences are unavailable', error);
      }
      if (label !== 'ball') {
        await loadAgents();
        if (!onboardingComplete && inventory.discovered.length > 0) page = 'connections';
      }
      const stopListening = await listen<SnapshotMessage>('dock:snapshot', (event) => {
        if (!active) return;
        snapshot = event.payload.snapshot;
        if (event.payload.attention && label === 'ball') {
          pulse = true;
          window.setTimeout(() => (pulse = false), 280);
          playChime(event.payload.attention.severity);
        }
      });
      if (active) unsubscribe = stopListening;
      else stopListening();
      if (label === 'ball') {
        const stopMoved = await getCurrentWindow().onMoved(() => {
          if (!dragArmed) return;
          window.clearTimeout(snapTimer);
          snapTimer = window.setTimeout(() => {
            dragArmed = false;
            void snapBallToEdge();
          }, 280);
        });
        if (active) {
          const previous = unsubscribe;
          unsubscribe = () => {
            previous?.();
            stopMoved();
          };
        } else {
          stopMoved();
        }
      }
    })();
    return () => {
      active = false;
      window.clearTimeout(snapTimer);
      unsubscribe?.();
    };
  });

  async function openPanel() {
    try {
      await invoke('open_panel');
    } catch (error) {
      const panel = await WebviewWindow.getByLabel('panel');
      await panel?.show();
      await panel?.setFocus();
      console.warn('Could not position Dock panel', error);
    }
  }

  async function onBallPointerDown(event: PointerEvent) {
    dragging = false;
    suppressClick = false;
    dragStart = { x: event.screenX, y: event.screenY };
  }

  async function onBallPointerMove(event: PointerEvent) {
    if (event.buttons === 0 || dragging) return;
    const dx = event.screenX - dragStart.x;
    const dy = event.screenY - dragStart.y;
    if (dx * dx + dy * dy < 25) return;
    dragging = true;
    suppressClick = true;
    dragArmed = true;
    try {
      await getCurrentWindow().startDragging();
    } catch (error) {
      dragging = false;
      suppressClick = false;
      dragArmed = false;
      console.warn('Could not drag Dock ball', error);
    }
  }

  function onBallClick(event: MouseEvent) {
    if (suppressClick) {
      event.preventDefault();
      suppressClick = false;
      return;
    }
    void openPanel();
  }

  async function snapBallToEdge() {
    try {
      const win = getCurrentWindow();
      const monitor = (await currentMonitor()) ?? (await primaryMonitor());
      if (!monitor) return;
      const pos = await win.outerPosition();
      const size = await win.outerSize();
      const area = monitor.workArea ?? { position: monitor.position, size: monitor.size };
      const margin = Math.round(16 * monitor.scaleFactor);
      const minX = area.position.x + margin;
      const minY = area.position.y + margin;
      const maxX = area.position.x + area.size.width - size.width - margin;
      const maxY = area.position.y + area.size.height - size.height - margin;
      const midX = pos.x + size.width / 2;
      const center = area.position.x + area.size.width / 2;
      const x = Math.round(midX < center ? minX : Math.max(minX, maxX));
      const y = Math.round(Math.min(Math.max(pos.y, minY), Math.max(minY, maxY)));
      await win.setPosition(new PhysicalPosition(x, y));
    } catch (error) {
      console.warn('Could not snap Dock ball', error);
    }
  }

  async function selectPage(next: typeof page) {
    page = next;
    if (next === 'connections') await loadAgents();
  }

  async function loadAgents() {
    inventoryLoading = true;
    connectionError = '';
    try {
      inventory = await invoke<AgentInventory>('agent_inventory');
    } catch (error) {
      connectionError = String(error);
    } finally {
      inventoryLoading = false;
    }
  }

  function connected(name: string) {
    return inventory.connected.find((record) => record.name === name);
  }

  async function connectAgent(agent: DiscoveredAgent) {
    pendingAgent = agent;
    connectionError = '';
  }

  async function confirmConnect() {
    if (!pendingAgent) return;
    connectionError = '';
    try {
      await invoke('connect_agent', { name: pendingAgent.name, original: pendingAgent.path });
      pendingAgent = null;
      await loadAgents();
      if (inventory.connected.length > 0) {
        onboardingComplete = true;
        localStorage.setItem('onboarding-complete', 'true');
      }
    } catch (error) {
      connectionError = String(error);
    }
  }

  function skipOnboarding() {
    onboardingComplete = true;
    localStorage.setItem('onboarding-complete', 'true');
    page = 'activity';
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && pendingAgent) pendingAgent = null;
  }

  async function disconnectAgent(name: string) {
    connectionError = '';
    try {
      await invoke('disconnect_agent', { name });
      await loadAgents();
    } catch (error) {
      connectionError = String(error);
    }
  }

  async function toggleAutostart() {
    try {
      if (autostartEnabled) await disableAutostart();
      else await enableAutostart();
      autostartEnabled = await isAutostartEnabled();
    } catch (error) {
      console.warn('Could not update autostart', error);
    }
  }

  async function toggleShortcut() {
    try {
      if (shortcutEnabled) {
        await unregister(shortcut);
        shortcutEnabled = false;
      } else {
        await register(shortcut, onShortcut);
        shortcutEnabled = true;
      }
      localStorage.setItem('shortcut-enabled', String(shortcutEnabled));
    } catch (error) {
      console.warn('Could not update shortcut', error);
    }
  }

  function toggleSound(channel: 'completion' | 'attention' | 'failure') {
    if (channel === 'completion') {
      completionSoundEnabled = !completionSoundEnabled;
      localStorage.setItem('completion-sound-enabled', String(completionSoundEnabled));
    } else if (channel === 'attention') {
      attentionSoundEnabled = !attentionSoundEnabled;
      localStorage.setItem('attention-sound-enabled', String(attentionSoundEnabled));
    } else {
      failureSoundEnabled = !failureSoundEnabled;
      localStorage.setItem('failure-sound-enabled', String(failureSoundEnabled));
    }
  }

  async function closePanel() {
    await getCurrentWindow().hide();
  }

  function onShortcut(event: { state: 'Released' | 'Pressed' }) {
    if (event.state === 'Pressed') void openPanel();
  }

  async function acknowledge(source: string, sessionId: string) {
    try {
      snapshot = await invoke<Snapshot>('acknowledge', { source, sessionId });
    } catch (error) {
      console.warn('Could not acknowledge session', error);
    }
  }

  async function resetSession(source: string, sessionId: string) {
    try {
      snapshot = await invoke<Snapshot>('reset', { source, sessionId });
    } catch (error) {
      console.warn('Could not reset session', error);
    }
  }

  function focusErrorKey(source: string, sessionId: string) {
    return `${source}\0${sessionId}`;
  }

  async function jumpBack(session: SessionSnapshot) {
    const key = focusErrorKey(session.source, session.session_id);
    try {
      const result = await invoke<FocusResult>('focus_source', {
        source: session.source,
        sessionId: session.session_id,
      });
      if (result.focused) {
        const next = { ...focusErrors };
        delete next[key];
        focusErrors = next;
        return;
      }
      focusErrors = { ...focusErrors, [key]: result.reason ?? '无法跳回' };
    } catch (error) {
      focusErrors = { ...focusErrors, [key]: String(error) };
    }
  }

  function markClass(mark: string) {
    if (mark === '!') return 'fail';
    if (mark === '?') return 'wait';
    if (mark === '*') return 'done';
    if (mark === 'x') return 'cancel';
    return 'idle';
  }

  function stateLabel(session: SessionSnapshot) {
    if (session.state === 'idle') return '空闲';
    if (session.state === 'working') return '工作中';
    if (session.state === 'needs_attention') {
      return session.attention_reason === 'permission' ? '等待授权' : '等待输入';
    }
    if (session.state === 'failed') return '失败';
    if (session.state === 'completed') return '已完成';
    return '已取消';
  }

  function auditStateLabel(entry: AuditEntry) {
    if (entry.state === 'idle') return '空闲';
    if (entry.state === 'working') return '工作中';
    if (entry.state === 'needs_attention') {
      return entry.attention_reason === 'permission' ? '等待授权' : '等待输入';
    }
    if (entry.state === 'failed') return '失败';
    if (entry.state === 'completed') return '已完成';
    return '已取消';
  }

  function formatAuditTime(value: string) {
    const time = new Date(value);
    return Number.isNaN(time.getTime()) ? value : time.toLocaleString();
  }

  function playChime(severity: 'info' | 'attention' | 'error') {
    if (severity === 'info' && !completionSoundEnabled) return;
    if (severity === 'attention' && !attentionSoundEnabled) return;
    if (severity === 'error' && !failureSoundEnabled) return;
    const AudioContextClass = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextClass) return;
    const context = new AudioContextClass();
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    oscillator.type = 'sine';
    oscillator.frequency.value = severity === 'error' ? 460 : severity === 'attention' ? 620 : 780;
    gain.gain.setValueAtTime(0.08, context.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, context.currentTime + 0.18);
    oscillator.connect(gain).connect(context.destination);
    oscillator.start();
    oscillator.stop(context.currentTime + 0.2);
    window.setTimeout(() => void context.close(), 260);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isBall}
  <main
    class="ball-shell"
    class:attention={snapshot.pending_mark === '?' || snapshot.pending_mark === '!'}
    class:pulse
    class:working={snapshot.working_count > 0}
    aria-label="Agent Activity Dock"
  >
    <button
      class="ball"
      class:working={snapshot.working_count > 0}
      class:wide={snapshot.count_label.length > 4}
      title={`${snapshot.count_label}，${snapshot.pending_mark || (snapshot.working_count ? '工作中' : '空闲')}`}
      onpointerdown={onBallPointerDown}
      onpointermove={onBallPointerMove}
      onclick={onBallClick}
    >
      <span class="ball-face" aria-hidden="true"></span>
      <span class="count">{snapshot.count_label}</span>
    </button>
    <!-- Rendered outside the button: overflow clipping on the circle would cut it off. -->
    {#if snapshot.pending_mark}<span class="badge mark-{markClass(snapshot.pending_mark)}" aria-label={snapshot.pending_mark}>{snapshot.pending_mark}</span>{/if}
  </main>
{:else}
  <main class="panel" aria-label="Agent Activity Dock 任务列表">
    <header class="panel-header">
      <div>
        <p class="eyebrow">AGENT ACTIVITY DOCK</p>
        <h1>{page === 'activity' ? '任务状态' : page === 'audit' ? '审计记录' : page === 'connections' ? 'Agent 连接' : '偏好设置'}</h1>
      </div>
      <button class="icon-button" onclick={closePanel} aria-label="关闭">×</button>
    </header>
    <nav class="primary-tabs" aria-label="Dock 页面">
      <button aria-pressed={page === 'activity'} class:active={page === 'activity'} onclick={() => selectPage('activity')}>动态</button>
      <button aria-pressed={page === 'audit'} class:active={page === 'audit'} onclick={() => selectPage('audit')}>审计</button>
      <button aria-pressed={page === 'connections'} class:active={page === 'connections'} onclick={() => selectPage('connections')}>连接</button>
      <button aria-pressed={page === 'settings'} class:active={page === 'settings'} onclick={() => selectPage('settings')}>设置</button>
    </nav>
    {#if page === 'activity'}
      <section class="summary" aria-live="polite">
        <div class="summary-count">{snapshot.count_label}</div>
        <div><strong>{snapshot.working_count ? '正在工作' : '当前空闲'}</strong><small>{snapshot.pending_count ? `${snapshot.pending_count} 项未在工作` : '全部在工作或没有打开中的会话'}</small></div>
      </section>
      <nav class="filters" aria-label="筛选任务">
        <button aria-pressed={filter === 'all'} class:active={filter === 'all'} onclick={() => (filter = 'all')}>全部 <span>{snapshot.tracked_count}</span></button>
        <button aria-pressed={filter === 'working'} class:active={filter === 'working'} onclick={() => (filter = 'working')}>工作中 <span>{snapshot.working_count}</span></button>
        <button aria-pressed={filter === 'attention'} class:active={filter === 'attention'} onclick={() => (filter = 'attention')}>未工作 <span>{snapshot.pending_count}</span></button>
      </nav>
      <div class="sessions">
        {#if visibleSessions.length === 0}
          <div class="empty"><span>✓</span><p>{filter === 'all' ? '还没有追踪中的任务' : '没有符合条件的任务'}</p><small>Agent 发出事件后会显示在这里</small></div>
        {:else}
          {#each sessionGroups as group (group.key)}
            <section class="project-group">
              <h2 class="project-heading">{group.label}</h2>
              {#each group.sessions as session (session.source + ':' + session.session_id)}
                <article class:unread={session.mark === '?' || session.mark === '!'} class="session-card">
                  <div class="state-mark {session.state}" aria-hidden="true"></div>
                  <div class="session-content">
                    <div class="session-topline"><strong>{session.source}</strong><span>{stateLabel(session)}</span></div>
                    <div class="session-id" title={session.session_id}>{session.session_id}</div>
                    {#if session.project_path}<div class="session-path" title={session.project_path}>{shortenProjectPath(session.project_path)}</div>{/if}
                    {#if session.summary}<p>{session.summary}</p>{/if}
                    <div class="session-actions">
                      <button onclick={() => jumpBack(session)}>回去</button>
                      {#if !session.acknowledged}<button onclick={() => acknowledge(session.source, session.session_id)}>标记已查看</button>{/if}
                      <button onclick={() => resetSession(session.source, session.session_id)}>清除</button>
                    </div>
                    {#if focusErrors[focusErrorKey(session.source, session.session_id)]}<p class="session-focus-error">{focusErrors[focusErrorKey(session.source, session.session_id)]}</p>{/if}
                  </div>
                  {#if session.mark}<span class="unread-mark mark-{markClass(session.mark)}">{session.mark}</span>{/if}
                </article>
              {/each}
            </section>
          {/each}
        {/if}
      </div>
      <footer>
        <button class="text-button" onclick={() => acknowledge('*', '*')} disabled={snapshot.pending_count === 0}>全部标记已查看</button>
        <button class="text-button danger" onclick={() => resetSession('*', '*')} disabled={snapshot.tracked_count === 0}>清除全部状态</button>
      </footer>
    {:else if page === 'audit'}
      <p class="section-intro">仅显示状态变更的来源、会话、状态和时间。</p>
      <div class="audit-list">
        {#if snapshot.audit.length === 0}
          <div class="empty compact"><span>✓</span><p>还没有审计记录</p><small>任务状态发生变化后会显示在这里</small></div>
        {:else}
          {#each [...snapshot.audit].reverse() as entry, index (entry.source + ':' + entry.session_id + ':' + entry.occurred_at + ':' + index)}
            <article class="audit-card">
              <div class="state-mark {entry.state}" aria-hidden="true"></div>
              <div class="audit-content">
                <div class="session-topline"><strong>{entry.source}</strong><time datetime={entry.occurred_at}>{formatAuditTime(entry.occurred_at)}</time></div>
                <div class="session-id" title={entry.session_id}>{entry.session_id}</div>
                <div class="audit-meta"><span>{auditStateLabel(entry)}</span>{#if entry.attention_reason}<span>{entry.attention_reason === 'permission' ? '授权请求' : '需要输入'}</span>{/if}</div>
              </div>
            </article>
          {/each}
        {/if}
      </div>
    {:else if page === 'connections'}
      <p class="section-intro">只连接本机已有的 Agent。Dock 不会下载、替换或读取它们的工作内容。</p>
      {#if !onboardingComplete}
        <div class="onboarding-banner">
          <strong>先连接一个已有的 Agent</strong>
          <p>连接只会安装可撤销的用户级 Hook 或 wrapper，并保留原命令和参数。</p>
          <button class="text-button" onclick={skipOnboarding}>稍后设置</button>
        </div>
      {/if}
      {#if inventoryLoading}
        <div class="empty compact"><span>…</span><p>正在检测本机 Agent</p></div>
      {:else if connectionAgents.length === 0}
        <div class="empty compact"><span>○</span><p>PATH 中没有检测到支持的 Agent</p><small>目前支持 Claude、Grok Build、Codex 和 DSH</small></div>
      {:else}
        <div class="connection-list">
          {#each connectionAgents as agent (agent.name)}
            {@const record = connected(agent.name)}
            <article class="connection-card">
              <div class="agent-icon">{agent.name.slice(0, 1).toUpperCase()}</div>
              <div class="connection-content">
                <div class="session-topline"><strong>{agent.name}</strong><span class:connected={record}>{record ? '已连接' : '可连接'}</span></div>
                <div class="session-id" title={agent.path}>{agent.path}</div>
                {#if record}<p>{record.limitation}</p>{/if}
              </div>
              {#if record}
                <button class="secondary-button" onclick={() => disconnectAgent(agent.name)}>断开</button>
              {:else}
                <button class="primary-button" onclick={() => connectAgent(agent)}>连接</button>
              {/if}
            </article>
          {/each}
        </div>
      {/if}
      {#if connectionError}<p class="error-message">{connectionError}</p>{/if}
      {#if pendingAgent}
        <div class="modal-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && (pendingAgent = null)}>
          <dialog open class="confirm-dialog" aria-labelledby="connect-title">
            <h2 id="connect-title">连接 {pendingAgent.name}</h2>
            <p>Dock 将使用现有可执行文件：</p>
            <code>{pendingAgent.path}</code>
            <p class="dialog-note">不会读取 transcript、提示词、命令或代码。连接可以随时在此页面撤销。</p>
            <div class="dialog-actions">
              <button class="secondary-button" onclick={() => (pendingAgent = null)}>取消</button>
              <button class="primary-button" onclick={confirmConnect}>确认连接</button>
            </div>
          </dialog>
        </div>
      {/if}
    {:else}
      <p class="section-intro">默认保持安静，只在任务真正需要你回来时提醒一次。</p>
      <div class="settings-list">
        <button class="setting-row" aria-pressed={completionSoundEnabled} onclick={() => toggleSound('completion')}>
          <span><strong>完成提示音</strong><small>任务正常完成时播放短音</small></span><span class:enabled={completionSoundEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={attentionSoundEnabled} onclick={() => toggleSound('attention')}>
          <span><strong>等待提示音</strong><small>等待输入或授权时播放短音</small></span><span class:enabled={attentionSoundEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={failureSoundEnabled} onclick={() => toggleSound('failure')}>
          <span><strong>失败提示音</strong><small>任务失败时播放较低音调</small></span><span class:enabled={failureSoundEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={autostartEnabled} onclick={toggleAutostart}>
          <span><strong>登录后自动启动</strong><small>让 Agent 事件随时有接收端</small></span><span class:enabled={autostartEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={shortcutEnabled} onclick={toggleShortcut}>
          <span><strong>全局快捷键</strong><small>{shortcut} 打开任务面板</small></span><span class:enabled={shortcutEnabled} class="switch"><i></i></span>
        </button>
      </div>
      <div class="privacy-note"><strong>本地与隐私优先</strong><p>Dock 默认不联网，不读取 transcript、prompt、命令或代码；持久化状态也不包含摘要。</p></div>
    {/if}
  </main>
{/if}
