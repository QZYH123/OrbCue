<script lang="ts">
  import type { AgentInventory, AgentSide, ConnectionPreview, ConnectionRecord, DiscoveredAgent, SessionSnapshot, Snapshot } from './types';
  import { copyOf, type Copy } from './copy';
  import type { Lang, LangPref } from './locale';
  import type { DockTheme } from './theme';
  import { THEMES, themeMeta } from './theme';
  import { isDesktopTerminalId, isDockTerminalId, jumpPhrases } from './jumpBack';
  import { displayAgent, formatAuditTime, sessionDomKey, type AuditRow, type SessionSection } from './sessionIdentity';
  import { sessionHighlightKey } from './highlight';
  import { showDetectingPlaceholder, sideLabel, wslDockErrorBanner } from './inventory';
  import { onboardingStepIndex, ONBOARDING_STEPS, type OnboardingStep } from './onboarding';

  export let snapshot: Snapshot;
  export let ballKind: string;
  export let theme: DockTheme;
  export let lang: Lang;
  export let langPref: LangPref;
  export let chooseLang: (next: LangPref) => void;
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
  export let directRun: boolean;
  export let toggleDirectRun: () => void;
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
  export let completionNotifyEnabled: boolean;
  export let toggleCompletionNotify: () => void;
  export let phoneNotifyDraft: string;
  export let savePhoneNotify: (event: SubmitEvent) => void;
  export let phoneNotifyError: string;
  export let phoneNotifyHint: string;
  export let phoneNotifySaving: boolean;
  export let autostartEnabled: boolean;
  export let toggleAutostart: () => void;
  export let shortcutEnabled: boolean;
  export let toggleShortcut: () => void;
  export let shortcut: string;
  export let selectPage: (page: 'activity' | 'audit' | 'connections' | 'settings') => void;
  export let visibleSessions: SessionSnapshot[];

  $: text = copyOf(lang);
  $: phrases = jumpPhrases(lang);

  function jumpButtonText(terminalId: string | null | undefined, copy: Copy) {
    if (isDesktopTerminalId(terminalId)) {
      return { precise: false, label: copy.jumpApp, title: copy.jumpAppTitle };
    }
    if (isDockTerminalId(terminalId)) {
      return { precise: true, label: copy.jumpExact, title: copy.jumpExact };
    }
    return { precise: false, label: copy.jumpWindow, title: copy.jumpWindowTitle };
  }

  $: themes = themeMeta(lang);
  $: wslBanner = wslDockErrorBanner(inventory, lang);
</script>

  <main class="panel tone-{ballKind}" aria-label={text.panelLabel}>
    <header class="hero">
      <div class="hero-main">
        <div class="hero-count lcd" aria-live="polite">
          <span class="lcd-digits">
            <span class="hero-work lcd-work">{snapshot.working_count}</span>
            <span class="hero-slash lcd-slash">/</span>
            <span class="hero-track lcd-track">{snapshot.tracked_count}</span>
          </span>
          <span class="hero-rest">
            <span class="hero-meta lcd-meta">{ballKind === 'fail' ? text.heroFailed : ballKind === 'wait' ? text.heroNeedsYou : ballKind === 'working' ? text.working : text.idle}</span>
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
      <button class="icon-button key-round" onclick={closePanel} aria-label={text.close}>×</button>
    </header>
    {#snippet themePicker()}
      <div class="theme-picker" role="radiogroup" aria-label={text.appearance}>
        {#each THEMES as item (item)}
          <button type="button" role="radio" aria-checked={theme === item} class:active={theme === item} title={themes[item].note} onclick={() => setTheme(item)}>
            {themes[item].name}
          </button>
        {/each}
      </div>
    {/snippet}
    {#snippet connectionsToolbar()}
      <div class="connections-toolbar">
        <button class="text-button" onclick={() => void refreshAgents()} disabled={inventoryRefreshing}>{text.refresh}</button>
        <button class="text-button" onclick={() => void addFromFolder()} disabled={inventoryRefreshing}>{text.addFolder}</button>
        {#if inventoryRefreshing}<span class="refresh-hint" aria-live="polite">{text.checking}</span>{/if}
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
          {#if record?.limitation}<p class="connection-note" title={record.limitation}>{record.limitation}</p>{/if}
        </div>
        <div class="connection-aside">
          <span class:connected={!!record} class="connection-state">{record ? text.connected : text.available}</span>
          {#if record}
            <button class="secondary-button" onclick={() => disconnectAgent(agent.name, agent.side)}>{text.disconnect}</button>
          {:else}
            <button class="primary-button" onclick={() => connectAgent(agent)}>{text.connect}</button>
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
      <section class="panel-body onboarding" aria-label={text.firstRun}>
        <p class="onboarding-step">{onboardingStepIndex(onboardingStep)} / {ONBOARDING_STEPS.length}</p>
        {#if onboardingStep === 'theme'}
          <div class="onboarding-copy">
            <h2>{text.onboardingLook}</h2>
            <p>{text.onboardingLookBody}</p>
          </div>
          {@render themePicker()}
        {:else if onboardingStep === 'connect'}
          <div class="onboarding-copy">
            <h2>{text.onboardingConnect}</h2>
            <p>{text.onboardingConnectBody}</p>
          </div>
          {@render connectionsToolbar()}
          <div class="onboarding-main">
            {#if showDetectingPlaceholder(inventory, inventoryRefreshing)}
              <div class="empty compact"><span>…</span><p>{text.checkingTools}</p></div>
            {:else if connectionAgents.length === 0}
              <div class="empty compact"><span>○</span><p>{text.noTools}</p><small>{text.noToolsOnboarding}</small></div>
            {:else}
              {@render connectionList()}
            {/if}
            {#if connectionError}<p class="error-message">{connectionError}</p>{/if}
          </div>
        {:else}
          <div class="onboarding-copy">
            <h2>{text.onboardingRun}</h2>
            <p>{text.onboardingRunBody}</p>
          </div>
          <code class="onboarding-code">orb run grok</code>
          <p class="onboarding-note">{text.onboardingAlias(runAlias)}</p>
        {/if}
        <div class="onboarding-actions">
          <button class="text-button" onclick={skipOnboardingStep}>{text.skip}</button>
          {#if onboardingStep === 'run'}
            <button class="primary-button" onclick={finishOnboarding}>{text.doneSetup}</button>
          {:else}
            <button class="primary-button" onclick={skipOnboardingStep}>{text.next}</button>
          {/if}
        </div>
      </section>
    {:else if page === 'activity'}
      <nav class="filters" aria-label={text.filterSessions}>
        <button aria-pressed={filter === 'all'} class:active={filter === 'all'} onclick={() => (filter = 'all')}>{text.filterAll} <span>{snapshot.tracked_count}</span></button>
        <button aria-pressed={filter === 'working'} class:active={filter === 'working'} onclick={() => (filter = 'working')}>{text.filterWorking} <span>{snapshot.working_count}</span></button>
        <button aria-pressed={filter === 'attention'} class:active={filter === 'attention'} onclick={() => (filter = 'attention')}>{text.filterNotWorking} <span>{snapshot.pending_count}</span></button>
      </nav>
      <div class="panel-body">
      {#if showAutostartHint}
        <div class="hint-banner" role="note">
          <p><strong>{text.autostartTitle}</strong><small>{text.autostartBody}</small></p>
          <div class="hint-actions">
            <button class="primary-button" onclick={() => void acceptAutostartHint()}>{text.turnOn}</button>
            <button class="text-button" onclick={dismissAutostartHint}>{text.dontAsk}</button>
          </div>
        </div>
      {/if}
      <div class="sessions">
        {#if visibleSessions.length === 0}
          <div class="empty"><span>✓</span><p>{filter === 'all' ? text.emptyAll : text.emptyFiltered}</p><small>{phrases.empty}</small></div>
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
                {@const jump = jumpButtonText(session.terminal_id, text)}
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
                      {#if !session.acknowledged}<button onclick={() => acknowledge(session.source, session.session_id, session.terminal_id)}>{text.markRead}</button>{/if}
                      <button onclick={() => resetSession(session.source, session.session_id, session.terminal_id)}>{text.clear}</button>
                    </div>
                    {#if focusNotes[sessionDomKey(session)]}<p class="session-focus-note">{focusNotes[sessionDomKey(session)]}</p>{/if}
                    {#if focusErrors[sessionDomKey(session)]}<p class="session-focus-error">{focusErrors[sessionDomKey(session)]}</p>{/if}
                  </div>
                  <button
                    class="jump-btn"
                    class:precise={jump.precise}
                    onclick={() => jumpBack(session)}
                    aria-label={jump.label}
                    title={jump.title}
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
        <button class="text-button" onclick={() => acknowledge('*', '*')} disabled={unread === 0}>{text.markAllRead}</button>
        <button class="text-button danger" onclick={() => resetSession('*', '*')} disabled={snapshot.tracked_count === 0}>{text.clearAll}</button>
      </footer>
    {:else if page === 'audit'}
      <p class="section-intro">{text.auditIntro}</p>
      <div class="panel-body">
      <div class="audit-list">
        {#if auditRows.length === 0}
          <div class="empty compact"><span>✓</span><p>{text.auditEmpty}</p><small>{text.auditEmptyHint}</small></div>
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
      <p class="section-intro">{phrases.intro}</p>
      {@render connectionsToolbar()}
      {#if wslBanner}
        <p class="error-message">{wslBanner}</p>
      {/if}
      {#if connectSuccess}
        <div class="hint-banner" role="status">
          <p>{connectSuccess}</p>
          <div class="hint-actions">
            <button class="text-button" onclick={() => (connectSuccess = '')} aria-label={text.dismiss}>×</button>
          </div>
        </div>
      {/if}
      {#if showDetectingPlaceholder(inventory, inventoryRefreshing)}
        <div class="empty compact"><span>…</span><p>{text.checkingAgents}</p></div>
      {:else if connectionAgents.length === 0}
        <div class="empty compact"><span>○</span><p>{text.noTools}</p><small>{text.noToolsConnect}</small></div>
      {:else}
        <div class="panel-body">
        {@render connectionList()}
        </div>
      {/if}
      {#if connectionError}<p class="error-message">{connectionError}</p>{/if}
    {:else}
      <p class="section-intro">{text.settingsIntro}</p>
      <div class="panel-body">
      {@render themePicker()}
      <div class="settings-list">
        <div class="setting-row lang-row">
          <span>
            <strong>{text.language}</strong>
            <small>{text.languageHint}</small>
          </span>
          <span class="lang-switch" role="group" aria-label={text.language}>
            <button type="button" aria-pressed={langPref === 'system'} onclick={() => chooseLang('system')}>{text.langSystem}</button>
            <button type="button" aria-pressed={langPref === 'zh'} onclick={() => chooseLang('zh')}>{text.langZh}</button>
            <button type="button" aria-pressed={langPref === 'en'} onclick={() => chooseLang('en')}>{text.langEn}</button>
          </span>
        </div>
        <div class="setting-row alias-row">
          <span>
            <strong>{text.runAlias}</strong>
            <small>{text.runAliasHint}</small>
          </span>
          <form onsubmit={saveRunAlias}>
            <input bind:value={runAliasDraft} maxlength="24" spellcheck="false" autocapitalize="off" autocomplete="off" placeholder="or" aria-label={text.runAlias} />
            <button type="submit" class="secondary-button">{text.apply}</button>
          </form>
        </div>
        {#if runAliasError}<p class="alias-hint error">{runAliasError}</p>
        {:else if runAliasHint}<p class="alias-hint">{runAliasHint}</p>{/if}
        {#snippet switchRow(pressed: boolean, title: string, hint: string, onclick: () => void)}
          <button class="setting-row" aria-pressed={pressed} {onclick}>
            <span><strong>{title}</strong><small>{hint}</small></span><span class:enabled={pressed} class="switch"><i></i></span>
          </button>
        {/snippet}
        {@render switchRow(directRun, text.originalCommand, text.originalCommandHint, () => void toggleDirectRun())}
        {@render switchRow(replaceTabOnRun, text.replaceTab, text.replaceTabHint, () => void toggleReplaceTab())}
        {@render switchRow(hideBallBadge, text.hideBadge, text.hideBadgeHint, toggleHideBallBadge)}
        {@render switchRow(sideDockEnabled, text.snapEdge, text.snapEdgeHint, toggleSideDock)}
        {@render switchRow(soundEnabled.completion, text.doneSound, text.doneSoundHint, () => toggleSound('completion'))}
        {@render switchRow(soundEnabled.attention, text.waitingSound, text.waitingSoundHint, () => toggleSound('attention'))}
        {@render switchRow(soundEnabled.failure, text.failureSound, text.failureSoundHint, () => toggleSound('failure'))}
        {@render switchRow(notificationsEnabled, text.systemNotifications, text.systemNotificationsHint, () => void toggleNotifications())}
        {@render switchRow(completionNotifyEnabled, text.completionAlerts, text.completionAlertsHint, () => void toggleCompletionNotify())}
        <div class="setting-row phone-row">
          <span>
            <strong>{text.phoneAlerts}</strong>
            <small>{text.phoneAlertsHint}</small>
          </span>
          <form onsubmit={savePhoneNotify}>
            <input bind:value={phoneNotifyDraft} maxlength="2048" spellcheck="false" autocapitalize="off" autocomplete="off" placeholder={text.phonePlaceholder} aria-label={text.phoneLabel} />
            <button type="submit" class="secondary-button" disabled={phoneNotifySaving}>{text.apply}</button>
          </form>
        </div>
        {#if phoneNotifyError}<p class="alias-hint error">{phoneNotifyError}</p>
        {:else if phoneNotifyHint}<p class="alias-hint">{phoneNotifyHint}</p>{/if}
        {@render switchRow(autostartEnabled, text.startAtLogin, text.startAtLoginHint, () => void toggleAutostart())}
        {@render switchRow(shortcutEnabled, text.globalShortcut, text.shortcutHint(shortcut), () => void toggleShortcut())}
      </div>
      </div>
      <div class="privacy-note"><strong>{text.privacyTitle}</strong><p>{text.privacyBody}</p></div>
    {/if}
    {#if onboardingComplete}
    <nav class="dock-nav" aria-label={text.pages}>
      <button aria-pressed={page === 'activity'} class:active={page === 'activity'} onclick={() => selectPage('activity')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M1.5 8.5h2.3l1.5-4.2 2.6 8.4L10 8.5h4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </span>
        {text.navActivity}
      </button>
      <button aria-pressed={page === 'audit'} class:active={page === 'audit'} onclick={() => selectPage('audit')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M3.5 4.5h9M3.5 8h9M3.5 11.5h6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
        </span>
        {text.navAudit}
      </button>
      <button aria-pressed={page === 'connections'} class:active={page === 'connections'} onclick={() => selectPage('connections')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M6.7 9.3 4.6 11.4a2 2 0 0 0 2.8 2.8l2.1-2.1M9.3 6.7l2.1-2.1a2 2 0 0 0-2.8-2.8L6.5 3.9M6.4 9.6l3.2-3.2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        </span>
        {text.navConnect}
      </button>
      <button aria-pressed={page === 'settings'} class:active={page === 'settings'} onclick={() => selectPage('settings')}>
        <span class="nav-key" aria-hidden="true">
          <svg class="nav-icon" viewBox="0 0 16 16"><path d="M3 4.5h10M3 8h10M3 11.5h10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><circle cx="6.2" cy="4.5" r="1.45" fill="currentColor"/><circle cx="10.2" cy="8" r="1.45" fill="currentColor"/><circle cx="7.4" cy="11.5" r="1.45" fill="currentColor"/></svg>
        </span>
        {text.navSettings}
      </button>
    </nav>
    {/if}
    {#if pendingAgent}
      <div class="modal-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && closeConnectDialog()}>
        <dialog open class="confirm-dialog" aria-labelledby="connect-title">
          <h2 id="connect-title">{text.connectTitle(displayAgent(pendingAgent.name))}<span class="side-badge side-{pendingAgent.side}">{sideLabel(pendingAgent.side)}</span></h2>
          <p>{text.connectUses(sideLabel(pendingAgent.side))}</p>
          <code>{pendingAgent.path}</code>
          {#if previewLoading}
            <p class="dialog-note">{text.buildingPreview}</p>
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
            <button class="secondary-button" onclick={closeConnectDialog}>{text.cancel}</button>
            <button class="primary-button" onclick={confirmConnect} disabled={previewLoading || !connectionPreview}>{text.confirmConnect}</button>
          </div>
        </dialog>
      </div>
    {/if}
  </main>
