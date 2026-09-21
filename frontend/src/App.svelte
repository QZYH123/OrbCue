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
  import Ball from './Ball.svelte';
  import Panel from './Panel.svelte';
  import { playChime, unlockAudio } from './chime';
  import { ensureNotificationPermission } from './notifications';
  import type { AgentInventory, AgentSide, ConnectionPreview, DiscoveredAgent, FocusResult, SessionSnapshot, Snapshot, SnapshotMessage } from './types';
  import { emptySnapshot } from './types';
  import {
    highlightFromNotificationExtra,
    projectGroupKey,
    revealHighlightedGroup,
    sessionHighlightKey,
  } from './highlight';
  import { connectSuccessNotice, inventoryHasRows } from './inventory';
  import {
    clampToWorkArea,
    dockHitPx,
    dockSnapPx,
    edgeExpandedPosition,
    nearestWorkAreaEdge,
    shouldSnapToEdge,
    type WorkAreaEdge,
  } from './placement';
  import { jumpFeedback } from './jumpBack';
  import { applyPreviewDocument, demoInventory, previewLabel, previewSnapshot, tauriAvailable } from './preview';
  import {
    filterSessionSections,
    presentAuditRows,
    presentSessionSections,
    sessionDomKey,
    unreadCount,
  } from './sessionIdentity';
  import { barTones, matrixTones } from './glyphMatrix';
  import {
    initialTheme,
    persistTheme,
    subscribeTheme,
    type DockTheme,
  } from './theme';
  import {
    initialOnboardingComplete,
    nextOnboardingStep,
    persistOnboardingComplete,
    type OnboardingStep,
  } from './onboarding';

  const previewMode = !tauriAvailable();
  let label: string = previewMode ? previewLabel() : 'ball';
  let snapshot: Snapshot = previewMode ? previewSnapshot() : emptySnapshot;
  let pulse = false;
  let filter: 'all' | 'attention' | 'working' = 'all';
  let collapsedGroups: Record<string, boolean> = {};
  let page: 'activity' | 'audit' | 'connections' | 'settings' = 'activity';
  let inventory: AgentInventory = previewMode ? demoInventory : { discovered: [], connected: [] };
  let inventoryRefreshing = false;
  let connectionError = '';
  let pendingAgent: DiscoveredAgent | null = null;
  let connectionPreview: ConnectionPreview | null = null;
  let previewLoading = false;
  let previewError = '';
  let onboardingComplete = initialOnboardingComplete(window.location.search, localStorage);
  let onboardingStep: OnboardingStep = 'theme';
  const soundKeys = {
    completion: 'completion-sound-enabled',
    attention: 'attention-sound-enabled',
    failure: 'failure-sound-enabled',
  } as const;
  type SoundChannel = keyof typeof soundKeys;
  function storedFlag(key: string, defaultOn: boolean) {
    const raw = localStorage.getItem(key);
    return defaultOn ? raw !== 'false' : raw === 'true';
  }
  let soundEnabled: Record<SoundChannel, boolean> = {
    completion: storedFlag(soundKeys.completion, true),
    attention: storedFlag(soundKeys.attention, true),
    failure: storedFlag(soundKeys.failure, true),
  };
  let notificationsEnabled = storedFlag('notifications-enabled', true);
  let highlightedKey = '';
  let autostartEnabled = false;
  let autostartChecked = false;
  let autostartHintDismissed = storedFlag('autostart-hint-dismissed', false);
  let connectSuccess = '';
  let shortcutEnabled = storedFlag('shortcut-enabled', true);
  let hideBallBadge = storedFlag('orbcue-hide-ball-badge', false);
  let sideDockEnabled = storedFlag('orbcue-side-dock', true);
  let parked = false;
  let docked = false;
  let dockHome = { x: 0, y: 0 };
  let dockedAt = 0;
  let dockEdge: WorkAreaEdge | null = null;
  let ballHovered = false;
  let ignoringLeave = false;
  let suppressExpandUntilLeave = false;
  let expandTimer: number | undefined;
  let redockTimer: number | undefined;
  let suppressTimer: number | undefined;
  let theme: DockTheme = initialTheme();
  let runAlias = '';
  let runAliasDraft = '';
  let runAliasHint = '';
  let runAliasError = '';
  let replaceTabOnRun = false;
  const shortcut = 'CommandOrControl+Shift+Space';
  const BADGE_KEY = 'orbcue-hide-ball-badge';
  const SIDE_DOCK_KEY = 'orbcue-side-dock';
  const REPLACE_TAB_KEY = 'orbcue-replace-tab';
  let badgeChannel: BroadcastChannel | null = null;
  let sideDockChannel: BroadcastChannel | null = null;
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
  $: unread = unreadCount(snapshot.sessions);
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

  function applySideDockPref(enabled: boolean) {
    sideDockEnabled = enabled;
    if (label === 'ball' && !enabled) void unparkBall();
  }

  function listenBooleanPreference(
    key: string,
    apply: (enabled: boolean) => void,
    defaultValue = false,
  ): { channel: BroadcastChannel | null; stop: () => void } {
    const onStorage = (event: StorageEvent) => {
      if (event.key === key) apply(event.newValue === null ? defaultValue : event.newValue === 'true');
    };
    window.addEventListener('storage', onStorage);
    if (typeof BroadcastChannel === 'undefined') {
      return { channel: null, stop: () => window.removeEventListener('storage', onStorage) };
    }
    const channel = new BroadcastChannel(key);
    channel.onmessage = (event) => apply(event.data === true);
    return {
      channel,
      stop: () => {
        channel.close();
        window.removeEventListener('storage', onStorage);
      },
    };
  }

  onMount(() => {
    const unsubTheme = subscribeTheme((next) => {
      theme = next;
    });
    const badgePreference = listenBooleanPreference(BADGE_KEY, (enabled) => {
      hideBallBadge = enabled;
    });
    badgeChannel = badgePreference.channel;
    const sideDockPreference = listenBooleanPreference(
      SIDE_DOCK_KEY,
      applySideDockPref,
      true,
    );
    sideDockChannel = sideDockPreference.channel;
    void loadRunAlias();
    void loadReplaceTab();
    if (previewMode) {
      const pageQ = new URLSearchParams(window.location.search).get('page');
      if (pageQ === 'audit' || pageQ === 'connections' || pageQ === 'settings' || pageQ === 'activity') {
        page = pageQ;
      }
      return () => {
        badgePreference.stop();
        sideDockPreference.stop();
        unsubTheme();
      };
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
        const next = event.payload.snapshot;
        const previousMark = snapshot.pending_mark;
        snapshot = next;
        if (
          label === 'ball' &&
          isAttentionMark(next.pending_mark) &&
          next.pending_mark !== previousMark
        ) {
          void unparkBall();
        }
        if (event.payload.attention && label === 'ball') {
          pulse = true;
          window.setTimeout(() => (pulse = false), 280);
          void playChime(event.payload.attention.severity, soundEnabled);
        }
      });
      const stopInventory = await listen<AgentInventory>('orb:inventory', (event) => {
        if (!active) return;
        inventory = event.payload;
        inventoryRefreshing = false;
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
      if (label === 'ball' && !onboardingComplete) {
        void invoke('open_panel');
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
      const stopListeners = () => {
        stopListening();
        stopInventory();
        stopHighlight();
        stopFocus();
        void stopAction.unregister();
      };
      if (active) unsubscribe = stopListeners;
      else stopListeners();
      if (label === 'ball') {
        const stopMoved = await getCurrentWindow().onMoved(() => {
          if (!dragArmed) return;
          window.clearTimeout(snapTimer);
          snapTimer = window.setTimeout(() => {
            dragArmed = false;
            dragging = false;
            void finishBallDrag();
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
      window.clearTimeout(expandTimer);
      window.clearTimeout(redockTimer);
      window.clearTimeout(suppressTimer);
      unsubscribe?.();
      badgePreference.stop();
      sideDockPreference.stop();
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
    window.clearTimeout(snapTimer);
    dragArmed = false;
    dragging = false;
    suppressClick = false;
    dragStart = { x: event.screenX, y: event.screenY };
    void unlockAudio();
  }

  function onBallPointerEnter() {
    ballHovered = true;
    window.clearTimeout(redockTimer);
    if (suppressExpandUntilLeave) return;
    queueExpandIfDocked();
  }

  function onDockedPointerMove() {
    ballHovered = true;
    window.clearTimeout(redockTimer);
    if (suppressExpandUntilLeave || dragArmed || dragging) return;
    queueExpandIfDocked();
  }

  function onBallPointerLeave() {
    if (ignoringLeave) return;
    ballHovered = false;
    window.clearTimeout(expandTimer);
    if (docked) {
      suppressExpandUntilLeave = false;
      return;
    }
    if (dragArmed || dragging) return;
    scheduleRedock();
  }

  function queueExpandIfDocked() {
    if (!docked || dragArmed || dragging || suppressExpandUntilLeave) return;
    window.clearTimeout(expandTimer);
    const wait = Math.max(0, 500 - (Date.now() - dockedAt));
    expandTimer = window.setTimeout(() => {
      expandTimer = undefined;
      if (docked && ballHovered && !dragArmed && !dragging && !suppressExpandUntilLeave) {
        void expandBall();
      }
    }, wait);
  }

  function scheduleRedock() {
    window.clearTimeout(redockTimer);
    redockTimer = window.setTimeout(() => {
      redockTimer = undefined;
      void confirmRedock();
    }, 160);
  }

  async function confirmRedock() {
    if (!parked || docked || dragArmed || dragging || ignoringLeave) return;
    try {
      if (await invoke<boolean>('cursor_over_ball')) {
        ballHovered = true;
        return;
      }
    } catch {
      if (ballHovered) return;
    }
    ballHovered = false;
    if (parked && !docked && !dragArmed && !dragging) {
      void dockBall();
    }
  }

  async function onBallPointerMove(event: PointerEvent) {
    if (event.buttons === 0 || dragging || dragArmed) return;
    const dx = event.screenX - dragStart.x;
    const dy = event.screenY - dragStart.y;
    if (dx * dx + dy * dy < 25) return;
    dragging = true;
    suppressClick = true;
    dragArmed = true;
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
    try {
      await getCurrentWindow().startDragging();
    } catch (error) {
      suppressClick = false;
      dragArmed = false;
      console.warn('Could not drag Dock ball', error);
    } finally {
      dragging = false;
    }
  }

  function onBallClick(event: MouseEvent) {
    if (suppressClick) {
      event.preventDefault();
      suppressClick = false;
      return;
    }
    if (docked) void expandBall();
    void togglePanel();
  }

  async function ballWorkArea() {
    const win = getCurrentWindow();
    const monitor = (await currentMonitor()) ?? (await primaryMonitor());
    if (!monitor) return null;
    const pos = await win.outerPosition();
    const size = await win.outerSize();
    const area = monitor.workArea ?? { position: monitor.position, size: monitor.size };
    return {
      win,
      rect: { x: pos.x, y: pos.y, width: size.width, height: size.height },
      work: {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
      },
    };
  }

  function persistFlag(key: string, value: boolean, channel?: BroadcastChannel | null) {
    try {
      localStorage.setItem(key, String(value));
    } catch {
      /* ignore quota */
    }
    channel?.postMessage(value);
  }

  function isAttentionMark(mark: string): boolean {
    return mark === '?' || mark === '!';
  }

  function clearDock() {
    parked = false;
    docked = false;
    dockEdge = null;
  }

  async function dockBall(): Promise<boolean> {
    if (previewMode || docked || label !== 'ball' || !sideDockEnabled) return false;
    try {
      const placed = await ballWorkArea();
      if (placed) {
        dockHome = clampToWorkArea(placed.rect, placed.work);
        dockEdge = nearestWorkAreaEdge(placed.rect, placed.work);
      }
      const tucked = await invoke<boolean>('dock_ball', { hit: placed ? dockHitPx(placed.rect) : 32 });
      if (!tucked) {
        clearDock();
        return false;
      }
      parked = true;
      docked = true;
      dockedAt = Date.now();
      suppressExpandUntilLeave = true;
      window.clearTimeout(suppressTimer);
      suppressTimer = window.setTimeout(() => {
        if (!ballHovered) suppressExpandUntilLeave = false;
      }, 200);
      return true;
    } catch (error) {
      clearDock();
      console.warn('Could not dock Dock ball', error);
      return false;
    }
  }

  async function expandDestination() {
    try {
      const placed = await ballWorkArea();
      if (placed && dockEdge) {
        return edgeExpandedPosition(placed.rect, placed.work, dockEdge);
      }
    } catch {
      /* fall through */
    }
    return dockHome;
  }

  async function expandBall() {
    if (!docked) return;
    const dest = await expandDestination();
    docked = false;
    ignoringLeave = true;
    window.clearTimeout(redockTimer);
    try {
      await invoke('undock_ball', { x: dest.x, y: dest.y });
    } catch (error) {
      console.warn('Could not undock Dock ball', error);
    }
    window.setTimeout(() => {
      ignoringLeave = false;
      void confirmRedock();
    }, 80);
  }

  async function unparkBall() {
    parked = false;
    await expandBall();
  }

  async function finishBallDrag() {
    if (previewMode) {
      return;
    }
    try {
      const placed = await ballWorkArea();
      if (
        sideDockEnabled &&
        placed &&
        shouldSnapToEdge(placed.rect, placed.work, dockSnapPx(placed.rect))
      ) {
        docked = false;
        if (await dockBall()) return;
      }
    } catch (error) {
      console.warn('Could not snap Dock ball to the edge', error);
    }
    parked = false;
    docked = false;
    void clampBallToWorkArea();
  }

  async function clampBallToWorkArea() {
    if (docked) return;
    try {
      const placed = await ballWorkArea();
      if (!placed) return;
      const clamped = clampToWorkArea(placed.rect, placed.work);
      if (clamped.x === placed.rect.x && clamped.y === placed.rect.y) return;
      await placed.win.setPosition(new PhysicalPosition(clamped.x, clamped.y));
    } catch (error) {
      console.warn('Could not keep Dock ball on screen', error);
    }
  }

  function toggleGroup(key: string) {
    collapsedGroups = { ...collapsedGroups, [key]: !collapsedGroups[key] };
  }

  function selectPage(next: typeof page) {
    if (next !== 'connections') connectSuccess = '';
    page = next;
    if (next === 'connections' && !previewMode) void loadAgentsCached();
  }

  async function updateInventory(
    command: string,
    waitForBackgroundRefresh = false,
  ) {
    if (previewMode) return;
    inventoryRefreshing = true;
    connectionError = '';
    try {
      inventory = await invoke<AgentInventory>(command);
      // `agent_inventory` may return an empty cache while a background scan
      // publishes the real result. Keep its placeholder only for that case.
      inventoryRefreshing = waitForBackgroundRefresh && !inventoryHasRows(inventory);
    } catch (error) {
      connectionError = String(error);
      // failed requests must never leave the UI stuck on "正在检测".
      inventoryRefreshing = false;
    }
  }

  function loadAgentsCached() {
    return updateInventory('agent_inventory', true);
  }

  function refreshAgents() {
    return updateInventory('refresh_agents');
  }

  function addFromFolder() {
    return updateInventory('add_agent_folder');
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
    } catch (error) {
      connectionError = String(error);
    }
  }

  function finishOnboarding() {
    onboardingComplete = true;
    persistOnboardingComplete(localStorage);
    page = 'activity';
  }

  function skipOnboardingStep() {
    const next = nextOnboardingStep(onboardingStep);
    if (next === 'done') {
      finishOnboarding();
      return;
    }
    onboardingStep = next;
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
    persistFlag('autostart-hint-dismissed', true);
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
      persistFlag('shortcut-enabled', shortcutEnabled);
    } catch (error) {
      console.warn('Could not update shortcut', error);
    }
  }

  async function toggleNotifications() {
    if (notificationsEnabled) {
      notificationsEnabled = false;
      persistFlag('notifications-enabled', false);
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
    persistFlag('notifications-enabled', true);
    try {
      await invoke('set_notification_enabled', { enabled: true });
      await invoke('preview_notification');
    } catch (error) {
      console.warn('Could not update notification preference', error);
    }
  }

  function setTheme(next: DockTheme) {
    theme = next;
    persistTheme(next);
  }

  function toggleHideBallBadge() {
    hideBallBadge = !hideBallBadge;
    persistFlag(BADGE_KEY, hideBallBadge, badgeChannel);
  }

  function toggleSideDock() {
    applySideDockPref(!sideDockEnabled);
    persistFlag(SIDE_DOCK_KEY, sideDockEnabled, sideDockChannel);
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

  async function loadReplaceTab() {
    if (previewMode) {
      replaceTabOnRun = storedFlag(REPLACE_TAB_KEY, false);
      return;
    }
    try {
      replaceTabOnRun = await invoke<boolean>('replace_tab_on_run');
    } catch (error) {
      console.warn('Could not load replace-tab preference', error);
    }
  }

  async function toggleReplaceTab() {
    const next = !replaceTabOnRun;
    if (previewMode) {
      replaceTabOnRun = next;
      persistFlag(REPLACE_TAB_KEY, next);
      return;
    }
    try {
      replaceTabOnRun = await invoke<boolean>('set_replace_tab_on_run', { enabled: next });
    } catch (error) {
      console.warn('Could not update replace-tab preference', error);
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

  function toggleSound(channel: SoundChannel) {
    const enabled = !soundEnabled[channel];
    soundEnabled = { ...soundEnabled, [channel]: enabled };
    persistFlag(soundKeys[channel], enabled);
    if (enabled) {
      const severity = channel === 'completion' ? 'info' : channel === 'attention' ? 'attention' : 'error';
      void playChime(severity, soundEnabled);
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
    const key = sessionDomKey(session);
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
    return ({ '!': 'fail', '?': 'wait', '*': 'done', x: 'cancel' } as Record<string, string>)[mark] ?? 'idle';
  }

  function stateLabel(item: { state: SessionSnapshot['state']; attention_reason: string | null }) {
    if (item.state === 'needs_attention') {
      return item.attention_reason === 'permission' ? '等待授权' : '等待输入';
    }
    return (
      {
        idle: '空闲',
        working: '工作中',
        failed: '失败',
        completed: '已完成',
        closed: '已关闭',
        cancelled: '已取消',
      } as Record<string, string>
    )[item.state] ?? '已取消';
  }

</script>

<svelte:window onkeydown={handleKeydown} onpointerleave={onBallPointerLeave} />

{#if isBall}
  <Ball
    {snapshot}
    {ballKind}
    {ringRatio}
    {pulse}
    {theme}
    {matrixDots}
    {hideBallBadge}
    {docked}
    {dockEdge}
    {onBallPointerEnter}
    {onBallPointerLeave}
    {onDockedPointerMove}
    {onBallPointerDown}
    {onBallPointerMove}
    {onBallClick}
    {markClass}
  />
{:else}
  <Panel
    {snapshot}
    {ballKind}
    {theme}
    {heroBar}
    {closePanel}
    {setTheme}
    {refreshAgents}
    {inventoryRefreshing}
    {addFromFolder}
    {connected}
    {disconnectAgent}
    {connectAgent}
    {connectionAgents}
    {onboardingComplete}
    {onboardingStep}
    {skipOnboardingStep}
    {finishOnboarding}
    {inventory}
    {connectionError}
    {runAlias}
    {page}
    bind:filter
    {unread}
    {showAutostartHint}
    {acceptAutostartHint}
    {dismissAutostartHint}
    {sessionGroups}
    {collapsedGroups}
    {toggleGroup}
    {highlightedKey}
    {acknowledge}
    {resetSession}
    {stateLabel}
    {focusNotes}
    {focusErrors}
    {jumpBack}
    {auditRows}
    bind:connectSuccess
    {pendingAgent}
    {closeConnectDialog}
    {previewLoading}
    {previewError}
    {connectionPreview}
    {confirmConnect}
    bind:runAliasDraft
    {saveRunAlias}
    {runAliasError}
    {runAliasHint}
    {replaceTabOnRun}
    {toggleReplaceTab}
    {hideBallBadge}
    {toggleHideBallBadge}
    {sideDockEnabled}
    {toggleSideDock}
    {soundEnabled}
    {toggleSound}
    {notificationsEnabled}
    {toggleNotifications}
    {autostartEnabled}
    {toggleAutostart}
    {shortcutEnabled}
    {toggleShortcut}
    {shortcut}
    {selectPage}
    {visibleSessions}
  />
{/if}
