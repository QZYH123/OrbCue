<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { currentMonitor, getCurrentWindow, PhysicalPosition, primaryMonitor } from '@tauri-apps/api/window';
  import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { invoke } from '@tauri-apps/api/core';
  import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutostartEnabled } from '@tauri-apps/plugin-autostart';
  import { isRegistered, register, unregister } from '@tauri-apps/plugin-global-shortcut';
  import {
    isPermissionGranted,
    onAction,
    requestPermission,
  } from '@tauri-apps/plugin-notification';
  import { onMount } from 'svelte';
  import { playChime, unlockAudio, type ChimeSettings } from './chime';
  import { ensureNotificationPermission } from './notifications';
  import type { AgentInventory, AgentSide, ConnectionPreview, DiscoveredAgent, FocusResult, SessionSnapshot, Snapshot, SnapshotMessage } from './types';
  import { emptySnapshot } from './types';
  import {
    highlightFromNotificationExtra,
    projectGroupKey,
    revealHighlightedGroup,
    sessionHighlightKey,
  } from './highlight';
  import { connectSuccessNotice, inventoryHasRows, showDetectingPlaceholder, sideLabel, wslDockErrorBanner } from './inventory';
  import { clampToWorkArea, shouldHidePanelOnBallDrag } from './placement';
  import { CONNECTIONS_INTRO, EMPTY_TRACKING_HINT, isDockTerminalId, jumpFeedback } from './jumpBack';
  import { applyPreviewDocument, demoInventory, demoSnapshot, previewLabel, tauriAvailable } from './preview';
  import {
    displayAgent,
    filterSessionSections,
    formatAuditTime,
    presentAuditRows,
    presentSessionSections,
    sessionDomKey,
  } from './sessionIdentity';
  import { barTones, matrixTones } from './glyphMatrix';
  import {
    THEMES,
    THEME_META,
    initialTheme,
    persistTheme,
    subscribeTheme,
    type DockTheme,
  } from './theme';

  const previewMode = !tauriAvailable();
  let label: string = previewMode ? previewLabel() : 'ball';
  let snapshot: Snapshot = previewMode ? demoSnapshot : emptySnapshot;
  let pulse = false;
  let filter: 'all' | 'attention' | 'working' = 'all';
  let collapsedGroups: Record<string, boolean> = {};
  let page: 'activity' | 'audit' | 'connections' | 'settings' = 'activity';
  let inventory: AgentInventory = previewMode ? demoInventory : { discovered: [], connected: [] };
  let inventoryRefreshing = false;
  let onboardingChecked = false;
  let connectionError = '';
  let pendingAgent: DiscoveredAgent | null = null;
  let connectionPreview: ConnectionPreview | null = null;
  let previewLoading = false;
  let previewError = '';
  let onboardingComplete = localStorage.getItem('onboarding-complete') === 'true';
  let completionSoundEnabled = localStorage.getItem('completion-sound-enabled') !== 'false';
  let attentionSoundEnabled = localStorage.getItem('attention-sound-enabled') !== 'false';
  let failureSoundEnabled = localStorage.getItem('failure-sound-enabled') !== 'false';
  let notificationsEnabled = localStorage.getItem('notifications-enabled') !== 'false';
  let highlightedKey = '';
  let autostartEnabled = false;
  let autostartChecked = false;
  let autostartHintDismissed = localStorage.getItem('autostart-hint-dismissed') === 'true';
  let connectSuccess = '';
  let shortcutEnabled = localStorage.getItem('shortcut-enabled') !== 'false';
  let hideBallBadge = localStorage.getItem('orbcue-hide-ball-badge') === 'true';
  let theme: DockTheme = initialTheme();
  let runAlias = '';
  let runAliasDraft = '';
  let runAliasHint = '';
  let runAliasError = '';
  const shortcut = 'CommandOrControl+Shift+Space';
  const BADGE_KEY = 'orbcue-hide-ball-badge';
  let badgeChannel: BroadcastChannel | null = null;
  let unsubscribe: (() => void) | undefined;
  let dragging = false;
  let suppressClick = false;
  let dragArmed = false;
  let dragStart = { x: 0, y: 0 };
  let snapTimer: number | undefined;
  let focusErrors: Record<string, string> = {};
  let focusNotes: Record<string, string> = {};
  const focusNoteTimers: Record<string, number> = {};

  if (previewMode) applyPreviewDocument(label === 'panel' ? 'panel' : 'ball');

  $: isBall = label === 'ball';
  $: visibleSessions = snapshot.sessions.filter((session) => {
    if (filter === 'attention') return session.state !== 'working';
    if (filter === 'working') return session.state === 'working';
    return true;
  });
  $: sessionGroups = filterSessionSections(presentSessionSections(snapshot.sessions), visibleSessions);
  $: auditRows = presentAuditRows(snapshot.audit, snapshot.sessions);
  $: ringRatio = snapshot.tracked_count <= 0 ? 0 : snapshot.working_count / snapshot.tracked_count;
  $: ballKind =
    snapshot.pending_mark === '!'
      ? 'fail'
      : snapshot.pending_mark === '?'
        ? 'wait'
        : snapshot.working_count > 0
          ? 'working'
          : 'idle';
  $: matrixDots = theme === 'glyph' ? matrixTones(snapshot.working_count, snapshot.tracked_count, ballKind) : [];
  $: heroBar = theme === 'glyph' ? barTones(snapshot.working_count, snapshot.tracked_count, 8) : [];
  $: showAutostartHint =
    !previewMode && label === 'panel' && autostartChecked && !autostartEnabled && !autostartHintDismissed;
  $: connectionAgents = [
    ...inventory.discovered,
    ...inventory.connected
      .filter(
        (record) =>
          !inventory.discovered.some(
            (agent) => agent.name === record.name && agent.side === record.side,
          ),
      )
      .map((record) => ({
        name: record.name,
        path: record.original,
        side: record.side,
      })),
  ];

  function listenBadgePref() {
    if (typeof BroadcastChannel === 'undefined' || badgeChannel) return;
    badgeChannel = new BroadcastChannel(BADGE_KEY);
    badgeChannel.onmessage = (event) => {
      hideBallBadge = event.data === true;
    };
    window.addEventListener('storage', (event) => {
      if (event.key !== BADGE_KEY) return;
      hideBallBadge = event.newValue === 'true';
    });
  }

  onMount(() => {
    const unsubTheme = subscribeTheme((next) => {
      theme = next;
    });
    listenBadgePref();
    void loadRunAlias();
    if (previewMode) {
      const pageQ = new URLSearchParams(window.location.search).get('page');
      if (pageQ === 'audit' || pageQ === 'connections' || pageQ === 'settings' || pageQ === 'activity') {
        page = pageQ;
      }
      return unsubTheme;
    }
    label = getCurrentWindow().label;
    let active = true;
    void (async () => {
      await refreshSnapshot();
      try {
        autostartEnabled = await isAutostartEnabled();
        autostartChecked = true;
        // Both windows mount this component. Register the process-wide
        // shortcut from the persistent ball window only; the panel can still
        // toggle it explicitly from Settings.
        if (label === 'ball') {
          try {
            if (notificationsEnabled) {
              const granted = await ensureNotificationPermission(
                isPermissionGranted,
                requestPermission,
              );
              if (!granted) {
                console.warn('System notifications are not permitted');
              }
            }
            await invoke('set_notification_enabled', { enabled: notificationsEnabled });
          } catch (error) {
            console.warn('Could not sync notification preference', error);
          }
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
      const stopListening = await listen<SnapshotMessage>('orb:snapshot', (event) => {
        if (!active) return;
        snapshot = event.payload.snapshot;
        if (event.payload.attention && label === 'ball') {
          pulse = true;
          window.setTimeout(() => (pulse = false), 280);
          void playChime(event.payload.attention.severity, currentChimeSettings());
        }
      });
      const stopInventory = await listen<AgentInventory>('orb:inventory', (event) => {
        if (!active) return;
        inventory = event.payload;
        inventoryRefreshing = false;
        maybeOpenOnboarding();
      });
      const stopHighlight = await listen<{ source: string; session_id: string }>('orb:highlight', (event) => {
        if (!active || label === 'ball') return;
        page = 'activity';
        highlightedKey = sessionHighlightKey(event.payload.source, event.payload.session_id);
        const session = snapshot.sessions.find(
          (item) =>
            item.source === event.payload.source && item.session_id === event.payload.session_id,
        );
        collapsedGroups = revealHighlightedGroup(
          collapsedGroups,
          projectGroupKey(session?.project_path),
        );
      });
      let stopAction: { unregister: () => Promise<void> } = { unregister: async () => {} };
      let stopFocus = () => {};
      if (label === 'ball') {
        try {
          stopAction = await onAction((notification) => {
            const target = highlightFromNotificationExtra(notification.extra);
            if (target) {
              void invoke('activate_attention', {
                source: target.source,
                sessionId: target.session_id,
              });
              return;
            }
            void invoke('open_panel');
          });
        } catch (error) {
          console.warn('Could not listen for notification clicks', error);
        }
      }
      if (label !== 'ball') {
        void loadAgentsCached();
        try {
          stopFocus = await getCurrentWindow().onFocusChanged((event) => {
            if (event.payload) void refreshSnapshot();
          });
        } catch (error) {
          console.warn('Could not listen for panel focus', error);
        }
      }
      if (active) {
        unsubscribe = () => {
          stopListening();
          stopInventory();
          stopHighlight();
          stopFocus();
          void stopAction.unregister();
        };
      } else {
        stopListening();
        stopInventory();
        stopHighlight();
        stopFocus();
        void stopAction.unregister();
      }
      if (label === 'ball') {
        const stopMoved = await getCurrentWindow().onMoved(() => {
          if (!dragArmed) return;
          window.clearTimeout(snapTimer);
          snapTimer = window.setTimeout(() => {
            dragArmed = false;
            void clampBallToWorkArea();
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
      unsubTheme();
    };
  });

  async function refreshSnapshot() {
    try {
      snapshot = await invoke<Snapshot>('snapshot');
    } catch (error) {
      console.warn('Dock snapshot unavailable', error);
    }
  }

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

  async function togglePanel() {
    if (previewMode) {
      label = label === 'ball' ? 'panel' : 'ball';
      applyPreviewDocument(label === 'panel' ? 'panel' : 'ball');
      return;
    }
    try {
      await invoke('toggle_panel');
    } catch (error) {
      try {
        const panel = await WebviewWindow.getByLabel('panel');
        if (panel && (await panel.isVisible())) {
          await invoke('hide_panel');
          return;
        }
      } catch {
        // Fall through to open.
      }
      await openPanel();
      console.warn('Could not toggle Dock panel', error);
    }
  }

  async function onBallPointerDown(event: PointerEvent) {
    dragging = false;
    suppressClick = false;
    dragStart = { x: event.screenX, y: event.screenY };
    void unlockAudio();
  }

  async function onBallPointerMove(event: PointerEvent) {
    if (event.buttons === 0 || dragging) return;
    const dx = event.screenX - dragStart.x;
    const dy = event.screenY - dragStart.y;
    if (dx * dx + dy * dy < 25) return;
    dragging = true;
    suppressClick = true;
    dragArmed = true;
    if (shouldHidePanelOnBallDrag(true)) {
      try {
        await invoke('hide_panel');
      } catch {
        try {
          const panel = await WebviewWindow.getByLabel('panel');
          await panel?.hide();
        } catch (error) {
          console.warn('Could not hide Dock panel while dragging', error);
        }
      }
    }
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
    void togglePanel();
  }

  async function clampBallToWorkArea() {
    try {
      const win = getCurrentWindow();
      const monitor = (await currentMonitor()) ?? (await primaryMonitor());
      if (!monitor) return;
      const pos = await win.outerPosition();
      const size = await win.outerSize();
      const area = monitor.workArea ?? { position: monitor.position, size: monitor.size };
      const clamped = clampToWorkArea(
        { x: pos.x, y: pos.y, width: size.width, height: size.height },
        { x: area.position.x, y: area.position.y, width: area.size.width, height: area.size.height },
      );
      if (clamped.x === pos.x && clamped.y === pos.y) return;
      await win.setPosition(new PhysicalPosition(clamped.x, clamped.y));
    } catch (error) {
      console.warn('Could not keep Dock ball on screen', error);
    }
  }

  function maybeOpenOnboarding() {
    if (onboardingChecked || onboardingComplete || label === 'ball') return;
    if (inventory.discovered.length > 0) {
      page = 'connections';
      onboardingChecked = true;
    }
  }

  function toggleGroup(key: string) {
    collapsedGroups = { ...collapsedGroups, [key]: !collapsedGroups[key] };
  }

  function selectPage(next: typeof page) {
    onboardingChecked = true;
    if (next !== 'connections') connectSuccess = '';
    page = next;
    if (next === 'connections' && !previewMode) void loadAgentsCached();
  }

  async function loadAgentsCached() {
    if (previewMode) return;
    inventoryRefreshing = true;
    connectionError = '';
    try {
      inventory = await invoke<AgentInventory>('agent_inventory');
      maybeOpenOnboarding();
    } catch (error) {
      connectionError = String(error);
    } finally {
      if (inventoryHasRows(inventory)) inventoryRefreshing = false;
    }
  }

  async function refreshAgents() {
    if (previewMode) return;
    inventoryRefreshing = true;
    connectionError = '';
    try {
      inventory = await invoke<AgentInventory>('refresh_agents');
    } catch (error) {
      connectionError = String(error);
    } finally {
      inventoryRefreshing = false;
    }
  }

  async function addFromFolder() {
    if (previewMode) return;
    connectionError = '';
    try {
      inventory = await invoke<AgentInventory>('add_agent_folder');
    } catch (error) {
      connectionError = String(error);
    }
  }

  function connected(name: string, side: AgentSide) {
    return inventory.connected.find((record) => record.name === name && record.side === side);
  }

  async function connectAgent(agent: DiscoveredAgent) {
    pendingAgent = agent;
    connectionError = '';
    connectSuccess = '';
    connectionPreview = null;
    previewError = '';
    previewLoading = true;
    try {
      connectionPreview = await invoke<ConnectionPreview>('preview_connect', {
        name: agent.name,
        original: agent.path,
        side: agent.side,
      });
    } catch (error) {
      previewError = String(error);
    } finally {
      previewLoading = false;
    }
  }

  function closeConnectDialog() {
    pendingAgent = null;
    connectionPreview = null;
    previewError = '';
    previewLoading = false;
  }

  async function confirmConnect() {
    if (!pendingAgent || !connectionPreview) return;
    const agent = pendingAgent;
    connectionError = '';
    try {
      await invoke('connect_agent', {
        name: agent.name,
        original: agent.path,
        side: agent.side,
      });
      closeConnectDialog();
      connectSuccess = connectSuccessNotice(agent.name, agent.side);
      await refreshAgents();
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
    if (event.key === 'Escape' && pendingAgent) closeConnectDialog();
  }

  async function disconnectAgent(name: string, side: AgentSide) {
    connectionError = '';
    connectSuccess = '';
    try {
      await invoke('disconnect_agent', { name, side });
      await refreshAgents();
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

  function dismissAutostartHint() {
    autostartHintDismissed = true;
    localStorage.setItem('autostart-hint-dismissed', 'true');
  }

  async function acceptAutostartHint() {
    if (!autostartEnabled) await toggleAutostart();
    dismissAutostartHint();
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

  async function toggleNotifications() {
    if (notificationsEnabled) {
      notificationsEnabled = false;
      localStorage.setItem('notifications-enabled', 'false');
      try {
        await invoke('set_notification_enabled', { enabled: false });
      } catch (error) {
        console.warn('Could not update notification preference', error);
      }
      return;
    }
    const granted = await ensureNotificationPermission(isPermissionGranted, requestPermission);
    if (!granted) {
      console.warn('System notifications are not permitted');
      return;
    }
    notificationsEnabled = true;
    localStorage.setItem('notifications-enabled', 'true');
    try {
      await invoke('set_notification_enabled', { enabled: true });
      await invoke('preview_notification');
    } catch (error) {
      console.warn('Could not update notification preference', error);
    }
  }

  function currentChimeSettings(): ChimeSettings {
    return {
      completion: completionSoundEnabled,
      attention: attentionSoundEnabled,
      failure: failureSoundEnabled,
    };
  }

  function setTheme(next: DockTheme) {
    theme = next;
    persistTheme(next);
  }

  function toggleHideBallBadge() {
    hideBallBadge = !hideBallBadge;
    try {
      localStorage.setItem(BADGE_KEY, String(hideBallBadge));
    } catch {
      /* ignore quota */
    }
    listenBadgePref();
    badgeChannel?.postMessage(hideBallBadge);
  }

  async function loadRunAlias() {
    runAliasError = '';
    if (previewMode) {
      runAlias = localStorage.getItem('orbcue-run-alias') || '';
      runAliasDraft = runAlias;
      return;
    }
    try {
      const value = await invoke<string | null>('run_alias');
      runAlias = value || '';
      runAliasDraft = runAlias;
    } catch (error) {
      runAliasError = String(error);
    }
  }

  async function saveRunAlias(event: SubmitEvent) {
    event.preventDefault();
    const name = runAliasDraft.trim();
    runAliasError = '';
    runAliasHint = '';
    if (previewMode) {
      if (name) localStorage.setItem('orbcue-run-alias', name);
      else localStorage.removeItem('orbcue-run-alias');
      runAlias = name;
      runAliasHint = name ? `预览：${name} grok 等于 orb run grok` : '已清除别名';
      return;
    }
    try {
      const value = await invoke<string | null>('set_run_alias', { name });
      runAlias = value || '';
      runAliasDraft = runAlias;
      runAliasHint = runAlias ? `之后在新终端输入 ${runAlias} grok` : '已删除别名';
    } catch (error) {
      runAliasError = String(error);
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
    if (
      (channel === 'completion' && completionSoundEnabled) ||
      (channel === 'attention' && attentionSoundEnabled) ||
      (channel === 'failure' && failureSoundEnabled)
    ) {
      const severity =
        channel === 'completion' ? 'info' : channel === 'attention' ? 'attention' : 'error';
      void playChime(severity, currentChimeSettings());
    }
  }

  async function closePanel() {
    if (previewMode) {
      label = 'ball';
      applyPreviewDocument('ball');
      return;
    }
    await getCurrentWindow().hide();
  }

  function onShortcut(event: { state: 'Released' | 'Pressed' }) {
    if (event.state === 'Pressed') void togglePanel();
  }

  async function acknowledge(source: string, sessionId: string, terminalId?: string | null) {
    try {
      snapshot = await invoke<Snapshot>('acknowledge', { source, sessionId, terminalId });
    } catch (error) {
      console.warn('Could not acknowledge session', error);
    }
  }

  async function resetSession(source: string, sessionId: string, terminalId?: string | null) {
    try {
      snapshot = await invoke<Snapshot>('reset', { source, sessionId, terminalId });
    } catch (error) {
      console.warn('Could not reset session', error);
    }
  }

  function clearFocusNote(key: string) {
    if (focusNoteTimers[key]) {
      window.clearTimeout(focusNoteTimers[key]);
      delete focusNoteTimers[key];
    }
    if (focusNotes[key]) {
      const next = { ...focusNotes };
      delete next[key];
      focusNotes = next;
    }
  }

  function setFocusNote(key: string, text: string) {
    if (focusNoteTimers[key]) window.clearTimeout(focusNoteTimers[key]);
    focusNotes = { ...focusNotes, [key]: text };
    focusNoteTimers[key] = window.setTimeout(() => {
      const next = { ...focusNotes };
      delete next[key];
      focusNotes = next;
      delete focusNoteTimers[key];
    }, 2500);
  }

  async function jumpBack(session: SessionSnapshot) {
    const key = sessionHighlightKey(session.source, session.session_id);
    try {
      const result = await invoke<FocusResult>('focus_source', {
        source: session.source,
        sessionId: session.session_id,
        terminalId: session.terminal_id,
        deepLink: session.deep_link,
      });
      const feedback = jumpFeedback(result);
      if (feedback.kind === 'error') {
        clearFocusNote(key);
        focusErrors = { ...focusErrors, [key]: feedback.text };
        return;
      }
      const next = { ...focusErrors };
      delete next[key];
      focusErrors = next;
      if (feedback.kind === 'note') {
        setFocusNote(key, feedback.text);
      } else {
        clearFocusNote(key);
      }
    } catch (error) {
      clearFocusNote(key);
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

  function stateLabel(item: { state: SessionSnapshot['state']; attention_reason: string | null }) {
    if (item.state === 'idle') return '空闲';
    if (item.state === 'working') return '工作中';
    if (item.state === 'needs_attention') {
      return item.attention_reason === 'permission' ? '等待授权' : '等待输入';
    }
    if (item.state === 'failed') return '失败';
    if (item.state === 'completed') return '已完成';
    if (item.state === 'closed') return '已关闭';
    return '已取消';
  }

</script>

<svelte:window onkeydown={handleKeydown} />

{#if isBall}
  <main
    class="ball-shell"
    class:attention={ballKind === 'wait' || ballKind === 'fail'}
    class:pulse
    class:working={ballKind === 'working'}
    aria-label="OrbCue"
  >
    <button
      class="ball {ballKind}"
      style="--ratio: {ringRatio}"
      aria-label={`${snapshot.count_label}，${snapshot.pending_mark || (snapshot.working_count ? '工作中' : '空闲')}。点击展开或收起面板`}
      title={`${snapshot.count_label}，${snapshot.pending_mark || (snapshot.working_count ? '工作中' : '空闲')}。点击展开或收起面板`}
      onpointerdown={onBallPointerDown}
      onpointermove={onBallPointerMove}
      onclick={onBallClick}
    >
      {#if theme === 'glyph'}
        <span class="matrix" aria-hidden="true">
          {#each matrixDots as tone, i (i)}<i class="dot {tone}"></i>{/each}
        </span>
        <span class="count">
          <span class="count-work">{snapshot.working_count}</span>
          <span class="count-sep">/</span>
          <span class="count-total">{snapshot.tracked_count}</span>
        </span>
      {:else if theme === 'braun'}
        <span class="ball-lcd" aria-hidden="true"></span>
        <span class="ball-ring" aria-hidden="true"></span>
        <span class="count">
          <span class="count-work">{snapshot.working_count}</span>
          <span class="count-total">{snapshot.tracked_count}</span>
        </span>
      {:else if theme === 'glass'}
        <span class="ball-frost" aria-hidden="true"></span>
        <span class="ball-rim" aria-hidden="true"></span>
        <span class="ball-sheen" aria-hidden="true"></span>
        <span class="ball-arc" aria-hidden="true"></span>
        <span class="count">
          <span class="count-work">{snapshot.working_count}</span>
          <span class="count-sep">/</span>
          <span class="count-total">{snapshot.tracked_count}</span>
        </span>
      {:else if theme === 'fluent'}
        <span class="count">
          <span class="count-work">{snapshot.working_count}</span>
          <span class="count-sep">/</span>
          <span class="count-total">{snapshot.tracked_count}</span>
        </span>
        <span class="ball-bar" aria-hidden="true"></span>
      {:else}
        <span class="ball-core" aria-hidden="true"></span>
        <span class="ball-ring" aria-hidden="true"></span>
        <span class="ball-sheen" aria-hidden="true"></span>
        <span class="count">
          <span class="count-work">{snapshot.working_count}</span>
          <span class="count-total">{snapshot.tracked_count}</span>
        </span>
      {/if}
    </button>
    {#if snapshot.pending_mark && !hideBallBadge}<span class="badge mark-{markClass(snapshot.pending_mark)}" aria-label={snapshot.pending_mark}>{snapshot.pending_mark}</span>{/if}
  </main>
{:else}
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
    {#if page === 'activity'}
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
                    {#if focusNotes[sessionHighlightKey(session.source, session.session_id)]}<p class="session-focus-note">{focusNotes[sessionHighlightKey(session.source, session.session_id)]}</p>{/if}
                    {#if focusErrors[sessionHighlightKey(session.source, session.session_id)]}<p class="session-focus-error">{focusErrors[sessionHighlightKey(session.source, session.session_id)]}</p>{/if}
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
        <button class="text-button" onclick={() => acknowledge('*', '*')} disabled={snapshot.pending_count === 0}>全部已读</button>
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
      {#if !onboardingComplete}
        <div class="onboarding-banner">
          <strong>先连接一个已有的 Agent</strong>
          <p>连接只会安装可撤销的用户级 Hook 或 wrapper，并保留原命令和参数。</p>
          <button class="text-button" onclick={skipOnboarding}>稍后设置</button>
        </div>
      {/if}
      <div class="connections-toolbar">
        <button class="text-button" onclick={() => void refreshAgents()} disabled={inventoryRefreshing}>刷新</button>
        <button class="text-button" onclick={() => void addFromFolder()} disabled={inventoryRefreshing}>从文件夹添加</button>
        {#if inventoryRefreshing}<span class="refresh-hint" aria-live="polite">正在检测</span>{/if}
      </div>
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
        <div class="connection-list">
          {#each connectionAgents as agent (agent.side + ':' + agent.name)}
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
          {/each}
        </div>
        </div>
      {/if}
      {#if connectionError}<p class="error-message">{connectionError}</p>{/if}
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
    {:else}
      <p class="section-intro">默认保持安静，只在任务真正需要你回来时提醒一次。</p>
      <div class="panel-body">
      <div class="theme-picker" role="radiogroup" aria-label="外观">
        {#each THEMES as item (item)}
          <button type="button" role="radio" aria-checked={theme === item} class:active={theme === item} title={THEME_META[item].note} onclick={() => setTheme(item)}>
            {THEME_META[item].name}
          </button>
        {/each}
      </div>
      <div class="settings-list">
        <div class="setting-row alias-row">
          <span>
            <strong>启动别名</strong>
            <small>把 orb run 收成短命令，空则删除</small>
          </span>
          <form onsubmit={saveRunAlias}>
            <input bind:value={runAliasDraft} maxlength="24" spellcheck="false" autocapitalize="off" autocomplete="off" placeholder="dr" aria-label="启动别名" />
            <button type="submit" class="secondary-button">应用</button>
          </form>
        </div>
        {#if runAliasError}<p class="alias-hint error">{runAliasError}</p>
        {:else if runAliasHint}<p class="alias-hint">{runAliasHint}</p>{/if}
        <button class="setting-row" aria-pressed={hideBallBadge} onclick={toggleHideBallBadge}>
          <span><strong>隐藏圆标</strong><small>小球右上角的 ? / ! 不再显示</small></span><span class:enabled={hideBallBadge} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={completionSoundEnabled} onclick={() => toggleSound('completion')}>
          <span><strong>完成提示音</strong><small>任务正常完成时播放短音</small></span><span class:enabled={completionSoundEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={attentionSoundEnabled} onclick={() => toggleSound('attention')}>
          <span><strong>等待提示音</strong><small>等待输入或授权时播放短音</small></span><span class:enabled={attentionSoundEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={failureSoundEnabled} onclick={() => toggleSound('failure')}>
          <span><strong>失败提示音</strong><small>任务失败时播放较低音调</small></span><span class:enabled={failureSoundEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={notificationsEnabled} onclick={() => void toggleNotifications()}>
          <span><strong>系统通知</strong><small>等待输入、授权或失败时弹出一次；已完成只走提示音</small></span><span class:enabled={notificationsEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={autostartEnabled} onclick={toggleAutostart}>
          <span><strong>开机自启</strong><small>登录 Windows 后自动打开 OrbCue，不必先手动启动才能接收 Agent 状态</small></span><span class:enabled={autostartEnabled} class="switch"><i></i></span>
        </button>
        <button class="setting-row" aria-pressed={shortcutEnabled} onclick={toggleShortcut}>
          <span><strong>全局快捷键</strong><small>{shortcut} 打开或收起任务面板</small></span><span class:enabled={shortcutEnabled} class="switch"><i></i></span>
        </button>
      </div>
      </div>
      <div class="privacy-note"><strong>本地与隐私优先</strong><p>OrbCue 默认不联网，不读取 transcript、prompt、命令或代码；持久化状态也不包含摘要。</p></div>
    {/if}
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
  </main>
{/if}
