import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import type { AppTheme } from '../../../shared/theme-protocol';
import { desktopWindowController, type WindowController } from '../../desktop/window';
import { getMessages, I18nProvider } from '../../i18n';
import { MotionRoot } from '../../ui/motion';
import { useResolvedTheme } from '../../ui/theme';
import { disconnectedBackend, type WorkbenchBackend } from './commands';
import { ActionContextStrip } from './components/ActionContextStrip';
import { ActionFooter } from './components/ActionFooter';
import { ChangesPanel } from './components/ChangesPanel';
import { CompactBar } from './components/CompactBar';
import { ContextHeader } from './components/ContextHeader';
import { MessageDock } from './components/MessageDock';
import { OperationNoticeBar } from './components/OperationNoticeBar';
import { WorkbenchOverlays } from './components/WorkbenchOverlays';
import { useDockResize } from './hooks/useDockResize';
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
  const [promptOpen, setPromptOpen] = useState(false);
  const [hydrated, setHydrated] = useState(() => !!initialState || !backend.loadWorkbenchState);

  const workbenchContentRef = useRef<HTMLDivElement>(null);
  const lowerWorkspaceRef = useRef<HTMLElement>(null);
  const contextStripRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const promptFocusRequestedRef = useRef(false);
  const prevPromptViewKeyRef = useRef<string | null>(null);
  const prevPromptTextRef = useRef('');

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

  const model = state.models.find((item) => item.id === state.activeModelId);
  const activeEditor = effectiveEditorFor(state, worktree?.repositoryId);
  const activeCommitLanguage = effectiveCommitLanguageFor(state, worktree?.repositoryId);
  const activeRepoCustomActions = customActionsFor(state, worktree?.repositoryId);
  const activeCustomAction = activeCustomActionFor(state, worktree?.repositoryId);
  const otherRepositories = repository
    ? state.repositories
        .filter((item) => item.id !== repository.id)
        .map((item) => ({
          id: item.id,
          name: item.name,
          existingActions: customActionsFor(state, item.id),
        }))
    : [];
  const otherRepoCount = otherRepositories.length;
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
    inFlight: commands.inFlight,
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
  const hasLivePendingQuestion = Boolean(
    worktree?.customAction?.pendingCallId || worktree?.cPush?.pendingCallId,
  );
  const dockBusy = hasLivePendingQuestion
    ? commands.pending.includes(`${worktree!.worktreeId}:respondCustomAction`)
    : busy;

  function openActionOverlayFromCompact(next: ActionOverlayItem) {
    setActionStack([next]);
    void windowMode.expandFromCompact();
  }

  const syncNativeWindowTheme = useCallback(
    (nextTheme: AppTheme) => windowController.setTheme?.(nextTheme) ?? Promise.resolve(),
    [windowController],
  );
  useResolvedTheme(state.theme, syncNativeWindowTheme);

  useLayoutEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = state.locale === 'zh' ? 'zh-CN' : 'en';
    }
  }, [state.locale]);

  useEffect(() => {
    setNoticePaused(false);
  }, [windowMode.presentation.surface, setNoticePaused]);

  const workbenchContentMounted = windowMode.presentation.mountWorkbenchContent
    && (hydrated || !windowController.isDesktopWindow || Boolean(initialState));
  const canMeasureDock = hasExpandedDock
    && workbenchContentMounted
    && windowMode.presentation.windowSizeMode === 'full'
    && windowMode.transitionKind !== 'collapsing';
  const {
    dockSplit,
    changesLayoutMode,
    resizingDock,
    handleDockResizeStart,
    handleDockResizeDoubleClick,
    handleDockResizeKeyDown,
  } = useDockResize({
    hasExpandedDock,
    canMeasureDock,
    workbenchContentRef,
    lowerWorkspaceRef,
  });

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
          data-window-transition-kind={windowMode.transitionKind}
          style={
            windowMode.transitionHeightPx !== null
              ? ({ '--window-transition-height': `${windowMode.transitionHeightPx}px` } as CSSProperties)
              : undefined
          }
          aria-label={t.workbench.ariaLabel}
        >
          {windowMode.presentation.surface === 'compact' && (
            <div className="compact-surface">
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
                onOpenModelSettings={(transientExpanded) => {
                  setNoticePaused(false);
                  void windowMode.openSettingsFromCompact({ focusModels: true, drawerWasOpen: transientExpanded ?? true });
                }}
                onNoticePauseChange={setNoticePaused}
                onDismissNotice={() => {
                  setNoticePaused(false);
                  notify('');
                }}
                onDesiredHeightChange={windowMode.handleDesiredCompactHeight}
              />
            </div>
          )}
          {windowMode.presentation.mountWorkbenchContent && (
            <div
              ref={workbenchContentRef}
              className="workbench-content"
              inert={windowMode.settingsOpen || modelOverlayOpen || actionOverlayOpen}
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
                      const dismissRunId = worktree.customAction?.runId ?? worktree.cPush?.runId;
                      dispatch({ type: 'dismissMessage', worktreeId: worktree.worktreeId, messageId: message.id });
                      void backend.dismissSessionDone?.(dismissTarget, dismissRunId, message.id, state.locale).catch(() => {});
                    }}
                    onReply={(messageId, text) => dispatch({ type: 'reply', worktreeId: worktree.worktreeId, messageId, text })}
                    onRespond={(message, answer) => commands.respond(message, answer)}
                    onOpenFile={commands.openFile}
                    onOpenRepoPath={commands.openRepoPath}
                    onCancelCustomAction={commands.cancelActiveCustomAction}
                    onFollowUpCustomAction={(message, text) => commands.followUpCustomAction(message, text)}
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
            </div>
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
            otherRepositories={otherRepositories}
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
