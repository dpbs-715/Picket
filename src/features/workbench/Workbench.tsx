import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from 'react';
import type { UpdateStateSnapshot } from '../../../shared/update-protocol';
import {
  WINDOW_DIMENSIONS,
  type WindowPosition,
  type WindowSize,
} from '../../../shared/workbench-persistence-protocol';
import { initialBrowserUpdateSnapshot } from '../../desktop/updater';
import {
  subscribeTrayMainAction,
  subscribeTrayRefreshRequested,
  syncTrayState,
} from '../../desktop/tray';
import {
  clampCompactHeight,
  desktopWindowController,
  normalizeCompactWidth,
  normalizeFullWindowSize,
  type WindowController,
} from '../../desktop/window';
import { buildTrayStateSnapshot } from '../tray/trayModel';
import { getMessages, I18nProvider } from '../../i18n';
import { Icon, IconButton } from '../../ui/controls';
import {
  Collapse,
  MOTION_DURATION,
  MOTION_EASE,
  MotionPresence,
  MotionRoot,
  PresenceNotice,
  m,
  useInstantMotion,
} from '../../ui/motion';
import { ModelEntryForm } from '../settings/ModelEntryForm';
import type { ModelConfiguration, ProviderConnection } from '../settings/modelProviders';
import { ProviderConnectionForm } from '../settings/ProviderConnectionForm';
import { SettingsPanel } from '../settings/SettingsPanel';
import { disconnectedBackend } from './commands';
import type { WorkbenchBackend, WorkbenchCommand } from './commands';
import { ActionFooter } from './components/ActionFooter';
import { ChangesPanel } from './components/ChangesPanel';
import { CompactBar } from './components/CompactBar';
import { CommitPanel } from './components/CommitPanel';
import { ContextHeader } from './components/ContextHeader';
import { MessageDock } from './components/MessageDock';
import { ReviewPanel } from './components/ReviewPanel';
import { serializeWorkbenchPersistence } from './state/repositories';
import {
  activeValidPlan,
  computeNextCompactPreferences,
  computeNextPinPreferences,
  defaultWindowPreferences,
  editors,
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  emptyWorkbench,
  isActiveSession,
  isModelErrorText,
  isPlanOutdated,
  orderedMessages,
  resolveWorkbenchPresentation,
  selectedFiles,
  splitPath,
  targetOf,
  workbenchReducer,
  type CompactExpandTarget,
  type Message,
  type SettingsReturnTarget,
  type WorkbenchState,
  type WorkbenchSurface,
  type Worktree,
} from './state/workbench';
import './workbench.css';

interface Props {
  initialState?: WorkbenchState;
  backend?: WorkbenchBackend;
  windowController?: WindowController;
}

interface NoticeItem {
  text: string;
  showModelSettings?: boolean;
}

type PendingModeFocus = { kind: CompactExpandTarget | 'addModel' };

interface CompactReturnSnapshot {
  position: WindowPosition;
  size: WindowSize;
}

interface SettingsSession {
  returnTo: SettingsReturnTarget;
  focusModels: boolean;
  /** Captured immediately before expanding from compact mode so closing Settings restores exact compact size & position. */
  compactSnapshot?: CompactReturnSnapshot;
}

type ModelOverlayState =
  | { kind: 'closed' }
  | { kind: 'createProvider'; keepActiveModel?: boolean }
  | { kind: 'editProvider'; connection: ProviderConnection }
  | { kind: 'addModel'; connection: ProviderConnection }
  | { kind: 'editModel'; connection: ProviderConnection; model: ModelConfiguration };

export function Workbench({
  initialState,
  backend = disconnectedBackend,
  windowController = desktopWindowController,
}: Props) {
  const [state, dispatch] = useReducer(workbenchReducer, initialState, (initial) => initial ?? emptyWorkbench());
  const [settingsSession, setSettingsSession] = useState<SettingsSession | null>(null);
  const settingsSessionRef = useRef<SettingsSession | null>(null);
  settingsSessionRef.current = settingsSession;
  const [modelOverlay, setModelOverlay] = useState<ModelOverlayState>({ kind: 'closed' });
  const modelOverlayOpen = modelOverlay.kind !== 'closed';
  const [emptyTab, setEmptyTab] = useState<'commit' | 'review'>('commit');
  const [promptOpen, setPromptOpen] = useState(false);
  const promptFocusRequestedRef = useRef(false);
  const [feedback, setFeedback] = useState<Record<string, NoticeItem>>({});
  const [pending, setPending] = useState<string[]>([]);
  const inFlight = useRef(new Set<string>());
  const cancelledRequests = useRef(new Set<string>());
  const latestLocalKeepRequestByRepo = useRef<Record<string, string>>({});
  const [pinBusy, setPinBusy] = useState(false);
  const pinInFlight = useRef(false);
  const [updateState, setUpdateState] = useState<UpdateStateSnapshot>(() => initialBrowserUpdateSnapshot());
  const desiredCompactHeightRef = useRef<number>(WINDOW_DIMENSIONS.compactDefaultHeight);
  const collapsedCompactHeightRef = useRef<number>(WINDOW_DIMENSIONS.compactDefaultHeight);
  const pendingModeFocusRef = useRef<PendingModeFocus | null>(null);
  const previousSessionStatusRef = useRef<Record<string, Worktree['sessionStatus']> | null>(null);
  const [hydrated, setHydrated] = useState(() => !!initialState || !backend.loadWorkbenchState);
  const workbenchReadyNotifiedRef = useRef(false);
  const saveRevision = useRef(0);
  const lastPersistedKey = useRef<string | null>(null);
  const lastStructuralKey = useRef<string | null>(null);
  const lastTraySnapshotKey = useRef<string | null>(null);
  const trayActionHandlerRef = useRef<(action: import('../../../shared/tray-protocol').TrayMainWindowAction) => void>(() => {});
  const trayRefreshHandlerRef = useRef<() => void>(() => {});
  const latestState = useRef(state);
  latestState.current = state;
  const commitPanelRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const tabListRef = useRef<HTMLDivElement>(null);
  const commitTabRef = useRef<HTMLButtonElement>(null);
  const reviewTabRef = useRef<HTMLButtonElement>(null);
  const [tabIndicator, setTabIndicator] = useState<{ left: number; width: number } | null>(null);
  const instantMotion = useInstantMotion();
  const t = getMessages(state.locale);
  const pinned = state.windowPrefs.pinned;
  const compactMode = state.windowPrefs.compactMode;
  const presentation = resolveWorkbenchPresentation(compactMode, settingsSession?.returnTo ?? null);
  const settingsOpen = presentation.showSettings;
  const activeSurfaceRef = useRef<WorkbenchSurface>(presentation.surface);
  activeSurfaceRef.current = presentation.surface;
  const worktree = state.worktrees.find((tree) => tree.worktreeId === state.activeWorktreeId);
  const repository = state.repositories.find((repository) => repository.id === worktree?.repositoryId);
  const viewKey = worktree?.worktreeId ?? 'empty';
  const tab = worktree?.tab ?? emptyTab;
  const promptText = worktree ? worktree.prompt.text : state.emptyPrompt;
  const showPromptBox = promptOpen || promptText.length > 0;
  const prevViewKeyRef = useRef(viewKey);
  const prevPromptTextRef = useRef(promptText);
  const model = state.models.find((model) => model.id === state.activeModelId);
  const activeEditor = effectiveEditorFor(state, worktree?.repositoryId);
  const activeCommitLanguage = effectiveCommitLanguageFor(state, worktree?.repositoryId);
  const editor = editors.find((item) => item.id === activeEditor) ?? editors[0];
  const editorShort = state.installedEditors?.length === 0 ? 'IDE' : editor.short;
  const selected = worktree ? selectedFiles(worktree) : [];
  const canUseModel = !!model && selected.length > 0 && !worktree?.readError;
  const canPlan = canUseModel && !worktree?.detached && (!worktree || !isActiveSession(worktree) || !!worktree.cPush?.planOnly);
  const planOutdated = worktree ? isPlanOutdated(worktree) : false;
  const refreshing = !!worktree && pending.includes(`${worktree.repositoryId}:refresh`);
  const opening = pending.includes('global:openRepository');
  const branchBusy = !!worktree && pending.includes(`${worktree.repositoryId}:write`);
  const pullBusy = !!worktree && pending.includes(`${worktree.repositoryId}:pull`);
  const localKeepBusy = !!worktree && pending.some((key) => key.startsWith(`${worktree.repositoryId}:localKeep:`));
  const gitBusyForUpdate = branchBusy
    || pullBusy
    || localKeepBusy
    || pending.some((key) => key.endsWith(':cPush') || key.endsWith(':respond') || key.endsWith(':pull'));
  const hasUpdate = updateState.phase === 'available'
    || updateState.phase === 'downloading'
    || updateState.phase === 'installing';
  const busy = refreshing || branchBusy || pullBusy || localKeepBusy || pending.some((key) => key.startsWith(`${viewKey}:`) && !key.endsWith(':planCPush') && !key.endsWith(':review'));

  function isEditingCommitInput(): boolean {
    const active = typeof document !== 'undefined' ? document.activeElement : null;
    if (!active) return false;
    if (active === textareaRef.current) return true;
    return !!commitPanelRef.current && commitPanelRef.current.contains(active) && active.tagName === 'TEXTAREA';
  }

  function openSettingsFromWorkbench(focusModels = false) {
    const next: SettingsSession = { returnTo: 'workbench', focusModels };
    settingsSessionRef.current = next;
    setSettingsSession(next);
  }

  async function openSettingsFromCompact(options: { focusModels?: boolean; drawerWasOpen?: boolean } = {}) {
    if (pinInFlight.current || activeSurfaceRef.current !== 'compact') return;
    const currentPrefs = latestState.current.windowPrefs;
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);

    if (!windowController.isDesktopWindow) {
      try {
        await windowController.applyWindowMode({
          pinned: currentPrefs.pinned,
          compactMode: false,
          fullSize,
          placement: 'anchorTopLeft',
          locale: state.locale,
        });
      } catch (error) {
        notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
      }
      return;
    }

    const focusModels = Boolean(options.focusModels);
    const fallbackCompactHeight = clampCompactHeight(collapsedCompactHeightRef.current);
    const provisionalSession: SettingsSession = {
      returnTo: 'compact',
      focusModels,
      ...(currentPrefs.compactPosition
        ? {
            compactSnapshot: {
              position: currentPrefs.compactPosition,
              size: { width: WINDOW_DIMENSIONS.compactWidth, height: fallbackCompactHeight },
            },
          }
        : {}),
    };

    pinInFlight.current = true;
    setPinBusy(true);
    // Switch surface directly to standalone Settings BEFORE enlarging the window so neither
    // a stretched CompactBar nor the full workbench content can ever flash during expansion.
    settingsSessionRef.current = provisionalSession;
    setSettingsSession(provisionalSession);

    try {
      const applied = await windowController.applyWindowMode({
        pinned: currentPrefs.pinned,
        compactMode: false,
        fullSize,
        placement: 'anchorTopLeft',
        locale: state.locale,
      });
      const compactSize: WindowSize = {
        width: normalizeCompactWidth(applied.previousSize.width),
        height: options.drawerWasOpen
          ? fallbackCompactHeight
          : clampCompactHeight(applied.previousSize.height),
      };
      const settledSession: SettingsSession = {
        returnTo: 'compact',
        focusModels,
        compactSnapshot: {
          position: applied.previousPosition,
          size: compactSize,
        },
      };
      settingsSessionRef.current = settledSession;
      setSettingsSession(settledSession);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...latestState.current.windowPrefs,
          pinned: applied.pinned,
          compactMode: true,
          compactPosition: applied.previousPosition,
        },
      });
    } catch (error) {
      settingsSessionRef.current = null;
      setSettingsSession(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    } finally {
      pinInFlight.current = false;
      setPinBusy(false);
    }
  }

  async function closeSettings() {
    const session = settingsSessionRef.current;
    if (!session) return;

    if (session.returnTo === 'workbench') {
      settingsSessionRef.current = null;
      setSettingsSession(null);
      return;
    }

    if (pinInFlight.current) return;
    const currentPrefs = latestState.current.windowPrefs;
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);
    const snapshot = session.compactSnapshot;
    const targetPosition = snapshot?.position ?? currentPrefs.compactPosition;
    const targetWidth = normalizeCompactWidth(snapshot?.size.width);
    const targetHeight = clampCompactHeight(snapshot?.size.height ?? collapsedCompactHeightRef.current);

    pinInFlight.current = true;
    setPinBusy(true);
    try {
      // Shrink and move back to pre-settings compact geometry FIRST while standalone Settings
      // remains mounted, then swap directly to CompactBar without ever mounting full workbench.
      const applied = await windowController.applyWindowMode({
        pinned: currentPrefs.pinned,
        compactMode: true,
        compactWidth: targetWidth,
        compactHeight: targetHeight,
        fullSize,
        placement: targetPosition ? 'restorePosition' : 'anchorTopLeft',
        ...(targetPosition ? { targetPosition } : {}),
        locale: state.locale,
      });
      desiredCompactHeightRef.current = applied.size.height;
      collapsedCompactHeightRef.current = applied.size.height;
      setModelOverlay({ kind: 'closed' });
      settingsSessionRef.current = null;
      setSettingsSession(null);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...latestState.current.windowPrefs,
          pinned: applied.pinned,
          compactMode: true,
          compactPosition: applied.position,
        },
      });
    } catch (error) {
      setModelOverlay({ kind: 'closed' });
      settingsSessionRef.current = null;
      setSettingsSession(null);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    } finally {
      pinInFlight.current = false;
      setPinBusy(false);
    }
  }

  useLayoutEffect(() => {
    if (!hydrated || !windowController.isDesktopWindow || workbenchReadyNotifiedRef.current) return;
    // Defer by one short macrotask (rather than rAF, which WebKit can throttle in hidden windows)
    // so the hydrated DOM has flushed before signaling the startup splash to leap and reveal.
    const timer = window.setTimeout(() => {
      if (workbenchReadyNotifiedRef.current) return;
      workbenchReadyNotifiedRef.current = true;
      void windowController.notifyWorkbenchReady?.();
    }, 16);
    return () => window.clearTimeout(timer);
  }, [hydrated, windowController]);

  useLayoutEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = state.locale === 'zh' ? 'zh-CN' : 'en';
    }
  }, [state.locale]);

  useLayoutEffect(() => {
    if (!promptFocusRequestedRef.current) return;
    promptFocusRequestedRef.current = false;
    textareaRef.current?.focus();
  }, [showPromptBox]);

  useLayoutEffect(() => {
    if (prevViewKeyRef.current !== viewKey) {
      prevViewKeyRef.current = viewKey;
      prevPromptTextRef.current = promptText;
      setPromptOpen(false);
      return;
    }
    if (prevPromptTextRef.current.length > 0 && promptText.length === 0) {
      const focused = typeof document !== 'undefined' && document.activeElement === textareaRef.current;
      if (!focused) {
        setPromptOpen(false);
      }
    }
    prevPromptTextRef.current = promptText;
  }, [promptText, viewKey]);

  useLayoutEffect(() => {
    const pendingFocus = pendingModeFocusRef.current;
    pendingModeFocusRef.current = null;
    if (!pendingFocus || typeof document === 'undefined') return;
    if (pendingFocus.kind === 'changes') {
      const panel = document.querySelector<HTMLElement>('.changes-panel');
      panel?.scrollIntoView({ block: 'nearest' });
      (panel?.querySelector<HTMLElement>('input[type="checkbox"], button') ?? panel)?.focus();
      return;
    }
    if (pendingFocus.kind === 'dock') {
      const dock = document.querySelector<HTMLElement>('.message-expanded, .message-collapsed');
      (dock?.querySelector<HTMLElement>('.message-actions button, .message-reply input, button') ?? dock)?.focus();
      return;
    }
    if (pendingFocus.kind === 'plan') {
      const region = document.querySelector<HTMLElement>('.plan-region');
      region?.scrollIntoView({ block: 'nearest' });
      (region?.querySelector<HTMLElement>('textarea, button:not(:disabled)') ?? region)?.focus();
      return;
    }
    if (pendingFocus.kind === 'repo') {
      document.querySelector<HTMLElement>('.repository-open-button, .repository-button')?.focus();
    }
  }, [presentation.surface]);

  useEffect(() => backend.subscribe?.((event) => dispatch({ type: 'event', event })), [backend]);
  useEffect(() => {
    let mounted = true;
    void backend.getUpdateStatus?.().then((snapshot) => {
      if (mounted) setUpdateState(snapshot);
    }).catch(() => {});
    const unsubscribe = backend.subscribeUpdateState?.((snapshot) => {
      if (mounted) setUpdateState(snapshot);
    });
    return () => {
      mounted = false;
      unsubscribe?.();
    };
  }, [backend]);
  useEffect(() => {
    const isSmoke = Boolean((globalThis as { __PICKET_UI_SMOKE__?: boolean }).__PICKET_UI_SMOKE__);
    if (!hydrated || initialState || !backend.repositoryReadsAvailable || !backend.checkForUpdate || isSmoke) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void backend.checkForUpdate?.(false, latestState.current.locale).then((snapshot) => {
        if (!cancelled) setUpdateState(snapshot);
      }).catch(() => {});
    }, 4_000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [backend, hydrated, initialState]);
  useEffect(() => {
    if (initialState || !backend.loadWorkbenchState) return;
    let mounted = true;
    void backend.loadWorkbenchState().then(async (restored) => {
      if (!mounted) return;
      let effectiveRestored = restored;
      if (windowController.isDesktopWindow) {
        if (restored.windowPreferences && !latestState.current.settingsModified && !pinInFlight.current) {
          const saved = restored.windowPreferences;
          const targetPos = saved.compactMode ? saved.compactPosition : saved.fullPosition;
          try {
            const applied = await windowController.applyWindowMode({
              pinned: saved.pinned,
              compactMode: saved.compactMode,
              compactHeight: desiredCompactHeightRef.current,
              fullSize: normalizeFullWindowSize(saved.fullSize),
              placement: targetPos ? 'restorePosition' : 'anchorTopLeft',
              targetPosition: targetPos,
              instant: true,
              locale: restored.locale ?? latestState.current.locale,
            });
            effectiveRestored = {
              ...restored,
              windowPreferences: {
                ...saved,
                pinned: applied.pinned,
                fullSize: normalizeFullWindowSize(saved.fullSize),
                ...(saved.compactMode
                  ? { compactPosition: applied.position }
                  : { fullPosition: applied.position }),
              },
            };
          } catch {
            // Ignore window restore failure so repository/model hydration still completes cleanly.
          }
        }
      }
      if (!mounted) return;
      dispatch({ type: 'workbenchHydrated', restored: effectiveRestored });
      if (restored.warning) {
        const key = restored.activeWorktreeId ?? restored.snapshots[0]?.activeWorktreeId ?? 'empty';
        setFeedback((previous) => ({ ...previous, [key]: { text: restored.warning! } }));
      }
      if (windowController.isDesktopWindow && !restored.windowPreferences && !latestState.current.settingsModified && !pinInFlight.current) {
        try {
          const snap = await windowController.readSnapshot();
          if (mounted && snap && !latestState.current.settingsModified && !pinInFlight.current) {
            dispatch({
              type: 'windowSnapshotSynced',
              pinned: snap.pinned,
              size: normalizeFullWindowSize(snap.size),
              position: snap.position,
            });
          }
        } catch {
          if (mounted) {
            setFeedback((previous) => ({
              ...previous,
              [latestState.current.activeWorktreeId ?? 'empty']: { text: getMessages(latestState.current.locale).workbench.readPinFailedFallback },
            }));
          }
        }
      }
      if (mounted) setHydrated(true);
    }).catch((error: unknown) => {
      if (!mounted) return;
      const message = error instanceof Error ? error.message : getMessages(latestState.current.locale).workbench.loadStateFailedFallback;
      setFeedback((previous) => ({ ...previous, empty: { text: message } }));
      setHydrated(true);
    });
    return () => { mounted = false; };
  }, [backend, initialState, windowController]);

  useEffect(() => {
    if (!hydrated || !windowController.isDesktopWindow) return;
    let mounted = true;
    if ((initialState || !backend.loadWorkbenchState)
      && !latestState.current.settingsModified
      && !latestState.current.windowPrefs.compactMode
      && !latestState.current.windowPrefs.fullPosition
      && !pinInFlight.current) {
      void windowController.readSnapshot().then((snap) => {
        if (!mounted || !snap || latestState.current.settingsModified || pinInFlight.current) return;
        dispatch({
          type: 'windowSnapshotSynced',
          pinned: snap.pinned,
          size: normalizeFullWindowSize(snap.size),
          position: snap.position,
        });
      }).catch(() => {});
    }
    const unsubscribeGeometry = windowController.subscribeGeometry?.((observed) => {
      if (!mounted || settingsSessionRef.current?.returnTo === 'compact') return;
      dispatch({ type: 'windowGeometryObserved', position: observed.position, size: observed.size });
    });
    const unsubscribeTap = windowController.subscribeNotificationTap?.(() => {
      // Focusing the window on notification click must never auto-expand compact mode.
    });
    return () => {
      mounted = false;
      unsubscribeGeometry?.();
      unsubscribeTap?.();
    };
  }, [hydrated, initialState?.activeWorktreeId, windowController]);

  useEffect(() => {
    if (!hydrated) return;
    const nextMap: Record<string, Worktree['sessionStatus']> = {};
    for (const tree of state.worktrees) {
      nextMap[tree.worktreeId] = tree.sessionStatus;
    }
    const previousMap = previousSessionStatusRef.current;
    previousSessionStatusRef.current = nextMap;
    if (!previousMap) return;

    const notifCatalog = getMessages(state.locale).compact.notifications;
    for (const tree of state.worktrees) {
      const prevStatus = previousMap[tree.worktreeId];
      if (prevStatus === tree.sessionStatus) continue;
      if (tree.sessionStatus !== 'needsDecision' && tree.sessionStatus !== 'pushFailed' && tree.sessionStatus !== 'conflict') {
        continue;
      }
      const title = tree.sessionStatus === 'conflict'
        ? notifCatalog.conflictTitle
        : tree.sessionStatus === 'pushFailed'
          ? notifCatalog.pushFailedTitle
          : notifCatalog.needsDecisionTitle;
      const topMessage = orderedMessages(tree.messages)[0];
      const body = topMessage?.title || notifCatalog.fallbackBody(tree.branch);
      void windowController.notifyBackgroundAttention({ title, body });
    }
  }, [hydrated, state.locale, state.worktrees, windowController]);

  const persistenceSnapshot = serializeWorkbenchPersistence(state, 0);
  const persistenceKey = JSON.stringify(persistenceSnapshot);
  const structuralKey = JSON.stringify([
    state.locale,
    state.editor,
    state.defaultCommitLanguage,
    state.repositorySettings,
    state.windowPrefs.pinned,
    state.windowPrefs.compactMode,
    state.windowPrefs.pinnedExpanded,
    state.activeWorktreeId,
    state.worktrees.map((tree) => [tree.worktreeId, tree.selectedIds, tree.scopeVersion, tree.tab]),
  ]);

  useEffect(() => {
    if (!hydrated || !backend.saveWorkbenchState) return;
    if (lastPersistedKey.current === null) {
      lastStructuralKey.current = structuralKey;
      if (!state.settingsModified && state.navigationRevision === 0 && !state.emptyPrompt) {
        lastPersistedKey.current = persistenceKey;
        return;
      }
    }
    if (persistenceKey === lastPersistedKey.current) {
      lastStructuralKey.current = structuralKey;
      return;
    }
    const immediate = structuralKey !== lastStructuralKey.current;
    lastStructuralKey.current = structuralKey;
    const persistNow = () => {
      const current = serializeWorkbenchPersistence(latestState.current, 0);
      const currentKey = JSON.stringify(current);
      if (currentKey === lastPersistedKey.current) return;
      lastPersistedKey.current = currentKey;
      const revision = ++saveRevision.current;
      void backend.saveWorkbenchState!({ ...current, revision }).catch((error: unknown) => {
        const message = error instanceof Error
          ? error.message
          : getMessages(latestState.current.locale).workbench.saveStateFailedFallback;
        setFeedback((previous) => ({ ...previous, [latestState.current.activeWorktreeId ?? 'empty']: { text: message } }));
      });
    };
    if (immediate) {
      persistNow();
      return;
    }
    const timer = setTimeout(persistNow, 80);
    return () => clearTimeout(timer);
  }, [backend, hydrated, persistenceKey, structuralKey]);

  useEffect(() => {
    if (!hydrated || !backend.saveWorkbenchState) return;
    const flush = () => {
      const current = serializeWorkbenchPersistence(latestState.current, 0);
      const currentKey = JSON.stringify(current);
      if (currentKey === lastPersistedKey.current) return;
      lastPersistedKey.current = currentKey;
      const revision = ++saveRevision.current;
      void backend.saveWorkbenchState!({ ...current, revision }).catch(() => {});
    };
    window.addEventListener('pagehide', flush);
    return () => window.removeEventListener('pagehide', flush);
  }, [backend, hydrated]);

  useEffect(() => {
    if (!backend.repositoryReadsAvailable || !worktree) return;
    const refresh = () => {
      if (inFlight.current.has('global:openRepository')) return;
      void execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: latestState.current.locale }, true);
    };
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [backend, worktree?.worktreeId, worktree?.scopeVersion]);

  const anyRepoRefreshing = pending.some((key) => key.endsWith(':refresh'));
  const traySnapshot = buildTrayStateSnapshot(state, {
    hasUpdate,
    refreshing: anyRepoRefreshing,
  });
  const traySnapshotKey = JSON.stringify(traySnapshot);

  useEffect(() => {
    if (!hydrated || initialState || !windowController.isDesktopWindow) return;
    if (traySnapshotKey === lastTraySnapshotKey.current) return;
    lastTraySnapshotKey.current = traySnapshotKey;
    void syncTrayState(traySnapshot).catch(() => {});
  }, [hydrated, initialState, traySnapshot, traySnapshotKey, windowController.isDesktopWindow]);

  trayRefreshHandlerRef.current = () => {
    if (!backend.repositoryReadsAvailable || inFlight.current.has('global:openRepository')) return;
    const current = latestState.current;
    const seenRepos = new Set<string>();
    const activeTree = current.worktrees.find((tree) => tree.worktreeId === current.activeWorktreeId);
    const orderedTrees = activeTree
      ? [activeTree, ...current.worktrees.filter((tree) => tree.worktreeId !== activeTree.worktreeId)]
      : current.worktrees;
    for (const tree of orderedTrees) {
      if (seenRepos.has(tree.repositoryId)) continue;
      seenRepos.add(tree.repositoryId);
      if (inFlight.current.has(`${tree.repositoryId}:refresh`) || inFlight.current.has(`${tree.repositoryId}:write`)) {
        continue;
      }
      void execute(
        { kind: 'refresh', target: targetOf(tree), requestId: crypto.randomUUID(), locale: current.locale },
        true,
      );
    }
  };

  trayActionHandlerRef.current = (action) => {
    if (action.kind === 'openSettings') {
      if (activeSurfaceRef.current === 'compact') {
        void openSettingsFromCompact({ focusModels: false });
      } else {
        openSettingsFromWorkbench(false);
      }
      return;
    }
    if (action.kind === 'focusWorktree') {
      const targetTree = latestState.current.worktrees.find((tree) => tree.worktreeId === action.worktreeId);
      if (!targetTree) return;
      if (settingsSessionRef.current?.returnTo === 'workbench') {
        settingsSessionRef.current = null;
        setSettingsSession(null);
      }
      setModelOverlay({ kind: 'closed' });
      dispatch({ type: 'switch', worktreeId: targetTree.worktreeId, expand: action.openDock || undefined });
      if (action.openDock && activeSurfaceRef.current === 'compact') {
        void expandFromCompact('dock');
      }
      if (backend.repositoryReadsAvailable) {
        void execute(
          { kind: 'refresh', target: targetOf(targetTree), requestId: crypto.randomUUID(), locale: latestState.current.locale },
          true,
        );
      }
    }
  };

  useEffect(() => {
    if (!windowController.isDesktopWindow || initialState) return;
    const unsubRefresh = subscribeTrayRefreshRequested(() => {
      trayRefreshHandlerRef.current();
    });
    const unsubAction = subscribeTrayMainAction((action) => {
      trayActionHandlerRef.current(action);
    });
    return () => {
      unsubRefresh();
      unsubAction();
    };
  }, [initialState, windowController.isDesktopWindow]);

  function notify(text: string, key = viewKey, showModelSettings = false) {
    setFeedback((previous) => ({
      ...previous,
      [key]: { text, ...(showModelSettings && text ? { showModelSettings: true } : {}) },
    }));
  }

  const handleDesiredCompactHeight = useCallback((height: number, drawerOpen = false) => {
    desiredCompactHeightRef.current = height;
    if (!drawerOpen) {
      collapsedCompactHeightRef.current = height;
    }
    if (activeSurfaceRef.current !== 'compact' || !windowController.isDesktopWindow || pinInFlight.current) {
      return;
    }
    void windowController.adjustCompactHeight(height, latestState.current.locale).catch(() => {});
  }, [windowController]);

  function captureModeSwitchFocus(overrideTarget?: CompactExpandTarget | 'addModel') {
    pendingModeFocusRef.current = overrideTarget ? { kind: overrideTarget } : null;
  }

  async function togglePin() {
    if (pinInFlight.current) return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPinned = !currentPrefs.pinned;

    pinInFlight.current = true;
    setPinBusy(true);
    try {
      const appliedPinned = await windowController.setPinned(nextPinned, state.locale);
      pinInFlight.current = false;
      setPinBusy(false);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: computeNextPinPreferences(latestState.current.windowPrefs, appliedPinned),
      });
    } catch (error) {
      pinInFlight.current = false;
      setPinBusy(false);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  async function collapseToCompact() {
    if (pinInFlight.current || latestState.current.windowPrefs.compactMode) return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPrefs = computeNextCompactPreferences(currentPrefs, true);
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);

    pinInFlight.current = true;
    setPinBusy(true);
    try {
      const applied = await windowController.applyWindowMode({
        pinned: nextPrefs.pinned,
        compactMode: true,
        compactHeight: WINDOW_DIMENSIONS.compactDefaultHeight,
        fullSize,
        placement: 'anchorTopLeft',
        locale: state.locale,
      });
      desiredCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      collapsedCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      captureModeSwitchFocus();
      settingsSessionRef.current = null;
      setSettingsSession(null);
      setModelOverlay({ kind: 'closed' });
      pinInFlight.current = false;
      setPinBusy(false);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...nextPrefs,
          pinned: applied.pinned,
          fullSize: normalizeFullWindowSize(applied.previousSize),
          fullPosition: applied.previousPosition,
          compactPosition: applied.position,
        },
      });
    } catch (error) {
      pinInFlight.current = false;
      setPinBusy(false);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  async function expandFromCompact(target?: CompactExpandTarget | 'addModel') {
    if (pinInFlight.current || activeSurfaceRef.current !== 'compact') return;
    const currentPrefs = latestState.current.windowPrefs;
    const nextPrefs = computeNextCompactPreferences(currentPrefs, false);
    const fullSize = normalizeFullWindowSize(currentPrefs.fullSize ?? defaultWindowPreferences().fullSize);

    pinInFlight.current = true;
    setPinBusy(true);
    try {
      const applied = await windowController.applyWindowMode({
        pinned: nextPrefs.pinned,
        compactMode: false,
        fullSize,
        placement: 'anchorTopLeft',
        locale: state.locale,
      });
      desiredCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      collapsedCompactHeightRef.current = WINDOW_DIMENSIONS.compactDefaultHeight;
      captureModeSwitchFocus(target);
      if (target === 'dock' && worktree) {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true });
      } else if (target === 'plan') {
        setTab('commit');
      } else if (target === 'addModel') {
        openConfigureModel();
      }
      pinInFlight.current = false;
      setPinBusy(false);
      dispatch({
        type: 'windowPreferencesUpdated',
        preferences: {
          ...nextPrefs,
          pinned: applied.pinned,
          fullSize: normalizeFullWindowSize(applied.size),
          compactPosition: applied.previousPosition,
          fullPosition: applied.position,
        },
      });
    } catch (error) {
      pinInFlight.current = false;
      setPinBusy(false);
      notify(error instanceof Error ? error.message : t.workbench.setPinFailedFallback);
    }
  }

  async function execute(command: WorkbenchCommand, preserveFeedback = false): Promise<boolean> {
    const commandLocale = command.locale ?? latestState.current.locale;
    const wtMessages = getMessages(commandLocale).workbench;
    const targetKey = 'target' in command ? command.target.worktreeId : viewKey;
    const branching = command.kind === 'switchBranch' || command.kind === 'checkoutRemoteBranch' ? command : undefined;
    const pulling = command.kind === 'pull' ? command : undefined;
    const settingLocalKeep = command.kind === 'setLocalKeep' ? command : undefined;
    const key = branching ? `${branching.target.repositoryId}:write` : pulling ? `${pulling.target.repositoryId}:pull` : settingLocalKeep ? `${settingLocalKeep.target.repositoryId}:localKeep:${settingLocalKeep.requestId}` : command.kind === 'openRepository' ? 'global:openRepository' : command.kind === 'refresh' ? `${command.target.repositoryId}:refresh` : `${targetKey}:${command.kind}`;
    if (inFlight.current.has(key)) return false;
    inFlight.current.add(key); setPending([...inFlight.current]);
    const reading = command.kind === 'openRepository' || command.kind === 'refresh' ? command : undefined;
    const readKey = reading?.kind === 'openRepository' ? 'open' : reading?.target.repositoryId;
    if (reading && readKey) dispatch({ type: 'repositoryReadStarted', key: readKey, requestId: reading.requestId });
    if (branching) dispatch({ type: 'repositoryBranchStarted', repositoryId: branching.target.repositoryId, requestId: branching.requestId });
    try {
      const reply = await backend.execute(command);
      if (command.kind === 'selectModel' || command.kind === 'deleteModel' || command.kind === 'deleteProviderConnection') {
        if (reply && typeof reply === 'object' && 'models' in reply && 'connections' in reply) {
          dispatch({
            type: 'catalogSynced',
            connections: reply.connections,
            models: reply.models,
            activeModelId: reply.activeModelId,
          });
        }
        return true;
      }
      if (command.kind === 'planCPush') {
        if (cancelledRequests.current.delete(command.requestId)) return false;
        if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'cPush') {
          if (reply.preparedPlan) {
            dispatch({
              type: 'event',
              event: {
                type: 'planPrepared',
                target: command.target,
                requestId: command.requestId,
                modelId: command.modelId,
                plan: reply.preparedPlan,
              },
            });
          } else {
            dispatch({ type: 'requestFailed', target: command.target, kind: 'plan', requestId: command.requestId });
            const expandDock = !isEditingCommitInput()
              && !latestState.current.windowPrefs.compactMode
              && reply.session.sessionStatus !== 'idle'
              && !reply.session.autoCollapse;
            dispatch({ type: 'cPushSettled', reply, expandDock });
          }
          notify('', targetKey);
        }
        return true;
      }
      if (command.kind === 'review') {
        if (cancelledRequests.current.delete(command.requestId)) return false;
        if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'review') {
          dispatch({
            type: 'event',
            event: {
              type: 'reviewed',
              target: reply.target,
              requestId: reply.requestId,
              findings: reply.findings,
              skippedPaths: reply.skippedPaths,
              reviewedFileIds: reply.reviewedFileIds,
            },
          });
          notify('', targetKey);
          const targetTree = latestState.current.worktrees.find((item) => item.worktreeId === reply.target.worktreeId);
          if (targetTree && isActiveSession(targetTree)) {
            const currentMessages = getMessages(latestState.current.locale).workbench;
            const high = reply.findings.filter((item) => item.severity === 'high').length;
            const sug = reply.findings.length - high;
            const summary = reply.findings.length === 0
              ? currentMessages.reviewNoRisksSummary
              : currentMessages.reviewFindingsSummary(high, sug);
            dispatch({
              type: 'event',
              event: {
                type: 'message',
                target: reply.target,
                message: {
                  id: 'review-notice',
                  kind: 'review',
                  title: currentMessages.reviewCompleteTitle(summary),
                  description: currentMessages.reviewCompleteDescription,
                },
              },
            });
          }
        }
        return true;
      }
      if (command.kind === 'cPush' || command.kind === 'respond' || command.kind === 'pull') {
        if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'cPush') {
          const expandDock = command.kind === 'pull'
            && !isEditingCommitInput()
            && !latestState.current.windowPrefs.compactMode
            && reply.session.sessionStatus !== 'idle'
            && !reply.session.autoCollapse;
          dispatch({ type: 'cPushSettled', reply, expandDock });
          if (command.kind === 'pull') {
            if (reply.session.sessionStatus === 'idle') {
              const syncedRemote = reply.repositorySnapshot?.worktrees.find((item) => item.worktreeId === command.target.worktreeId)?.remote;
              const currentMessages = getMessages(latestState.current.locale).workbench;
              notify(
                syncedRemote?.kind === 'tracked'
                  ? currentMessages.pullRebaseTrackedDone(syncedRemote.branch)
                  : currentMessages.pullRebaseDone,
                targetKey,
              );
            } else {
              notify('', targetKey);
            }
          } else if (reply.preparedPlan) {
            notify('', targetKey);
          }
        }
        return true;
      }
      if (command.kind === 'openFile') {
        if (!reply || !('kind' in reply) || reply.kind !== 'openFile' || reply.requestId !== command.requestId) {
          throw new Error(wtMessages.openFileMismatchError);
        }
        const shortName = editors.find((item) => item.id === reply.editor)?.short ?? reply.editor;
        const fileName = splitPath(reply.filePath).name;
        notify(getMessages(latestState.current.locale).workbench.openedInEditor(shortName, reply.action, fileName, reply.line), targetKey);
      }
      if (settingLocalKeep) {
        if (!reply || !('kind' in reply) || reply.kind !== 'localKeep' || reply.requestId !== settingLocalKeep.requestId) {
          throw new Error(getMessages(commandLocale).desktop.localKeepReplyMismatch);
        }
        if (latestLocalKeepRequestByRepo.current[settingLocalKeep.target.repositoryId] === settingLocalKeep.requestId) {
          dispatch({ type: 'repositoryLocalKeepFinished', repositoryId: settingLocalKeep.target.repositoryId, reply });
        }
        return true;
      }
      if (branching) {
        if (!reply || !('kind' in reply) || reply.kind !== 'branch' || reply.requestId !== branching.requestId) {
          throw new Error(wtMessages.branchMismatchError);
        }
        dispatch({ type: 'repositoryBranchFinished', repositoryId: branching.target.repositoryId, reply });
        const landedBranch = reply.snapshot.worktrees.find((tree) => tree.worktreeId === reply.snapshot.activeWorktreeId)?.branch ?? branching.branch;
        const currentMessages = getMessages(latestState.current.locale).workbench;
        notify(
          branching.kind === 'checkoutRemoteBranch' && landedBranch !== branching.branch
            ? currentMessages.remoteBranchCheckedOut(landedBranch, branching.branch)
            : currentMessages.branchSwitched(landedBranch),
          reply.snapshot.activeWorktreeId,
        );
      }
      if (reading && readKey) {
        if (!reply || ('kind' in reply && reply.kind !== undefined) || !('requestId' in reply) || reply.requestId !== reading.requestId) {
          throw new Error(wtMessages.readMismatchError);
        }
        const previousTree = latestState.current.worktrees.find((item) => item.worktreeId === targetKey);
        const previousReadError = previousTree?.readError;
        dispatch({ type: 'repositoryReadFinished', key: readKey, reply });
        if (reading.kind === 'openRepository') {
          if (reply.snapshot && !preserveFeedback) notify('', targetKey);
        } else {
          let recoveredFromReadError = false;
          if (previousReadError) {
            const refreshedTree = reply.snapshot?.worktrees.find((item) => item.worktreeId === targetKey);
            recoveredFromReadError = !!refreshedTree && !refreshedTree.readError;
          }
          if (!preserveFeedback || recoveredFromReadError) notify('', targetKey);
          const sessionStatus = previousTree?.sessionStatus;
          if (sessionStatus && ['waitingManual', 'conflict', 'pushFailed'].includes(sessionStatus)) {
            void backend.reconcileSession?.(reading.target, latestState.current.locale).catch(() => {});
          }
        }
      }
      return true;
    } catch (error) {
      if ('requestId' in command && cancelledRequests.current.delete(command.requestId)) {
        return false;
      }
      const message = error instanceof Error
        ? error.message
        : typeof error === 'string'
          ? error
          : getMessages(latestState.current.locale).workbench.genericOperationFailed;
      if (command.kind === 'planCPush') {
        dispatch({ type: 'requestFailed', target: command.target, kind: 'plan', requestId: command.requestId });
      }
      if (command.kind === 'review') {
        dispatch({ type: 'requestFailed', target: command.target, kind: 'review', requestId: command.requestId, error: message });
      }
      if (command.kind === 'cPush') {
        dispatch({ type: 'cPushStartFailed', worktreeId: command.target.worktreeId, runId: command.requestId });
      }
      if (pulling && backend.repositoryReadsAvailable) {
        void execute({ kind: 'refresh', target: pulling.target, requestId: crypto.randomUUID(), locale: latestState.current.locale }, true);
      }
      if (branching) {
        dispatch({ type: 'repositoryBranchFailed', repositoryId: branching.target.repositoryId, requestId: branching.requestId });
        if (backend.repositoryReadsAvailable) {
          void execute({ kind: 'refresh', target: branching.target, requestId: crypto.randomUUID(), locale: latestState.current.locale }, true);
        }
      }
      if (settingLocalKeep && backend.repositoryReadsAvailable) {
        void execute({ kind: 'refresh', target: settingLocalKeep.target, requestId: crypto.randomUUID(), locale: latestState.current.locale }, true);
      }
      if (reading && readKey) {
        dispatch({
          type: 'repositoryReadFailed',
          key: readKey,
          requestId: reading.requestId,
          ...(reading.kind === 'refresh' ? { worktreeId: reading.target.worktreeId, error: message } : {}),
        });
      }
      const modelRelatedCommand = ['planCPush', 'review', 'cPush', 'respond', 'selectModel', 'deleteModel', 'deleteProviderConnection'].includes(command.kind);
      notify(message, targetKey, modelRelatedCommand && isModelErrorText(message));
      return false;
    } finally { inFlight.current.delete(key); setPending([...inFlight.current]); }
  }

  function setTab(next: 'commit' | 'review') {
    if (worktree) dispatch({ type: 'tab', worktreeId: worktree.worktreeId, tab: next });
    else setEmptyTab(next);
  }
  async function runPlan() {
    if (!worktree || !model || !canPlan || worktree.requests.plan) return;
    if (worktree.detached) { notify(t.workbench.detachedPrimaryBlocked); return; }
    const target = targetOf(worktree);
    const requestId = crypto.randomUUID();
    const fileIds = selected.map((file) => file.id);
    const selections = selected.every((file) => !!file.commitToken)
      ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    dispatch({ type: 'request', target, kind: 'plan', requestId, fileIds });
    await execute({
      kind: 'planCPush',
      target,
      requestId,
      fileIds,
      selections,
      modelId: model.id,
      prompt: promptText,
      locale: state.locale,
      commitLanguage: activeCommitLanguage,
    });
  }
  async function runReview(changedOnly = false) {
    if (!worktree || !model || !canUseModel || worktree.requests.review) return;
    const target = targetOf(worktree), requestId = crypto.randomUUID();
    const targetFiles = selected.filter((file) => !changedOnly || worktree.review.changedPaths.includes(file.path));
    const fileIds = targetFiles.map((file) => file.id);
    if (!fileIds.length) { notify(t.workbench.noChangedFilesToReReview); return; }
    const selections = targetFiles.every((file) => !!file.commitToken)
      ? targetFiles.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    dispatch({ type: 'request', target, kind: 'review', requestId, fileIds });
    await execute({
      kind: 'review',
      target,
      requestId,
      fileIds,
      selections,
      modelId: model.id,
      prompt: promptText,
      locale: state.locale,
      ...(changedOnly ? { changedPaths: worktree.review.changedPaths } : {}),
    });
  }
  function cancelModel(kind: 'plan' | 'review') {
    const active = worktree?.requests[kind];
    if (!worktree || !active) return;
    cancelledRequests.current.add(active.id);
    dispatch({ type: 'requestFailed', target: targetOf(worktree), kind, requestId: active.id });
    void backend.cancelRequest?.(active.id).catch(() => {});
    notify(kind === 'plan' ? t.workbench.cancelledPlan : t.workbench.cancelledReview);
  }
  function cancelActiveModelRequests(tree: typeof worktree) {
    if (!tree) return;
    for (const kind of ['plan', 'review'] as const) {
      const active = tree.requests[kind];
      if (active) {
        cancelledRequests.current.add(active.id);
        void backend.cancelRequest?.(active.id).catch(() => {});
      }
    }
  }
  function switchWorktree(worktreeId: string, expand?: boolean) {
    dispatch({ type: 'switch', worktreeId, expand });
    const next = state.worktrees.find((tree) => tree.worktreeId === worktreeId);
    if (backend.repositoryReadsAvailable && next) {
      void execute({ kind: 'refresh', target: targetOf(next), requestId: crypto.randomUUID(), locale: state.locale });
    }
  }
  function setLocalKeep(paths: string[], keep: boolean) {
    if (!worktree || !paths.length) return;
    cancelActiveModelRequests(worktree);
    const target = targetOf(worktree);
    dispatch({ type: 'localKeepUpdated', repositoryId: worktree.repositoryId, paths, keep });
    if (backend.repositoryReadsAvailable) {
      const requestId = crypto.randomUUID();
      latestLocalKeepRequestByRepo.current[worktree.repositoryId] = requestId;
      void execute(
        {
          kind: 'setLocalKeep',
          target,
          requestId,
          paths,
          keep,
          locale: state.locale,
        },
        true,
      );
    }
  }
  function openFile(fileId: string, line?: number) {
    if (worktree) {
      void execute({
        kind: 'openFile',
        target: targetOf(worktree),
        requestId: crypto.randomUUID(),
        fileId,
        editor: activeEditor,
        line,
        locale: state.locale,
      });
    }
  }
  function openConfigureModel() {
    if (latestState.current.connections.length === 0) {
      setModelOverlay({ kind: 'createProvider' });
    } else {
      openSettingsFromWorkbench(true);
    }
  }
  function openConfigureModelFromCompact() {
    if (latestState.current.connections.length === 0) {
      void expandFromCompact('addModel');
    } else {
      void openSettingsFromCompact({ focusModels: true });
    }
  }
  function primary() {
    if (!model && (!worktree || !isActiveSession(worktree))) {
      if (compactMode) {
        openConfigureModelFromCompact();
      } else {
        openConfigureModel();
      }
      return;
    }
    if (!worktree) return;
    if (isActiveSession(worktree)) {
      if (compactMode) {
        void expandFromCompact('dock');
      } else {
        dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true });
      }
      return;
    }
    if (inFlight.current.has(`${worktree.worktreeId}:cPush`)) return;
    if (worktree.detached) { notify(t.workbench.detachedPrimaryBlocked); return; }
    if (model && selected.length && !worktree.readError) {
      const validPlan = activeValidPlan(worktree);
      if (validPlan && validPlan.groups.some((group) => !group.message.trim())) {
        notify(t.commit.emptyPlanMessageError);
        return;
      }
      if (worktree.plan && !validPlan) {
        // Outdated plan is discarded before starting a fresh C-Push plan:
        dispatch({ type: 'planCleared', worktreeId: worktree.worktreeId });
      }
      const activePlanReq = worktree.requests.plan;
      if (activePlanReq) {
        cancelledRequests.current.add(activePlanReq.id);
        void backend.cancelRequest?.(activePlanReq.id).catch(() => {});
      }
      const requestId = crypto.randomUUID();
      const target = targetOf(worktree);
      const fileIds = selected.map((file) => file.id);
      const selections = selected.every((file) => !!file.commitToken)
        ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
        : undefined;
      const expandDock = !isEditingCommitInput() && !compactMode;
      dispatch({ type: 'cPushStarted', worktreeId: worktree.worktreeId, runId: requestId, frozenTarget: target, frozenFileIds: fileIds, modelId: model.id, expandDock });
      void execute({
        kind: 'cPush',
        target,
        requestId,
        fileIds,
        selections,
        modelId: model.id,
        prompt: promptText,
        ...(validPlan
          ? {
              confirmedPlan: {
                groups: validPlan.groups.map((group) => ({ ...group, fileIds: [...group.fileIds] })),
                skippedFileIds: [...validPlan.skippedFileIds],
              },
            }
          : {}),
        locale: state.locale,
        commitLanguage: activeCommitLanguage,
      });
    }
  }
  async function respond(message: Message, answer: { optionId?: string; text?: string }) {
    if (!worktree) return;
    const id = worktree.worktreeId;
    const target = worktree.cPush?.frozenTarget ?? targetOf(worktree);
    const runId = worktree.cPush?.runId;
    if (await execute({
      kind: 'respond',
      target,
      requestId: crypto.randomUUID(),
      ...(runId ? { runId } : {}),
      ...(model ? { modelId: model.id } : {}),
      messageId: message.id,
      locale: state.locale,
      ...answer,
    })) {
      dispatch({ type: 'reply', worktreeId: id, messageId: message.id, text: '' });
    }
  }
  async function selectModel(id: string) {
    if (await execute({ kind: 'selectModel', modelId: id, locale: state.locale })) dispatch({ type: 'modelSelected', modelId: id });
  }
  async function deleteModel(id: string) {
    if (await execute({ kind: 'deleteModel', modelId: id, locale: state.locale })) dispatch({ type: 'modelDeleted', modelId: id });
  }
  async function deleteProviderConnection(connectionId: string) {
    if (await execute({ kind: 'deleteProviderConnection', connectionId, locale: state.locale })) {
      dispatch({ type: 'providerDeleted', connectionId });
    }
  }
  async function handleCheckUpdate() {
    if (!backend.checkForUpdate) return;
    setUpdateState((previous) => ({ ...previous, phase: 'checking', error: undefined }));
    try {
      const next = await backend.checkForUpdate(true, latestState.current.locale);
      setUpdateState(next);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateState((previous) => ({ ...previous, phase: 'error', error: message, canInstall: false }));
    }
  }
  async function handleInstallUpdate(version: string) {
    if (!backend.installUpdate) return;
    try {
      const next = await backend.installUpdate(version, latestState.current.locale);
      setUpdateState(next);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateState((previous) => ({ ...previous, phase: 'error', error: message, canInstall: false }));
    }
  }
  async function handleCancelUpdate() {
    if (!backend.cancelUpdate) return;
    try {
      const next = await backend.cancelUpdate(latestState.current.locale);
      setUpdateState(next);
    } catch {
      // Keep current snapshot if cancel failed.
    }
  }
  function handleOpenReleasePage() {
    void backend.openReleasePage?.(latestState.current.locale).catch((error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateState((previous) => ({ ...previous, phase: 'error', error: message }));
    });
  }
  const highCount = worktree?.review.findings.filter((finding) => finding.severity === 'high').length ?? 0;
  const suggestionCount = (worktree?.review.findings.length ?? 0) - highCount;
  const noFilesReviewed = worktree?.review.reviewedFileIds !== undefined && worktree.review.reviewedFileIds.length === 0 && (worktree.review.skippedPaths.length ?? 0) > 0;
  const reviewLabel = worktree?.review.status === 'running'
    ? t.review.tabStatusRunning
    : worktree?.review.status === 'done'
      ? worktree.review.changedPaths.length || selected.some((file) => !worktree.review.scopeFileIds?.includes(file.id))
        ? t.review.tabStatusChanged
        : highCount || suggestionCount
          ? t.review.tabStatusCounts(highCount, suggestionCount)
          : noFilesReviewed
            ? t.review.tabStatusUncovered
            : t.review.tabStatusPassed
      : '';
  const activeNotice = feedback[viewKey];

  const syncTabIndicator = useCallback(() => {
    const activeButton = tab === 'commit' ? commitTabRef.current : reviewTabRef.current;
    if (!activeButton || activeButton.offsetWidth <= 0) {
      setTabIndicator(null);
      return;
    }
    const nextLeft = activeButton.offsetLeft;
    const nextWidth = activeButton.offsetWidth;
    setTabIndicator((previous) =>
      previous && previous.left === nextLeft && previous.width === nextWidth
        ? previous
        : { left: nextLeft, width: nextWidth },
    );
  }, [tab]);

  useLayoutEffect(() => {
    if (!presentation.mountWorkbenchContent) return;
    syncTabIndicator();
  }, [
    presentation.mountWorkbenchContent,
    syncTabIndicator,
    state.locale,
    reviewLabel,
    worktree?.requests.plan,
    worktree?.plan?.groups.length,
    planOutdated,
  ]);

  useEffect(() => {
    if (!presentation.mountWorkbenchContent || typeof ResizeObserver === 'undefined') return;
    const listEl = tabListRef.current;
    if (!listEl) return;
    const observer = new ResizeObserver(() => syncTabIndicator());
    observer.observe(listEl);
    if (commitTabRef.current) observer.observe(commitTabRef.current);
    if (reviewTabRef.current) observer.observe(reviewTabRef.current);
    return () => observer.disconnect();
  }, [presentation.mountWorkbenchContent, syncTabIndicator]);

  if (!hydrated && windowController.isDesktopWindow && !initialState) {
    return <div className="workbench-hydrating" aria-busy="true" />;
  }

  return <I18nProvider locale={state.locale}>
    <MotionRoot>
    <main
      className={`workbench ${presentation.windowSizeMode === 'compact' ? 'workbench-compact' : ''}`}
      data-surface={presentation.surface}
      data-window-size={presentation.windowSizeMode}
      data-window-transition={pinBusy ? 'resizing' : 'idle'}
      aria-label={t.workbench.ariaLabel}
    >
      {presentation.surface === 'compact' && (
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
          opening={opening}
          branchBusy={branchBusy}
          pullBusy={pullBusy}
          busy={busy}
          pinned={pinned}
          pinBusy={pinBusy}
          hasUpdate={hasUpdate}
          repositoryReadsAvailable={!!backend.repositoryReadsAvailable}
          notice={activeNotice?.text ? activeNotice : undefined}
          onSwitchWorktree={(worktreeId) => switchWorktree(worktreeId)}
          onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID(), locale: state.locale })}
          onBranch={async (kind, branch) => worktree ? execute({ kind, branch, branchToken: worktree.branchToken ?? '', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale }) : false}
          onPull={() => { if (worktree) void execute({ kind: 'pull', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale, ...(model ? { modelId: model.id } : {}) }); }}
          onSelect={(fileId) => { if (worktree) { cancelActiveModelRequests(worktree); dispatch({ type: 'select', worktreeId: worktree.worktreeId, fileId }); } }}
          onSelectAll={() => { if (worktree) { cancelActiveModelRequests(worktree); dispatch({ type: 'selectAll', worktreeId: worktree.worktreeId }); } }}
          onExpand={() => void expandFromCompact()}
          onExpandTarget={(target) => void expandFromCompact(target)}
          onPin={() => void togglePin()}
          onSettings={(drawerWasOpen) => void openSettingsFromCompact({ focusModels: false, drawerWasOpen })}
          onPrimary={primary}
          onConfigureModel={openConfigureModelFromCompact}
          onOpenModelSettings={() => void openSettingsFromCompact({ focusModels: true })}
          onDismissNotice={() => notify('')}
          onDesiredHeightChange={handleDesiredCompactHeight}
        />
        </m.div>
      )}
      {presentation.mountWorkbenchContent && (
        <m.div
          className="workbench-content"
          inert={settingsOpen || modelOverlayOpen}
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
            pinned={pinned}
            hasUpdate={hasUpdate}
            pinBusy={pinBusy}
            onCollapse={() => void collapseToCompact()}
            onPin={() => void togglePin()}
            onSettings={() => openSettingsFromWorkbench(false)}
            onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID(), locale: state.locale })}
            onSwitch={switchWorktree}
            onBranch={async (kind, branch) => worktree ? execute({ kind, branch, branchToken: worktree.branchToken ?? '', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale }) : false}
            onPull={() => { if (worktree) void execute({ kind: 'pull', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale, ...(model ? { modelId: model.id } : {}) }); }}
          />
          <PresenceNotice show={Boolean(activeNotice?.text)} motionKey={`notice:${viewKey}:${activeNotice?.text ?? ''}`}>
            {activeNotice?.text && (
              <div className="operation-notice" role="alert">
                <span className="operation-notice-body">
                  <Icon name="alert" className="operation-notice-icon" />
                  <span>{activeNotice.text}</span>
                </span>
                <div className="operation-notice-actions">
                  {activeNotice.showModelSettings && (
                    <button type="button" className="secondary operation-notice-action" onClick={() => openSettingsFromWorkbench(true)}>
                      {t.workbench.openModelSettingsAction}
                    </button>
                  )}
                  <IconButton icon="close" label={t.common.dismissNotice} onClick={() => notify('')} />
                </div>
              </div>
            )}
          </PresenceNotice>
          <ChangesPanel
            worktree={worktree}
            editorShort={editorShort}
            opening={opening}
            repositoryReadsAvailable={!!backend.repositoryReadsAvailable}
            onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID(), locale: state.locale })}
            onSelect={(fileId) => { if (worktree) { cancelActiveModelRequests(worktree); dispatch({ type: 'select', worktreeId: worktree.worktreeId, fileId }); } }}
            onSelectAll={() => { if (worktree) { cancelActiveModelRequests(worktree); dispatch({ type: 'selectAll', worktreeId: worktree.worktreeId }); } }}
            onSelectFiles={(fileIds) => { if (worktree) { cancelActiveModelRequests(worktree); dispatch({ type: 'selectFiles', worktreeId: worktree.worktreeId, fileIds }); } }}
            onSetLocalKeep={setLocalKeep}
            onOpenFile={openFile}
            onReview={() => setTab('review')}
            refreshing={refreshing}
            onRefresh={() => { if (worktree) void execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID(), locale: state.locale }); }}
          />
          <section className="lower-workspace" aria-label={t.workbench.lowerWorkspaceLabel}>
            <div className="tab-workspace" inert={!!worktree?.dockExpanded}>
              <div className="tab-heading"><div ref={tabListRef} className={`tab-list ${tabIndicator ? '' : 'no-indicator'}`.trim()} role="tablist" aria-label={t.workbench.workspaceTabsLabel}>
                {tabIndicator && (
                  <span
                    className={`tab-active-indicator ${instantMotion ? 'is-instant' : ''}`.trim()}
                    aria-hidden="true"
                    style={{
                      transform: `translateX(${tabIndicator.left}px)`,
                      width: `${tabIndicator.width}px`,
                    }}
                  />
                )}
                <button ref={commitTabRef} type="button" id="commit-tab" role="tab" aria-selected={tab === 'commit'} aria-controls="commit-panel" tabIndex={tab === 'commit' ? 0 : -1} onKeyDown={(event) => { if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { setTab('review'); document.getElementById('review-tab')?.focus(); } }} onClick={() => setTab('commit')}>
                  {t.commit.tabLabel}
                  {worktree?.requests.plan
                    ? <span className="small muted">{t.commit.planningBadge}</span>
                    : worktree?.plan
                      ? <span className={`small ${planOutdated ? 'warning' : 'muted'}`}>{planOutdated ? t.commit.planOutdatedBadge : t.commit.planCountBadge(worktree.plan.groups.length)}</span>
                      : null}
                </button>
                <button ref={reviewTabRef} type="button" id="review-tab" role="tab" aria-selected={tab === 'review'} aria-controls="review-panel" tabIndex={tab === 'review' ? 0 : -1} onKeyDown={(event) => { if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { setTab('commit'); document.getElementById('commit-tab')?.focus(); } }} onClick={() => setTab('review')}>{t.review.tabLabel}{reviewLabel && <span className={`small ${highCount ? 'danger' : 'muted'}`}>{reviewLabel}</span>}</button>
              </div><div className="row">
                {tab === 'review' && worktree?.review.status === 'done' && <IconButton icon="refresh" label={t.review.reReviewLabel} disabled={!canUseModel} onClick={() => void runReview()} />}
                <button
                  type="button"
                  className={`prompt-toggle ${showPromptBox ? 'is-open' : ''} ${promptText ? 'has-text' : ''}`}
                  aria-expanded={showPromptBox}
                  onClick={() => {
                    if (showPromptBox && !promptText) {
                      setPromptOpen(false);
                    } else {
                      promptFocusRequestedRef.current = true;
                      setPromptOpen(true);
                      textareaRef.current?.focus();
                    }
                  }}
                >
                  {promptText ? (
                    <>
                      <span className="prompt-dot" aria-hidden="true" />
                      {t.commit.promptActiveToggle}
                    </>
                  ) : (
                    t.commit.promptToggle
                  )}
                </button>
              </div></div>
              <Collapse key={`prompt:${viewKey}`} open={showPromptBox}>
                <div className="prompt-box">
                  <textarea
                    ref={textareaRef}
                    className="prompt-textarea"
                    aria-label={t.commit.promptLabel}
                    rows={2}
                    value={promptText}
                    onChange={(event) => dispatch({ type: 'prompt', worktreeId: worktree?.worktreeId ?? null, text: event.target.value })}
                    placeholder={t.commit.promptPlaceholder}
                    spellCheck={false}
                  />
                  <IconButton
                    icon="close"
                    label={promptText ? t.commit.clearPromptLabel : t.commit.hidePromptLabel}
                    onClick={() => {
                      if (promptText) {
                        dispatch({ type: 'clearPrompt', worktreeId: worktree?.worktreeId ?? null });
                      }
                      setPromptOpen(false);
                    }}
                  />
                </div>
              </Collapse>
              <div role="tabpanel" id={tab === 'commit' ? 'commit-panel' : 'review-panel'} aria-labelledby={tab === 'commit' ? 'commit-tab' : 'review-tab'} className="tab-panel">
                {tab === 'commit' ? (
                  <CommitPanel
                    files={worktree?.files ?? []}
                    plan={worktree?.plan}
                    planOutdated={planOutdated}
                    hasWorktree={!!worktree}
                    hasModel={!!model}
                    canPlan={canPlan}
                    planning={!!worktree?.requests.plan}
                    panelRef={commitPanelRef}
                    onPlan={() => void runPlan()}
                    onCancelPlan={() => cancelModel('plan')}
                    onEditPlanGroup={(groupId, message) => { if (worktree) dispatch({ type: 'planGroupEdited', worktreeId: worktree.worktreeId, groupId, message }); }}
                    onClearPlan={() => { if (worktree) dispatch({ type: 'planCleared', worktreeId: worktree.worktreeId }); }}
                    onConfigureModel={openConfigureModel}
                  />
                ) : (
                  <ReviewPanel
                    worktree={worktree}
                    hasModel={!!model}
                    canReview={canUseModel}
                    onReview={(changedOnly) => void runReview(changedOnly)}
                    onCancel={() => cancelModel('review')}
                    onAddModel={openConfigureModel}
                    onOpenModelSettings={() => openSettingsFromWorkbench(true)}
                    onOpenFile={openFile}
                  />
                )}
              </div>
            </div>
            {worktree && <MessageDock worktree={worktree} busy={busy} editorShort={editorShort} onExpanded={(expanded) => dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded })} onReply={(messageId, text) => dispatch({ type: 'reply', worktreeId: worktree.worktreeId, messageId, text })} onRespond={(message, answer) => void respond(message, answer)} onOpenFile={openFile} onReview={() => setTab('review')} />}
          </section>
          <ActionFooter
            key={`footer:${viewKey}`}
            worktree={worktree}
            connections={state.connections}
            models={state.models}
            activeModelId={state.activeModelId}
            busy={busy}
            onConfigureModel={openConfigureModel}
            onManageModels={() => openSettingsFromWorkbench(true)}
            onSelectModel={(id) => void selectModel(id)}
            onPrimary={primary}
          />
        </m.div>
      )}
      <MotionPresence>
      {presentation.showSettings && (
        <SettingsPanel
          key={presentation.surface === 'standaloneSettings' ? 'settings-standalone' : 'settings-sheet'}
          overlayVariant={presentation.surface === 'standaloneSettings' ? 'standalone' : 'sheet'}
          locale={state.locale}
          repositoryName={repository?.name}
          editor={activeEditor}
          commitLanguage={activeCommitLanguage}
          installedEditors={state.installedEditors}
          connections={state.connections}
          models={state.models}
          activeModelId={state.activeModelId}
          onLocale={(locale) => dispatch({ type: 'locale', locale })}
          onEditor={(nextEditor) => dispatch({ type: 'editor', editor: nextEditor, repositoryId: worktree?.repositoryId ?? null })}
          onCommitLanguage={(commitLanguage) => dispatch({ type: 'commitLanguage', commitLanguage, repositoryId: worktree?.repositoryId ?? null })}
          onSelectModel={(id) => void selectModel(id)}
          onAddProvider={() => setModelOverlay({ kind: 'createProvider', keepActiveModel: state.models.length > 0 })}
          onEditProvider={(connection) => setModelOverlay({ kind: 'editProvider', connection })}
          onDeleteProvider={(connectionId) => void deleteProviderConnection(connectionId)}
          onAddModel={(connection) => setModelOverlay({ kind: 'addModel', connection })}
          onEditModel={(connection, target) => setModelOverlay({ kind: 'editModel', connection, model: target })}
          onDeleteModel={(id) => void deleteModel(id)}
          updateState={updateState}
          gitBusyForUpdate={gitBusyForUpdate}
          onCheckUpdate={() => void handleCheckUpdate()}
          onInstallUpdate={(version) => void handleInstallUpdate(version)}
          onCancelUpdate={() => void handleCancelUpdate()}
          onOpenReleasePage={handleOpenReleasePage}
          covered={modelOverlayOpen}
          closeDisabled={pinBusy && settingsSession?.returnTo === 'compact'}
          error={activeNotice?.text}
          focusModels={settingsSession?.focusModels ?? false}
          onClose={() => void closeSettings()}
        />
      )}
      </MotionPresence>
      <MotionPresence>
      {presentation.surface !== 'compact' && modelOverlay.kind === 'createProvider' && (
        <ProviderConnectionForm
          key="create-provider"
          overlayVariant={settingsOpen ? 'drill' : 'sheet'}
          mode="create"
          keepActiveModel={modelOverlay.keepActiveModel}
          onClose={() => setModelOverlay({ kind: 'closed' })}
          test={(input) => backend.testModel({ ...input, locale: state.locale })}
          create={(input) => backend.createProviderConnection({ ...input, locale: state.locale })}
          onCreated={(reply) => {
            const closeSettingsAfter = !modelOverlay.keepActiveModel;
            dispatch({
              type: 'catalogSynced',
              connections: reply.connections,
              models: reply.models,
              activeModelId: reply.activeModelId,
            });
            setModelOverlay({ kind: 'closed' });
            if (closeSettingsAfter) {
              void closeSettings();
            }
          }}
        />
      )}
      {presentation.surface !== 'compact' && modelOverlay.kind === 'editProvider' && (
        <ProviderConnectionForm
          key={`edit-provider:${modelOverlay.connection.id}`}
          overlayVariant={settingsOpen ? 'drill' : 'sheet'}
          mode="edit"
          connection={
            state.connections.find((c) => c.id === modelOverlay.connection.id)
            ?? modelOverlay.connection
          }
          childModels={state.models.filter((m) => m.connectionId === modelOverlay.connection.id)}
          onClose={() => setModelOverlay({ kind: 'closed' })}
          test={(input) => backend.testModel({ ...input, locale: state.locale })}
          update={(input) => backend.updateProviderConnection({ ...input, locale: state.locale })}
          revealApiKey={
            backend.revealProviderApiKey
              ? (connectionId) => backend.revealProviderApiKey!(connectionId, state.locale)
              : undefined
          }
          onKeyHintResolved={(connectionId, keyHint) =>
            dispatch({ type: 'providerKeyHintSynced', connectionId, keyHint })}
          onUpdated={(reply) => {
            dispatch({
              type: 'catalogSynced',
              connections: reply.connections,
              models: reply.models,
              activeModelId: reply.activeModelId,
            });
            setModelOverlay({ kind: 'closed' });
          }}
        />
      )}
      {presentation.surface !== 'compact' && (modelOverlay.kind === 'addModel' || modelOverlay.kind === 'editModel') && (
        <ModelEntryForm
          key={
            modelOverlay.kind === 'editModel'
              ? `edit-model:${modelOverlay.model.id}`
              : `add-model:${modelOverlay.connection.id}`
          }
          overlayVariant={settingsOpen ? 'drill' : 'sheet'}
          connection={
            state.connections.find((c) => c.id === modelOverlay.connection.id)
            ?? modelOverlay.connection
          }
          initialModel={modelOverlay.kind === 'editModel' ? modelOverlay.model : undefined}
          onClose={() => setModelOverlay({ kind: 'closed' })}
          test={(input) => backend.testModel({ ...input, locale: state.locale })}
          save={(input) => backend.saveModel({ ...input, locale: state.locale })}
          onSaved={(reply) => {
            dispatch({
              type: 'catalogSynced',
              connections: reply.connections,
              models: reply.models,
              activeModelId: reply.activeModelId,
            });
            setModelOverlay({ kind: 'closed' });
          }}
        />
      )}
      </MotionPresence>
    </main>
    </MotionRoot>
  </I18nProvider>;
}
