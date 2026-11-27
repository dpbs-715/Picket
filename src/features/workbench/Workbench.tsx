import { useEffect, useLayoutEffect, useReducer, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import { desktopWindowController, type WindowController } from '../../desktop/window';
import { getMessages, I18nProvider } from '../../i18n';
import {
  MOTION_DURATION,
  MOTION_EASE,
  MotionRoot,
  m,
  useInstantMotion,
} from '../../ui/motion';
import { disconnectedBackend, type WorkbenchBackend } from './commands';
import { ActionContextStrip } from './components/ActionContextStrip';
import { ActionFooter } from './components/ActionFooter';
import { ChangesPanel } from './components/ChangesPanel';
import { CompactBar } from './components/CompactBar';
import { ContextHeader } from './components/ContextHeader';
import {
  computeDraggedDockPreference,
  readSavedDockSize,
  resolveDockSplit,
  writeSavedDockSize,
  type DockSizePreference,
} from './components/dockResize';
import { MessageDock } from './components/MessageDock';
import { OperationNoticeBar } from './components/OperationNoticeBar';
import { WorkbenchOverlays } from './components/WorkbenchOverlays';
import { useNoticeFeedback } from './hooks/useNoticeFeedback';
import { useSettingsServices } from './hooks/useSettingsServices';
import { useWindowMode } from './hooks/useWindowMode';
import { useWorkbenchCommands } from './hooks/useWorkbenchCommands';
import { useWorkbenchSync } from './hooks/useWorkbenchSync';
import {
  activeCustomActionFor,
  customActionsFor,
  editors,
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  emptyWorkbench,
  selectedFiles,
  targetOf,
  workbenchReducer,
  type WorkbenchState,
} from './state/workbench';
import type { ActionOverlayItem, ModelOverlayState } from './types';
import './workbench.css';

interface Props {
  initialState?: WorkbenchState;
  backend?: WorkbenchBackend;
  windowController?: WindowController;
}

export function Workbench({
  initialState,
  backend = disconnectedBackend,
  windowController = desktopWindowController,
}: Props) {
  const [state, dispatch] = useReducer(workbenchReducer, initialState, (initial) => initial ?? emptyWorkbench());
  const latestState = useRef(state);
  latestState.current = state;

  const [modelOverlay, setModelOverlay] = useState<ModelOverlayState>({ kind: 'closed' });
  const [actionStack, setActionStack] = useState<ActionOverlayItem[]>([]);
  const [dockSizePref, setDockSizePref] = useState<DockSizePreference>(() => readSavedDockSize());
  const [dockAvailableHeight, setDockAvailableHeight] = useState(0);
  const [resizingDock, setResizingDock] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [hydrated, setHydrated] = useState(() => !!initialState || !backend.loadWorkbenchState);

  const workbenchContentRef = useRef<HTMLDivElement>(null);
  const lowerWorkspaceRef = useRef<HTMLElement>(null);
  const cleanupDockResizeRef = useRef<(() => void) | null>(null);
  const contextStripRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const promptFocusRequestedRef = useRef(false);
  const prevPromptViewKeyRef = useRef<string | null>(null);
  const prevPromptTextRef = useRef('');

  const instantMotion = useInstantMotion();
  const t = getMessages(state.locale);
  const modelOverlayOpen = modelOverlay.kind !== 'closed';
  const actionOverlayOpen = actionStack.length > 0;

  const worktree = state.worktrees.find((tree) => tree.worktreeId === state.activeWorktreeId);
  const repository = state.repositories.find((item) => item.id === worktree?.repositoryId);
  const viewKey = worktree?.worktreeId ?? 'empty';
  const promptText = worktree ? worktree.prompt.text : state.emptyPrompt;
  const showPromptBox = promptOpen || promptText.length > 0;
  const hasExpandedDock = Boolean(worktree && worktree.dockExpanded && worktree.messages.length > 0);
  const hasCollapsedDockBar = Boolean(worktree && !worktree.dockExpanded && worktree.messages.length > 0);
  const dockSplit = hasExpandedDock
    ? resolveDockSplit(dockSizePref, dockAvailableHeight)
    : { fullHeight: false, changesHeightPx: null, dockHeightPx: null };
  const changesLayoutMode: 'idle' | 'peek' | 'hidden' = !hasExpandedDock
    ? 'idle'
    : dockSplit.fullHeight
      ? 'hidden'
      : 'peek';

  function measureDockArea(): { available: number; dock: number } {
    const contentEl = workbenchContentRef.current;
    const changesEl = contentEl?.querySelector<HTMLElement>('.changes-panel');
    const lowerEl = lowerWorkspaceRef.current;
    const changesH = changesEl?.getBoundingClientRect().height ?? 0;
    const lowerH = lowerEl?.getBoundingClientRect().height ?? 0;
    return {
      available: Math.round(changesH + lowerH),
      dock: Math.round(lowerH),
    };
  }

  useLayoutEffect(() => {
    if (!promptFocusRequestedRef.current) return;
    promptFocusRequestedRef.current = false;
    textareaRef.current?.focus();
  }, [showPromptBox, hasExpandedDock]);

  useLayoutEffect(() => {
    if (prevPromptViewKeyRef.current !== null && prevPromptViewKeyRef.current !== viewKey) {
      prevPromptViewKeyRef.current = viewKey;
      prevPromptTextRef.current = promptText;
      setPromptOpen(false);
      return;
    }
    prevPromptViewKeyRef.current = viewKey;
    if (prevPromptTextRef.current.length > 0 && promptText.length === 0) {
      const focused = typeof document !== 'undefined' && document.activeElement === textareaRef.current;
      if (!focused) {
        setPromptOpen(false);
      }
    }
    prevPromptTextRef.current = promptText;
  }, [promptText, viewKey]);

  function handleTogglePrompt() {
    if (hasExpandedDock && worktree) {
      dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: false });
      promptFocusRequestedRef.current = true;
      setPromptOpen(true);
      return;
    }
    if (showPromptBox && !promptText) {
      setPromptOpen(false);
    } else {
      promptFocusRequestedRef.current = true;
      setPromptOpen(true);
      textareaRef.current?.focus();
    }
  }

  useEffect(() => {
    if (!hasExpandedDock) {
      cleanupDockResizeRef.current?.();
    }
  }, [hasExpandedDock]);

  useLayoutEffect(() => {
    if (!hasExpandedDock || typeof dockSizePref !== 'number') return;
    const syncAvailable = () => {
      const { available } = measureDockArea();
      if (available > 0) {
        setDockAvailableHeight((prev) => (prev === available ? prev : available));
      }
    };
    syncAvailable();
    const contentEl = workbenchContentRef.current;
    if (!contentEl || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => syncAvailable());
    observer.observe(contentEl);
    return () => observer.disconnect();
  }, [hasExpandedDock, dockSizePref]);

  useEffect(() => () => {
    cleanupDockResizeRef.current?.();
  }, []);

  function handleDockResizeStart(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || !hasExpandedDock) return;
    event.preventDefault();
    event.stopPropagation();
    cleanupDockResizeRef.current?.();
    const initial = measureDockArea();
    const measuredAvailable = initial.available || dockAvailableHeight;
    if (measuredAvailable > 0) setDockAvailableHeight(measuredAvailable);
    const startDockHeight = initial.dock > 0 ? initial.dock : (dockSplit.dockHeightPx ?? Math.round(measuredAvailable * 0.75));
    const pointerId = event.pointerId;
    const startY = event.clientY;
    let latestPref: DockSizePreference = dockSizePref;
    let didDrag = false;

    const previousCursor = document.body.style.cursor;
    const previousSelect = document.body.style.userSelect;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
    setResizingDock(true);

    const teardown = () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousSelect;
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', finishResize);
      window.removeEventListener('pointercancel', finishResize);
      window.removeEventListener('blur', handleBlur);
      cleanupDockResizeRef.current = null;
      setResizingDock(false);
    };

    const handleMove = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      const deltaY = startY - moveEvent.clientY;
      if (!didDrag && Math.abs(deltaY) < 3) return;
      didDrag = true;
      const currentAvailable = measureDockArea().available || measuredAvailable;
      if (currentAvailable > 0) setDockAvailableHeight(currentAvailable);
      const nextPref = computeDraggedDockPreference(startDockHeight + deltaY, currentAvailable);
      latestPref = nextPref;
      setDockSizePref(nextPref);
    };

    const finishResize = (endEvent: PointerEvent) => {
      if (endEvent.pointerId !== pointerId) return;
      teardown();
      if (didDrag) writeSavedDockSize(latestPref);
    };

    const handleBlur = () => {
      teardown();
      if (didDrag) writeSavedDockSize(latestPref);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', finishResize);
    window.addEventListener('pointercancel', finishResize);
    window.addEventListener('blur', handleBlur);
    cleanupDockResizeRef.current = teardown;
  }

  function handleDockResizeDoubleClick() {
    if (!hasExpandedDock) return;
    const next: DockSizePreference = dockSizePref === null ? 'full' : null;
    setDockSizePref(next);
    writeSavedDockSize(next);
  }

  function handleDockResizeKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (!hasExpandedDock) return;
    const measured = measureDockArea();
    const available = measured.available || dockAvailableHeight;
    if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
      event.preventDefault();
      const base = dockSizePref === 'full'
        ? available
        : measured.dock || dockSplit.dockHeightPx || Math.round(available * 0.75);
      const delta = event.key === 'ArrowUp' ? 32 : (dockSizePref === 'full' ? -56 : -32);
      const next = computeDraggedDockPreference(base + delta, available);
      if (available > 0) setDockAvailableHeight(available);
      setDockSizePref(next);
      writeSavedDockSize(next);
    } else if (event.key === 'Home' || event.key === '0') {
      event.preventDefault();
      setDockSizePref(null);
      writeSavedDockSize(null);
    }
  }

  const model = state.models.find((item) => item.id === state.activeModelId);
  const activeEditor = effectiveEditorFor(state, worktree?.repositoryId);
  const activeCommitLanguage = effectiveCommitLanguageFor(state, worktree?.repositoryId);
  const activeRepoCustomActions = customActionsFor(state, worktree?.repositoryId);
  const activeCustomAction = activeCustomActionFor(state, worktree?.repositoryId);
  const otherRepoCount = repository ? Math.max(0, state.repositories.length - 1) : 0;
  const editor = editors.find((item) => item.id === activeEditor) ?? editors[0];
  const editorShort = state.installedEditors?.length === 0 ? 'IDE' : editor.short;
  const selected = worktree ? selectedFiles(worktree) : [];

  const { setFeedback, activeNotice, setNoticePaused, notify } = useNoticeFeedback(viewKey);

  function clearTransientOverlays() {
    setModelOverlay({ kind: 'closed' });
    setActionStack([]);
  }

  function isEditingCommitInput(): boolean {
    const active = typeof document !== 'undefined' ? document.activeElement : null;
    if (!active) return false;
    if (active === textareaRef.current) return true;
    return !!contextStripRef.current && contextStripRef.current.contains(active) && active.tagName === 'TEXTAREA';
  }

  const windowMode = useWindowMode({
    state,
    latestState,
    worktree,
    hydrated,
    initialState,
    backendHasLoadState: Boolean(backend.loadWorkbenchState),
    windowController,
    dispatch,
    notify,
    setModelOverlay,
    onClearOverlays: clearTransientOverlays,
  });

  const commands = useWorkbenchCommands({
    state,
    latestState,
    worktree,
    model,
    activeEditor,
    activeCommitLanguage,
    activeCustomAction,
    selected,
    promptText,
    compactMode: windowMode.compactMode,
    viewKey,
    backend,
    dispatch,
    notify,
    isEditingCommitInput,
    openConfigureModel: windowMode.openConfigureModel,
    openConfigureModelFromCompact: windowMode.openConfigureModelFromCompact,
    expandFromCompact: windowMode.expandFromCompact,
  });

  const services = useSettingsServices({
    backend,
    hydrated,
    initialState,
    settingsOpen: windowMode.settingsOpen,
    latestState,
  });

  useWorkbenchSync({
    state,
    latestState,
    worktree,
    hydrated,
    setHydrated,
    initialState,
    backend,
    windowController,
    dispatch,
    setFeedback,
    pending: commands.pending,
    inFlight: commands.inFlight,
    hasUpdate: services.hasUpdate,
    pinInFlight: windowMode.pinInFlight,
    desiredCompactHeightRef: windowMode.desiredCompactHeightRef,
    activeSurfaceRef: windowMode.activeSurfaceRef,
    isEditingCommitInput,
    execute: commands.execute,
    openSettingsFromWorkbench: windowMode.openSettingsFromWorkbench,
    openSettingsFromCompact: windowMode.openSettingsFromCompact,
    clearWorkbenchSettingsSession: windowMode.clearWorkbenchSettingsSession,
    onClearOverlays: clearTransientOverlays,
    expandFromCompact: windowMode.expandFromCompact,
  });

  const refreshing = !!worktree && commands.pending.includes(`${worktree.repositoryId}:refresh`);
  const opening = commands.pending.includes('global:openRepository');
  const branchBusy = !!worktree && commands.pending.includes(`${worktree.repositoryId}:write`);
  const pullBusy = !!worktree && commands.pending.includes(`${worktree.repositoryId}:pull`);
  const customActionBusy = !!worktree && commands.pending.includes(`${worktree.repositoryId}:customAction`);
  const localKeepBusy = !!worktree && commands.pending.some((key) => key.startsWith(`${worktree.repositoryId}:localKeep:`));
  const gitBusyForUpdate = branchBusy
    || pullBusy
    || localKeepBusy
    || commands.pending.some((key) => key.endsWith(':cPush') || key.endsWith(':respond') || key.endsWith(':pull') || key.endsWith(':customAction'));
  const busy = refreshing
    || branchBusy
    || pullBusy
    || customActionBusy
    || localKeepBusy
    || commands.pending.some((key) => key.startsWith(`${viewKey}:`));
  const dockBusy = worktree?.customAction?.pendingCallId
    ? commands.pending.includes(`${worktree.worktreeId}:respondCustomAction`)
    : busy;

  function openActionOverlayFromCompact(next: ActionOverlayItem) {
    setActionStack([next]);
    void windowMode.expandFromCompact();
  }

  useLayoutEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = state.locale === 'zh' ? 'zh-CN' : 'en';
    }
  }, [state.locale]);

  if (!hydrated && windowController.isDesktopWindow && !initialState) {
    return <div className="workbench-hydrating" aria-busy="true" />;
  }

  return (
    <I18nProvider locale={state.locale}>
      <MotionRoot>
        <main
          className={`workbench ${windowMode.presentation.windowSizeMode === 'compact' ? 'workbench-compact' : ''}`}
          data-surface={windowMode.presentation.surface}
          data-window-size={windowMode.presentation.windowSizeMode}
          data-window-transition={windowMode.pinBusy ? 'resizing' : 'idle'}
          aria-label={t.workbench.ariaLabel}
        >
          {windowMode.presentation.surface === 'compact' && (
            <m.div
              className="compact-surface"
              initial={instantMotion ? false : { opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
            >
              <CompactBar
                state={state}
                repository={repository}
                worktree={worktree}
                hasModel={!!model}
                customActions={activeRepoCustomActions}
                activeAction={activeCustomAction}
                opening={opening}
                branchBusy={branchBusy}
                pullBusy={pullBusy}
                busy={busy}
                pinned={windowMode.pinned}
                pinBusy={windowMode.pinBusy}
                hasUpdate={services.hasUpdate}
                repositoryReadsAvailable={!!backend.repositoryReadsAvailable}
                notice={activeNotice?.text ? activeNotice : undefined}
                onSwitchWorktree={(worktreeId) => commands.switchWorktree(worktreeId)}
                onOpenRepository={() => void commands.execute({ kind: 'openRepository', requestId: crypto.randomUUID(), locale: state.locale })}
                onBranch={async (kind, branch) => worktree ? commands.execute({ kind, branch, branchToken: worktree.branchToken ?? '', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale }) : false}
                onPull={() => { if (worktree) void commands.execute({ kind: 'pull', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale, ...(model ? { modelId: model.id } : {}) }); }}
                onSelect={(fileId) => { if (worktree) { dispatch({ type: 'select', worktreeId: worktree.worktreeId, fileId }); } }}
                onSelectAll={() => { if (worktree) { dispatch({ type: 'selectAll', worktreeId: worktree.worktreeId }); } }}
                onSelectAction={(actionId) => { if (worktree) dispatch({ type: 'selectActiveAction', repositoryId: worktree.repositoryId, actionId }); }}
                onNewAction={(mode) => openActionOverlayFromCompact({ kind: 'create', mode })}
                onManageActions={() => openActionOverlayFromCompact({ kind: 'manage' })}
                onExpand={() => void windowMode.expandFromCompact()}
                onExpandTarget={(target) => void windowMode.expandFromCompact(target)}
                onPin={() => void windowMode.togglePin()}
                onSettings={(drawerWasOpen) => void windowMode.openSettingsFromCompact({ focusModels: false, drawerWasOpen })}
                onPrimary={commands.primary}
                onConfigureModel={windowMode.openConfigureModelFromCompact}
                onOpenModelSettings={() => void windowMode.openSettingsFromCompact({ focusModels: true })}
                onDismissNotice={() => notify('')}
                onDesiredHeightChange={windowMode.handleDesiredCompactHeight}
              />
            </m.div>
          )}
          {windowMode.presentation.mountWorkbenchContent && (
            <m.div
              ref={workbenchContentRef}
              className="workbench-content"
              inert={windowMode.settingsOpen || modelOverlayOpen || actionOverlayOpen}
              initial={instantMotion ? false : { opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
            >
              <ContextHeader
                key={`header:${viewKey}`}
                state={state}
                worktree={worktree}
                repository={repository}
                opening={opening}
                branchBusy={branchBusy}
                pullBusy={pullBusy}
                pinned={windowMode.pinned}
                hasUpdate={services.hasUpdate}
                pinBusy={windowMode.pinBusy}
                onCollapse={() => void windowMode.collapseToCompact()}
                onPin={() => void windowMode.togglePin()}
                onSettings={() => windowMode.openSettingsFromWorkbench(false)}
                onOpenRepository={() => void commands.execute({ kind: 'openRepository', requestId: crypto.randomUUID(), locale: state.locale })}
                onSwitch={commands.switchWorktree}
                onBranch={async (kind, branch) => worktree ? commands.execute({ kind, branch, branchToken: worktree.branchToken ?? '', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale }) : false}
                onPull={() => { if (worktree) void commands.execute({ kind: 'pull', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale, ...(model ? { modelId: model.id } : {}) }); }}
              />
              <OperationNoticeBar
                viewKey={viewKey}
                notice={activeNotice}
                onPauseChange={setNoticePaused}
                onOpenModelSettings={() => windowMode.openSettingsFromWorkbench(true)}
                onDismiss={() => {
                  setNoticePaused(false);
                  notify('');
                }}
              />
              <ChangesPanel
                worktree={worktree}
                editorShort={editorShort}
                opening={opening}
                repositoryReadsAvailable={!!backend.repositoryReadsAvailable}
                layoutMode={changesLayoutMode}
                peekHeightPx={dockSplit.changesHeightPx}
                resizingDock={resizingDock}
                onOpenRepository={() => void commands.execute({ kind: 'openRepository', requestId: crypto.randomUUID(), locale: state.locale })}
                onSelect={(fileId) => { if (worktree) { dispatch({ type: 'select', worktreeId: worktree.worktreeId, fileId }); } }}
                onSelectAll={() => { if (worktree) { dispatch({ type: 'selectAll', worktreeId: worktree.worktreeId }); } }}
                onSelectFiles={(fileIds) => { if (worktree) { dispatch({ type: 'selectFiles', worktreeId: worktree.worktreeId, fileIds }); } }}
                onSetLocalKeep={commands.setLocalKeep}
                onOpenFile={commands.openFile}
                refreshing={refreshing}
                onRefresh={() => { if (worktree) void commands.execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale }); }}
              />
              <section
                ref={lowerWorkspaceRef}
                className={`lower-workspace ${hasExpandedDock ? 'is-dock-expanded' : ''}`}
                aria-label={t.workbench.lowerWorkspaceLabel}
              >
                <ActionContextStrip
                  worktree={worktree}
                  viewKey={viewKey}
                  promptText={promptText}
                  showPromptBox={showPromptBox}
                  activeCustomAction={activeCustomAction}
                  inert={hasExpandedDock}
                  stripRef={contextStripRef}
                  textareaRef={textareaRef}
                  onClosePrompt={() => setPromptOpen(false)}
                  onEditAction={(action) => setActionStack([{ kind: 'edit', action }])}
                  dispatch={dispatch}
                />
                {worktree && (
                  <MessageDock
                    worktree={worktree}
                    busy={dockBusy}
                    editorShort={editorShort}
                    fullHeight={dockSplit.fullHeight}
                    resizing={resizingDock}
                    dockHeightPx={dockSplit.dockHeightPx}
                    onResizeStart={handleDockResizeStart}
                    onResizeDoubleClick={handleDockResizeDoubleClick}
                    onResizeKeyDown={handleDockResizeKeyDown}
                    onExpanded={(expanded) => dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded })}
                    onDismiss={(message) => {
                      const dismissTarget = worktree.cPush?.frozenTarget ?? targetOf(worktree);
                      const dismissCPushRunId = worktree.cPush?.runId;
                      const dismissActionRunId = worktree.customAction?.runId;
                      const isCustomActionMessage = Boolean(
                        worktree.customAction
                        || message.id.startsWith('done-action-')
                        || message.id.startsWith('interrupted-action-'),
                      );
                      dispatch({ type: 'dismissMessage', worktreeId: worktree.worktreeId, messageId: message.id });
                      if (isCustomActionMessage) {
                        void backend.dismissCustomActionDone?.(targetOf(worktree), dismissActionRunId, message.id, state.locale).catch(() => {});
                      } else if (message.kind === 'done') {
                        void backend.dismissCPushDone?.(dismissTarget, dismissCPushRunId, message.id, state.locale).catch(() => {});
                      }
                    }}
                    onReply={(messageId, text) => dispatch({ type: 'reply', worktreeId: worktree.worktreeId, messageId, text })}
                    onRespond={(message, answer) => void commands.respond(message, answer)}
                    onOpenFile={commands.openFile}
                    onOpenRepoPath={commands.openRepoPath}
                    onCancelCustomAction={commands.cancelActiveCustomAction}
                    onFollowUpCustomAction={commands.followUpCustomAction}
                  />
                )}
              </section>
              <ActionFooter
                key={`footer:${viewKey}`}
                worktree={worktree}
                connections={state.connections}
                models={state.models}
                activeModelId={state.activeModelId}
                customActions={activeRepoCustomActions}
                activeAction={activeCustomAction}
                busy={busy}
                promptOpen={showPromptBox && !hasExpandedDock}
                hasPromptText={promptText.length > 0}
                hasPromptAbove={showPromptBox && !hasExpandedDock && !hasCollapsedDockBar}
                onTogglePrompt={handleTogglePrompt}
                onConfigureModel={windowMode.openConfigureModel}
                onManageModels={() => windowMode.openSettingsFromWorkbench(true)}
                onSelectModel={(id) => void commands.selectModel(id)}
                onSelectAction={(actionId) => { if (worktree) dispatch({ type: 'selectActiveAction', repositoryId: worktree.repositoryId, actionId }); }}
                onNewAction={(mode) => setActionStack([{ kind: 'create', mode }])}
                onManageActions={() => setActionStack([{ kind: 'manage' }])}
                onPrimary={commands.primary}
              />
            </m.div>
          )}
          <WorkbenchOverlays
            state={state}
            backend={backend}
            presentation={windowMode.presentation}
            settingsSession={windowMode.settingsSession}
            settingsOpen={windowMode.settingsOpen}
            pinBusy={windowMode.pinBusy}
            modelOverlay={modelOverlay}
            setModelOverlay={setModelOverlay}
            actionStack={actionStack}
            setActionStack={setActionStack}
            worktree={worktree}
            repository={repository}
            viewKey={viewKey}
            model={model}
            activeEditor={activeEditor}
            activeCommitLanguage={activeCommitLanguage}
            activeRepoCustomActions={activeRepoCustomActions}
            activeCustomAction={activeCustomAction}
            otherRepoCount={otherRepoCount}
            activeNotice={activeNotice}
            skillsState={services.skillsState}
            skillsBusy={services.skillsBusy}
            skillsError={services.skillsError}
            skillsNotice={services.skillsNotice}
            onSyncSkills={services.handleSyncSkills}
            onToggleSkill={services.handleToggleSkill}
            onResetSkills={services.handleResetSkills}
            updateState={services.updateState}
            gitBusyForUpdate={gitBusyForUpdate}
            onCheckUpdate={services.handleCheckUpdate}
            onInstallUpdate={services.handleInstallUpdate}
            onCancelUpdate={services.handleCancelUpdate}
            onOpenReleasePage={services.handleOpenReleasePage}
            onSelectModel={commands.selectModel}
            onDeleteModel={commands.deleteModel}
            onDeleteProviderConnection={commands.deleteProviderConnection}
            onSyncActionToAllRepos={commands.syncActionToAllRepos}
            onCloseSettings={windowMode.closeSettings}
            dispatch={dispatch}
            notify={notify}
          />
        </main>
      </MotionRoot>
    </I18nProvider>
  );
}
