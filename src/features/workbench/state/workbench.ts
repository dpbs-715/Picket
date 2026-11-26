import type { ModelConfiguration, ProviderConnection } from '../../settings/modelProviders';
import type { AskUserQuestionRequest } from '../../../../shared/ask-user-protocol';
import type {
  CPushCommandReply,
  CPushLandedCommit,
  CPushPlanGroup,
  CPushPreparedPlan,
  CPushProgressEvent,
  CPushQaItem,
  CPushTimelineItem,
} from '../../../../shared/cpush-protocol';
import {
  CUSTOM_ACTION_LIMITS,
  computeCustomActionPromptHash,
  sanitizeCustomActionConfig,
  type CustomActionCommandReply,
  type CustomActionConfig,
  type CustomActionLink,
  type CustomActionProgressEvent,
  type CustomActionSyncResult,
  type CustomActionTimelineItem,
} from '../../../../shared/custom-action-protocol';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitLocalKeepReply, GitReadReply, GitReadTarget, GitRemoteBranch } from '../../../../shared/git-read-protocol';
import { DEFAULT_APP_LOCALE, DEFAULT_COMMIT_LANGUAGE, type AppLocale, type CommitLanguage } from '../../../../shared/language-protocol';
import {
  WINDOW_DIMENSIONS,
  type PersistedWindowPreferences,
  type WindowPosition,
  type WindowSize,
  type WorkbenchRestoreReply,
} from '../../../../shared/workbench-persistence-protocol';
import { getMessages } from '../../../i18n/messages';
import {
  applyCustomActionProgress,
  applyLocalKeepUpdate,
  finishCPushCommand,
  finishCustomActionCommand,
  finishRepositoryBranch,
  finishRepositoryLocalKeep,
  finishRepositoryRead,
  hydrateWorkbench,
  readScopeVersion,
  receiveCPushProgress,
} from './repositories';

export type Target = GitReadTarget;

export interface Change {
  id: string;
  path: string;
  previousPath?: string;
  status: 'A' | 'M' | 'D' | 'R' | 'U' | 'T' | 'C';
  gitStatus?: string;
  range: { kind: 'partial' | 'staged' | 'unstaged'; added: number; removed: number };
  ignoredForReview?: boolean;
  localKeep?: boolean;
  binary?: boolean;
  contentVersion?: string;
  commitToken?: string;
}

export interface Finding {
  id: string;
  fileId: string;
  path: string;
  line: number;
  severity: 'high' | 'suggestion';
  title: string;
  reason: string;
}

export interface Message {
  id: string;
  kind: 'progress' | 'question' | 'conflict' | 'waitingManual' | 'resolved' | 'pushFailed' | 'done' | 'review' | 'error';
  title: string;
  description: string;
  scope?: string;
  files?: { id: string; label: string; badge?: string }[];
  commits?: { hash: string; title: string }[];
  links?: { label: string; url: string }[];
  options?: { id: string; label: string; description?: string; recommended?: boolean; preview?: string }[];
  questionnaire?: AskUserQuestionRequest;
  progress?: { completed: number; total: number; label: string };
}

export interface EditableCommitPlan {
  id: string;
  baseScopeVersion: string;
  modelId: string;
  summary: string;
  groups: CPushPlanGroup[];
  skippedFileIds: string[];
}

export interface ActiveCPushState {
  runId: string;
  frozenTarget: Target;
  frozenFileIds: string[];
  promptRevisionAtStart: number;
  modelId: string;
  timeline: CPushTimelineItem[];
  qaLog: CPushQaItem[];
  commits: CPushLandedCommit[];
  canRetryRemaining: boolean;
  planOnly?: boolean;
}

export interface ActiveCustomActionState {
  runId: string;
  actionId: string;
  actionLabel: string;
  pendingCallId?: string;
  timeline: CustomActionTimelineItem[];
  commits: CPushLandedCommit[];
  links: CustomActionLink[];
}

export interface Worktree extends Target {
  label: string;
  path: string;
  branch: string;
  head?: string | null;
  detached?: boolean;
  statusVersion?: string;
  branchToken?: string | null;
  readError?: string | null;
  remote: { kind: 'tracked'; branch: string; ahead: number; behind: number; localOnly?: boolean } | { kind: 'missing' | 'error'; reason: string };
  files: Change[];
  selectedIds: string[];
  unselectedIds?: string[];
  prompt: { text: string; revision: number };
  plan?: EditableCommitPlan;
  planRevision: number;
  review: {
    status: 'idle' | 'running' | 'done' | 'failed';
    findings: Finding[];
    skippedPaths: string[];
    changedPaths: string[];
    scopeFileIds?: string[];
    reviewedFileIds?: string[];
    error?: string;
  };
  sessionStatus: 'idle' | 'running' | 'needsDecision' | 'conflict' | 'waitingManual' | 'pushFailed' | 'done';
  messages: Message[];
  cPush?: ActiveCPushState;
  customAction?: ActiveCustomActionState;
  tab: 'commit' | 'review';
  dockExpanded: boolean;
  replies: Record<string, string>;
  requests: Partial<{
    plan: { id: string; promptRevision: number; planRevision: number; fileIds: string[] };
    review: { id: string; promptRevision: number; fileIds: string[]; previousReviewStatus?: Worktree['review']['status'] };
  }>;
}

export interface Repository {
  id: string;
  name: string;
  branches: { name: string; worktreeId?: string }[];
  remoteBranches?: GitRemoteBranch[];
  localKeepPaths?: string[];
  readSequence?: number;
}

export interface RepositorySettings {
  editor: EditorId;
  commitLanguage: CommitLanguage;
  localKeepPaths?: string[];
  customActions?: CustomActionConfig[];
  activeActionId?: string | null;
}

export interface WorkbenchState {
  repositories: Repository[];
  worktrees: Worktree[];
  activeWorktreeId: string | null;
  emptyPrompt: string;
  connections: ProviderConnection[];
  models: ModelConfiguration[];
  activeModelId: string | null;
  locale: AppLocale;
  editor: EditorId;
  defaultCommitLanguage: CommitLanguage;
  repositorySettings: Record<string, RepositorySettings>;
  windowPrefs: PersistedWindowPreferences;
  installedEditors?: EditorId[];
  settingsModified?: boolean;
  navigationRevision: number;
  repositoryBranches: Record<string, { requestId: string; navigationRevision: number }>;
  repositoryReads: Record<string, { requestId: string; navigationRevision: number }>;
}

export const editors = [
  { id: 'idea', name: 'IntelliJ IDEA', cli: 'idea', short: 'IDEA' },
  { id: 'rustrover', name: 'RustRover', cli: 'rustrover', short: 'RustRover' },
  { id: 'webstorm', name: 'WebStorm', cli: 'webstorm', short: 'WebStorm' },
  { id: 'vscode', name: 'VS Code', cli: 'code', short: 'VS Code' },
  { id: 'cursor', name: 'Cursor', cli: 'cursor', short: 'Cursor' },
] as const;
export type EditorId = typeof editors[number]['id'];

export type SettingsReturnTarget = 'compact' | 'workbench';
export type WorkbenchSurface = 'compact' | 'workbench' | 'standaloneSettings';
export type WindowSizeMode = 'compact' | 'full';

export interface WorkbenchPresentation {
  /** Which primary UI surface should be rendered inside the desktop window. */
  surface: WorkbenchSurface;
  /** Target native window frame size category ('compact' vs 'full'). */
  windowSizeMode: WindowSizeMode;
  /** Whether the full workbench content (file list, tabs, footer) should be mounted in the DOM. */
  mountWorkbenchContent: boolean;
  /** Whether SettingsPanel should be rendered (standalone or overlaid on the workbench). */
  showSettings: boolean;
}

export function resolveWorkbenchPresentation(
  compactMode: boolean,
  settingsReturnTo: SettingsReturnTarget | null,
): WorkbenchPresentation {
  if (settingsReturnTo === 'compact') {
    return {
      surface: 'standaloneSettings',
      windowSizeMode: 'full',
      mountWorkbenchContent: false,
      showSettings: true,
    };
  }
  if (compactMode) {
    return {
      surface: 'compact',
      windowSizeMode: 'compact',
      mountWorkbenchContent: false,
      showSettings: false,
    };
  }
  return {
    surface: 'workbench',
    windowSizeMode: 'full',
    mountWorkbenchContent: true,
    showSettings: settingsReturnTo === 'workbench',
  };
}

export function defaultWindowPreferences(): PersistedWindowPreferences {
  return {
    pinned: false,
    compactMode: false,
    pinnedExpanded: false,
    fullSize: {
      width: WINDOW_DIMENSIONS.defaultFullWidth,
      height: WINDOW_DIMENSIONS.defaultFullHeight,
    },
  };
}

export function computeNextPinPreferences(
  current: PersistedWindowPreferences,
  nextPinned: boolean,
): PersistedWindowPreferences {
  return {
    ...current,
    pinned: nextPinned,
  };
}

export function computeNextCompactPreferences(
  current: PersistedWindowPreferences,
  nextCompactMode: boolean,
): PersistedWindowPreferences {
  return {
    ...current,
    compactMode: nextCompactMode,
  };
}

export function emptyWorkbench(): WorkbenchState {
  return {
    repositories: [],
    worktrees: [],
    activeWorktreeId: null,
    emptyPrompt: '',
    connections: [],
    models: [],
    activeModelId: null,
    locale: DEFAULT_APP_LOCALE,
    editor: 'idea',
    defaultCommitLanguage: DEFAULT_COMMIT_LANGUAGE,
    repositorySettings: {},
    windowPrefs: defaultWindowPreferences(),
    navigationRevision: 0,
    repositoryReads: {},
    repositoryBranches: {},
  };
}

export function effectiveEditorFor(state: WorkbenchState, repositoryId?: string | null): EditorId {
  const desired = repositoryId
    ? state.repositorySettings[repositoryId]?.editor ?? state.editor
    : state.editor;
  if (state.installedEditors?.length && !state.installedEditors.includes(desired)) {
    return state.installedEditors[0];
  }
  return desired;
}

export function effectiveCommitLanguageFor(state: WorkbenchState, repositoryId?: string | null): CommitLanguage {
  return repositoryId
    ? state.repositorySettings[repositoryId]?.commitLanguage ?? state.defaultCommitLanguage
    : state.defaultCommitLanguage;
}

export function localKeepPathsFor(state: WorkbenchState, repositoryId?: string | null): string[] {
  if (!repositoryId) return [];
  const fromSettings = state.repositorySettings[repositoryId]?.localKeepPaths;
  if (fromSettings) return fromSettings;
  return state.repositories.find((repository) => repository.id === repositoryId)?.localKeepPaths ?? [];
}

export function customActionsFor(state: WorkbenchState, repositoryId?: string | null): CustomActionConfig[] {
  if (!repositoryId) return [];
  return state.repositorySettings[repositoryId]?.customActions ?? [];
}

export function activeCustomActionFor(state: WorkbenchState, repositoryId?: string | null): CustomActionConfig | undefined {
  if (!repositoryId) return undefined;
  const settings = state.repositorySettings[repositoryId];
  const activeId = settings?.activeActionId;
  if (!activeId) return undefined;
  return settings.customActions?.find((item) => item.id === activeId);
}

export function computeSyncCustomActionAcrossRepos(
  state: WorkbenchState,
  sourceRepositoryId: string,
  action: CustomActionConfig,
  overwrite = false,
): { repositorySettings: Record<string, RepositorySettings>; result: CustomActionSyncResult } {
  const sanitized = sanitizeCustomActionConfig(action);
  const templateKey = sanitized.templateKey ?? sanitized.id;
  const normalizedLabel = sanitized.label.toLowerCase();
  const nextSettings: Record<string, RepositorySettings> = { ...state.repositorySettings };
  let added = 0;
  let updated = 0;
  let skipped = 0;

  // Ensure the source repository's copy records the same templateKey so future syncs recognize lineage.
  const sourcePrev = nextSettings[sourceRepositoryId];
  if (sourcePrev?.customActions?.some((item) => item.id === sanitized.id && !item.templateKey)) {
    nextSettings[sourceRepositoryId] = {
      ...sourcePrev,
      customActions: sourcePrev.customActions.map((item) =>
        item.id === sanitized.id ? { ...item, templateKey } : item,
      ),
    };
  }

  for (const repo of state.repositories) {
    if (repo.id === sourceRepositoryId) continue;
    const prev = nextSettings[repo.id];
    const existingList = prev?.customActions ?? [];
    const matchIndex = existingList.findIndex(
      (item) => (item.templateKey && item.templateKey === templateKey)
        || item.label.trim().toLowerCase() === normalizedLabel,
    );
    if (matchIndex >= 0) {
      if (!overwrite) {
        skipped += 1;
        continue;
      }
      const existing = existingList[matchIndex];
      const replaced = sanitizeCustomActionConfig({
        id: existing.id,
        templateKey,
        label: sanitized.label,
        prompt: sanitized.prompt,
      });
      const updatedList = existingList.map((item, idx) => (idx === matchIndex ? replaced : item));
      nextSettings[repo.id] = {
        editor: prev?.editor ?? effectiveEditorFor(state, repo.id),
        commitLanguage: prev?.commitLanguage ?? effectiveCommitLanguageFor(state, repo.id),
        ...(prev?.localKeepPaths?.length ? { localKeepPaths: prev.localKeepPaths } : {}),
        customActions: updatedList,
        ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
      };
      updated += 1;
      continue;
    }
    if (existingList.length >= CUSTOM_ACTION_LIMITS.maxPerRepo) {
      skipped += 1;
      continue;
    }
    const copy: CustomActionConfig = sanitizeCustomActionConfig({
      id: `act-${crypto.randomUUID().slice(0, 12)}`,
      templateKey,
      label: sanitized.label,
      prompt: sanitized.prompt,
    });
    nextSettings[repo.id] = {
      editor: prev?.editor ?? effectiveEditorFor(state, repo.id),
      commitLanguage: prev?.commitLanguage ?? effectiveCommitLanguageFor(state, repo.id),
      ...(prev?.localKeepPaths?.length ? { localKeepPaths: prev.localKeepPaths } : {}),
      customActions: [...existingList, copy],
      ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
    };
    added += 1;
  }

  return {
    repositorySettings: nextSettings,
    result: { added, updated, skipped },
  };
}

export function normalChanges(worktree: Pick<Worktree, 'files'>): Change[] {
  return worktree.files.filter((file) => !file.localKeep);
}

export function localKeepChanges(worktree: Pick<Worktree, 'files'>): Change[] {
  return worktree.files.filter((file) => !!file.localKeep);
}

export function selectedLocalKeepChanges(worktree: Pick<Worktree, 'files' | 'selectedIds'>): Change[] {
  return worktree.files.filter((file) => !!file.localKeep && worktree.selectedIds.includes(file.id));
}

export function computeNormalUnselectedIds(files: readonly Change[], selectedIds: readonly string[]): string[] {
  const selectedSet = new Set(selectedIds);
  return files.filter((file) => !file.localKeep && !selectedSet.has(file.id)).map((file) => file.id);
}

export function targetOf(worktree: Worktree): Target {
  return { repositoryId: worktree.repositoryId, worktreeId: worktree.worktreeId, sessionId: worktree.sessionId, scopeVersion: worktree.scopeVersion };
}

export function selectedFiles(worktree: Worktree): Change[] {
  return worktree.files.filter((file) => worktree.selectedIds.includes(file.id));
}

export function scopeSummary(worktree?: Worktree, locale: AppLocale = DEFAULT_APP_LOCALE): string {
  const t = getMessages(locale).changes;
  if (worktree?.readError) return t.statusUnavailable;
  if (!worktree || !worktree.files.length) return t.noChanges;
  const selected = selectedFiles(worktree);
  const normalCount = normalChanges(worktree).length;
  const selectedKeptCount = selectedLocalKeepChanges(worktree).length;
  const conflicts = worktree.files.filter((file) => file.status === 'U').length;
  if (!selected.length) {
    return conflicts ? `${t.noneSelected} · ${t.localKeepConflicts(conflicts)}` : t.noneSelected;
  }
  return t.selectedSummary(
    selected.length,
    normalCount + selectedKeptCount,
    selected.some((file) => file.range.kind === 'partial'),
    conflicts,
  );
}

export function splitPath(path: string) {
  const separator = path.lastIndexOf('/');
  return { directory: path.slice(0, Math.max(separator, 0)), name: path.slice(separator + 1) };
}

export function groupFiles(files: Change[], rootDirectoryLabel = getMessages().changes.rootDirectory) {
  const groups = new Map<string, Change[]>();
  for (const file of files) {
    const directory = splitPath(file.path).directory || rootDirectoryLabel;
    groups.set(directory, [...(groups.get(directory) ?? []), file]);
  }
  return [...groups].map(([directory, files]) => ({ directory, files }));
}

export const messageLabels: Record<Message['kind'], string> = getMessages().messageLabels;

const priorities: Record<Message['kind'], number> = {
  conflict: 100, question: 95, waitingManual: 90, resolved: 85, pushFailed: 80, error: 75, progress: 60, review: 20, done: 30,
};
export function orderedMessages(messages: Message[]) {
  return messages.toSorted((a, b) => priorities[b.kind] - priorities[a.kind]);
}
export function isProtected(branch: string) { return branch === 'main' || branch === 'master'; }
export function isActiveSession(worktree: Worktree) { return worktree.sessionStatus !== 'idle' && worktree.sessionStatus !== 'done'; }

export function isPlanOutdated(worktree: Pick<Worktree, 'plan' | 'scopeVersion'>): boolean {
  return !!worktree.plan && worktree.plan.baseScopeVersion !== worktree.scopeVersion;
}

export function activeValidPlan(worktree: Pick<Worktree, 'plan' | 'scopeVersion'>): EditableCommitPlan | undefined {
  return worktree.plan && !isPlanOutdated(worktree) ? worktree.plan : undefined;
}

export function hasEmptyValidPlanMessage(worktree: Pick<Worktree, 'plan' | 'scopeVersion'>): boolean {
  const plan = activeValidPlan(worktree);
  return !!plan && plan.groups.some((group) => !group.message.trim());
}

export function isModelErrorText(message: string): boolean {
  if (!message) return false;
  const lower = message.toLowerCase();
  return /\b(?:401|402|403|404|429|5\d\d)\b/.test(lower)
    || lower.includes('authentication')
    || lower.includes('unauthorized')
    || lower.includes('api key')
    || lower.includes('base url')
    || lower.includes('quota')
    || lower.includes('rate limit')
    || lower.includes('insufficient balance')
    || lower.includes('timed out')
    || lower.includes('timeout')
    || lower.includes('connect to model')
    || lower.includes('model service')
    || lower.includes('model request')
    || lower.includes('model provider')
    || lower.includes('selected model')
    || lower.includes('credentials.json')
    || lower.includes('provider connection')
    || message.includes('\u9274\u6743')
    || message.includes('\u989d\u5ea6')
    || message.includes('\u8bf7\u6c42\u8fc7\u4e8e\u9891\u7e41')
    || message.includes('\u6a21\u578b\u670d\u52a1')
    || message.includes('\u8fde\u63a5\u6a21\u578b')
    || message.includes('\u6a21\u578b\u8bf7\u6c42')
    || message.includes('\u63a5\u53e3\u5730\u5740')
    || message.includes('\u4f9b\u5e94\u5546\u8fde\u63a5')
    || message.includes('\u5f53\u524d\u9009\u4e2d\u7684\u6a21\u578b');
}

export type CompactExpandTarget = 'changes' | 'dock' | 'plan' | 'repo';
export type CompactStatusTone = 'muted' | 'ready' | 'running' | 'warning' | 'danger' | 'success';

export interface CompactStatusSummary {
  tone: CompactStatusTone;
  label: string;
  expandTarget?: CompactExpandTarget;
}

function extractCommitStepProgress(message?: Message): { completed: number; total: number } | undefined {
  if (!message) return undefined;
  if (message.id.includes('-step-') || /commit|\u63d0\u4ea4/i.test(`${message.title} ${message.progress?.label ?? ''}`)) {
    const match = /(\d+)\s*\/\s*(\d+)/.exec(`${message.progress?.label ?? ''} ${message.title}`);
    if (match) {
      const completed = Number(match[1]);
      const total = Number(match[2]);
      if (completed > 0 && total >= completed) return { completed, total };
    }
  }
  return undefined;
}

export function compactStatusSummary(
  worktree: Worktree | undefined,
  hasModel: boolean,
  locale: AppLocale = DEFAULT_APP_LOCALE,
): CompactStatusSummary {
  const t = getMessages(locale).compact.status;
  if (!worktree) {
    return { tone: 'muted', label: t.noRepo, expandTarget: 'repo' };
  }
  if (isActiveSession(worktree)) {
    switch (worktree.sessionStatus) {
      case 'conflict':
        return { tone: 'danger', label: t.conflict, expandTarget: 'dock' };
      case 'pushFailed':
        return { tone: 'danger', label: t.pushFailed, expandTarget: 'dock' };
      case 'needsDecision':
        return { tone: 'warning', label: t.needsDecision, expandTarget: 'dock' };
      case 'waitingManual':
        return { tone: 'warning', label: t.waitingManual, expandTarget: 'dock' };
      case 'running': {
        if (worktree.cPush?.planOnly) {
          return { tone: 'running', label: t.planning };
        }
        const progressMsg = orderedMessages(worktree.messages).find((item) => item.kind === 'progress');
        const step = extractCommitStepProgress(progressMsg);
        return {
          tone: 'running',
          label: step ? t.committingStep(step.completed, step.total) : t.running,
        };
      }
    }
  }
  if (worktree.requests.plan) {
    return { tone: 'running', label: t.planning, expandTarget: 'plan' };
  }
  if (worktree.readError) {
    return { tone: 'danger', label: t.readError, expandTarget: 'changes' };
  }
  if (worktree.detached) {
    return { tone: 'warning', label: t.detachedHead, expandTarget: 'changes' };
  }
  if (worktree.plan) {
    if (isPlanOutdated(worktree)) {
      return {
        tone: 'warning',
        label: t.planOutdated(worktree.plan.groups.length),
        expandTarget: 'plan',
      };
    }
    if (hasEmptyValidPlanMessage(worktree)) {
      return { tone: 'danger', label: t.emptyPlanMessage, expandTarget: 'plan' };
    }
    return {
      tone: 'ready',
      label: t.planReady(worktree.plan.groups.length),
      expandTarget: 'plan',
    };
  }
  if (worktree.files.length === 0) {
    const doneMsg = worktree.sessionStatus === 'done'
      ? orderedMessages(worktree.messages).find((item) => item.kind === 'done')
      : undefined;
    if (doneMsg) {
      return { tone: 'success', label: `${doneMsg.title} ▸`, expandTarget: 'dock' };
    }
    return { tone: 'muted', label: t.clean };
  }
  const selectedCount = selectedFiles(worktree).length;
  const total = normalChanges(worktree).length + selectedLocalKeepChanges(worktree).length;
  if (selectedCount === 0) {
    return { tone: 'muted', label: t.noneSelected(worktree.files.length), expandTarget: 'changes' };
  }
  if (!hasModel) {
    return { tone: 'warning', label: t.selectedNoModel(selectedCount, total), expandTarget: 'changes' };
  }
  return { tone: 'ready', label: t.selectedReady(selectedCount, total), expandTarget: 'changes' };
}

export function primaryAction(
  worktree: Worktree | undefined,
  hasModel: boolean,
  locale: AppLocale = DEFAULT_APP_LOCALE,
  activeCustomAction?: CustomActionConfig,
) {
  const labels = getMessages(locale).primaryActions;
  if (worktree && isActiveSession(worktree)) {
    return {
      label: labels[worktree.sessionStatus as 'running' | 'needsDecision' | 'conflict' | 'waitingManual' | 'pushFailed'],
      configureModel: false,
      resume: true,
      disabled: false,
    };
  }
  if (!hasModel) {
    return {
      label: labels.configureModel,
      configureModel: true,
      resume: false,
      disabled: false,
    };
  }
  if (activeCustomAction) {
    return {
      label: activeCustomAction.label,
      configureModel: false,
      resume: false,
      disabled: !worktree || !!worktree.readError,
    };
  }
  return {
    label: labels.cPush,
    configureModel: false,
    resume: false,
    disabled: !worktree || !!worktree.readError || selectedFiles(worktree).length === 0 || hasEmptyValidPlanMessage(worktree),
  };
}

export type WorkbenchEvent =
  | { type: 'message'; target: Target; message: Message; sessionStatus?: Worktree['sessionStatus'] }
  | { type: 'planPrepared'; target: Target; requestId: string; modelId: string; plan: CPushPreparedPlan }
  | { type: 'reviewed'; target: Target; requestId: string; findings: Finding[]; skippedPaths: string[]; reviewedFileIds?: string[] }
  | { type: 'files'; target: Target; nextScopeVersion: string; files: Change[]; changedPaths: string[] }
  | { type: 'cPushProgress'; progress: CPushProgressEvent }
  | { type: 'customActionProgress'; progress: CustomActionProgressEvent };

export type WorkbenchAction =
  | { type: 'workbenchHydrated'; restored: WorkbenchRestoreReply }
  | { type: 'cPushStarted'; worktreeId: string; runId: string; frozenTarget: Target; frozenFileIds: string[]; modelId: string; expandDock: boolean }
  | { type: 'cPushSettled'; reply: CPushCommandReply; expandDock?: boolean }
  | { type: 'cPushStartFailed'; worktreeId: string; runId: string }
  | { type: 'customActionStarted'; worktreeId: string; runId: string; actionId: string; actionLabel: string; expandDock: boolean }
  | { type: 'customActionSettled'; reply: CustomActionCommandReply; expandDock?: boolean }
  | { type: 'customActionFailed'; worktreeId: string; runId: string; message?: string }
  | { type: 'selectActiveAction'; repositoryId: string; actionId: string | null }
  | { type: 'saveCustomAction'; repositoryId: string; action: CustomActionConfig; selectAsActive?: boolean }
  | { type: 'deleteCustomAction'; repositoryId: string; actionId: string }
  | { type: 'syncCustomActionToAllRepos'; sourceRepositoryId: string; action: CustomActionConfig; overwrite?: boolean }
  | { type: 'trustCustomActionWrites'; repositoryId: string; actionId: string; trustedPromptHash: string }
  | { type: 'resetCustomActionWriteTrust'; repositoryId: string; actionId: string }
  | { type: 'repositoryBranchStarted'; repositoryId: string; requestId: string }
  | { type: 'repositoryBranchFinished'; repositoryId: string; reply: GitBranchReply }
  | { type: 'repositoryBranchFailed'; repositoryId: string; requestId: string }
  | { type: 'repositoryReadStarted'; key: string; requestId: string }
  | { type: 'repositoryReadFinished'; key: string; reply: GitReadReply }
  | { type: 'repositoryReadFailed'; key: string; requestId: string; worktreeId?: string; error?: string }
  | { type: 'localKeepUpdated'; repositoryId: string; paths: string[]; keep: boolean }
  | { type: 'repositoryLocalKeepFinished'; repositoryId: string; reply: GitLocalKeepReply }
  | { type: 'switch'; worktreeId: string; expand?: boolean }
  | { type: 'select'; worktreeId: string; fileId: string }
  | { type: 'selectAll'; worktreeId: string }
  | { type: 'selectFiles'; worktreeId: string; fileIds: string[] }
  | { type: 'prompt'; worktreeId: string | null; text: string }
  | { type: 'clearPrompt'; worktreeId: string | null }
  | { type: 'planGroupEdited'; worktreeId: string; groupId: string; message: string }
  | { type: 'planCleared'; worktreeId: string }
  | { type: 'tab'; worktreeId: string; tab: Worktree['tab'] }
  | { type: 'dock'; worktreeId: string; expanded: boolean }
  | { type: 'dismissMessage'; worktreeId: string; messageId: string }
  | { type: 'reply'; worktreeId: string; messageId: string; text: string }
  | { type: 'request'; target: Target; kind: 'plan' | 'review'; requestId: string; fileIds?: string[] }
  | { type: 'requestFailed'; target: Target; kind: 'plan' | 'review'; requestId: string; error?: string }
  | { type: 'locale'; locale: AppLocale }
  | { type: 'editor'; editor: EditorId; repositoryId?: string | null }
  | { type: 'commitLanguage'; commitLanguage: CommitLanguage; repositoryId?: string | null }
  | { type: 'windowPreferencesUpdated'; preferences: PersistedWindowPreferences }
  | { type: 'windowSnapshotSynced'; pinned: boolean; position: WindowPosition; size: WindowSize }
  | { type: 'windowGeometryObserved'; position: WindowPosition; size: WindowSize }
  | { type: 'catalogSynced'; connections: ProviderConnection[]; models: ModelConfiguration[]; activeModelId: string | null }
  | { type: 'providerKeyHintSynced'; connectionId: string; keyHint: string }
  | { type: 'modelSelected'; modelId: string }
  | { type: 'modelDeleted'; modelId: string }
  | { type: 'providerDeleted'; connectionId: string }
  | { type: 'event'; event: WorkbenchEvent };

function updateWorktreeSelection(worktree: Worktree, selectedIds: string[]): Worktree {
  const nextScopeVersion = worktree.statusVersion !== undefined
    ? readScopeVersion({ ...worktree, selectedIds })
    : `${worktree.scopeVersion.split('#')[0]}#${selectedIds.join(',')}`;
  return {
    ...worktree,
    selectedIds,
    unselectedIds: computeNormalUnselectedIds(worktree.files, selectedIds),
    scopeVersion: nextScopeVersion,
    requests: {},
    review: {
      ...worktree.review,
      status: worktree.review.status === 'running' ? worktree.requests.review?.previousReviewStatus ?? 'idle' : worktree.review.status,
    },
  };
}

function matchesTarget(worktree: Worktree, target: Target) {
  return worktree.repositoryId === target.repositoryId && worktree.worktreeId === target.worktreeId
    && worktree.sessionId === target.sessionId && worktree.scopeVersion === target.scopeVersion;
}

function receiveEvent(worktree: Worktree, event: WorkbenchEvent, keepSet: ReadonlySet<string>): Worktree {
  if (event.type === 'cPushProgress' || event.type === 'customActionProgress') return worktree;
  if (!matchesTarget(worktree, event.target)) return worktree;
  switch (event.type) {
    case 'message':
      return { ...worktree, sessionStatus: event.sessionStatus ?? worktree.sessionStatus,
        dockExpanded: event.sessionStatus === 'done' ? false : worktree.dockExpanded,
        messages: [...worktree.messages.filter((message) => message.id !== event.message.id && (event.sessionStatus !== 'done' || message.kind === 'review')), event.message] };
    case 'planPrepared': {
      const request = worktree.requests.plan;
      if (request?.id !== event.requestId) return worktree;
      if (request.promptRevision !== worktree.prompt.revision || request.planRevision !== worktree.planRevision) {
        return { ...worktree, requests: { ...worktree.requests, plan: undefined } };
      }
      return {
        ...worktree,
        requests: { ...worktree.requests, plan: undefined },
        plan: {
          id: event.requestId,
          baseScopeVersion: event.target.scopeVersion,
          modelId: event.modelId,
          summary: event.plan.summary,
          groups: event.plan.groups.map((group) => ({ ...group, fileIds: [...group.fileIds] })),
          skippedFileIds: [...event.plan.skippedFileIds],
        },
        planRevision: worktree.planRevision + 1,
      };
    }
    case 'reviewed': {
      const request = worktree.requests.review;
      if (request?.id !== event.requestId) return worktree;
      const reviewedIds = new Set(request.fileIds);
      const preserved = worktree.review.findings.filter((finding) => !reviewedIds.has(finding.fileId) && worktree.selectedIds.includes(finding.fileId));
      const reviewedPaths = worktree.files.filter((file) => reviewedIds.has(file.id)).map((file) => file.path);
      const matchesSkipped = (entry: string, path: string) =>
        entry === path || entry.startsWith(`${path} (`) || entry.startsWith(`${path}\uff08`);
      const incomingReviewedIds = event.reviewedFileIds
        ?? request.fileIds.filter((id) => {
          const file = worktree.files.find((item) => item.id === id);
          return !!file && !file.ignoredForReview && !file.binary;
        });
      const preservedReviewedIds = (worktree.review.reviewedFileIds ?? []).filter((id) => !reviewedIds.has(id) && worktree.selectedIds.includes(id));
      return { ...worktree, requests: { ...worktree.requests, review: undefined },
        review: { status: 'done', findings: [...preserved, ...event.findings],
          scopeFileIds: [...new Set([...(worktree.review.scopeFileIds ?? []).filter((id) => worktree.selectedIds.includes(id)), ...request.fileIds])],
          reviewedFileIds: [...new Set([...preservedReviewedIds, ...incomingReviewedIds])],
          skippedPaths: [...new Set([...worktree.review.skippedPaths.filter((entry) => !reviewedPaths.some((path) => matchesSkipped(entry, path))), ...event.skippedPaths])],
          changedPaths: worktree.review.changedPaths.filter((path) => !reviewedPaths.includes(path)) } };
    }
    case 'files': {
      const incomingFiles = event.files.map((file) => ({
        ...file,
        localKeep: file.localKeep ?? keepSet.has(file.path),
      }));
      const prevById = new Map(worktree.files.map((file) => [file.id, file]));
      const prevSelectedSet = new Set(worktree.selectedIds);
      const prevUnselectedSet = new Set(worktree.unselectedIds ?? computeNormalUnselectedIds(worktree.files, worktree.selectedIds));
      const nextSelectedIds = incomingFiles
        .filter((nextFile) => {
          if (nextFile.localKeep) {
            return Boolean(prevById.get(nextFile.id)?.localKeep && prevSelectedSet.has(nextFile.id));
          }
          return !prevUnselectedSet.has(nextFile.id);
        })
        .map((file) => file.id);
      const nextUnselectedIds = computeNormalUnselectedIds(incomingFiles, nextSelectedIds);
      const selectedPaths = selectedFiles(worktree).flatMap((file) => [file.path, file.previousPath].filter((path): path is string => !!path));
      const relevant = event.nextScopeVersion !== worktree.scopeVersion || event.changedPaths.some((path) => selectedPaths.includes(path));
      const reviewedPaths = worktree.files.filter((file) => worktree.review.scopeFileIds?.includes(file.id)).map((file) => file.path);
      return { ...worktree, files: incomingFiles, scopeVersion: event.nextScopeVersion,
        selectedIds: nextSelectedIds,
        unselectedIds: nextUnselectedIds,
        review: { ...worktree.review, status: relevant && worktree.review.status === 'running' ? worktree.requests.review?.previousReviewStatus ?? 'idle' : worktree.review.status,
          changedPaths: [...new Set([...worktree.review.changedPaths, ...event.changedPaths.filter((path) => reviewedPaths.includes(path))])] },
        requests: relevant ? {} : worktree.requests };
    }
  }
}

export function workbenchReducer(state: WorkbenchState, action: WorkbenchAction): WorkbenchState {
  if (action.type === 'workbenchHydrated') return hydrateWorkbench(state, action.restored);
  if (action.type === 'event' && action.event.type === 'cPushProgress') {
    return receiveCPushProgress(state, action.event.progress);
  }
  if (action.type === 'event' && action.event.type === 'customActionProgress') {
    return applyCustomActionProgress(state, action.event.progress);
  }
  if (action.type === 'customActionStarted') {
    const t = getMessages(state.locale).workbench;
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || (isActiveSession(tree) && !tree.cPush?.planOnly)) return tree;
        const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
        const progressMsg: Message = {
          id: `progress-${action.runId}`,
          kind: 'progress',
          title: action.actionLabel,
          description: t.cPushInitialDescription,
          progress: { completed: 1, total: 3, label: t.cPushInitialProgressLabel },
        };
        return {
          ...tree,
          sessionStatus: 'running',
          dockExpanded: action.expandDock ? true : tree.dockExpanded,
          messages: [...reviewMessages, progressMsg],
          cPush: undefined,
          customAction: {
            runId: action.runId,
            actionId: action.actionId,
            actionLabel: action.actionLabel,
            timeline: [{ state: 'active', label: action.actionLabel }],
            commits: [],
            links: [],
          },
        };
      }),
    };
  }
  if (action.type === 'customActionSettled') {
    return finishCustomActionCommand(state, action.reply, action.expandDock);
  }
  if (action.type === 'customActionFailed') {
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || tree.customAction?.runId !== action.runId) {
          return tree;
        }
        const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
        const errorMsg: Message | undefined = action.message
          ? {
              id: `error-${action.runId}`,
              kind: 'error',
              title: tree.customAction.actionLabel,
              description: action.message,
            }
          : undefined;
        return {
          ...tree,
          sessionStatus: 'idle',
          dockExpanded: false,
          messages: errorMsg ? [...reviewMessages, errorMsg] : reviewMessages,
          customAction: undefined,
        };
      }),
    };
  }
  if (action.type === 'selectActiveAction') {
    const prev = state.repositorySettings[action.repositoryId];
    const actions = prev?.customActions ?? [];
    const nextActiveId = action.actionId && actions.some((item) => item.id === action.actionId)
      ? action.actionId
      : undefined;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    return {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          ...(actions.length ? { customActions: actions } : {}),
          ...(nextActiveId ? { activeActionId: nextActiveId } : {}),
        },
      },
      settingsModified: true,
    };
  }
  if (action.type === 'saveCustomAction') {
    const sanitized = sanitizeCustomActionConfig(action.action);
    if (!sanitized.id || !sanitized.label || !sanitized.prompt) return state;
    const prev = state.repositorySettings[action.repositoryId];
    const existing = prev?.customActions ?? [];
    const idx = existing.findIndex((item) => item.id === sanitized.id);
    let nextActions: CustomActionConfig[];
    if (idx >= 0) {
      const prior = existing[idx];
      const promptUnchanged = prior.prompt === sanitized.prompt;
      const merged = sanitizeCustomActionConfig({
        ...sanitized,
        templateKey: sanitized.templateKey ?? prior.templateKey,
        ...(promptUnchanged && prior.skipWriteConfirm && prior.trustedPromptHash
          ? { skipWriteConfirm: prior.skipWriteConfirm, trustedPromptHash: prior.trustedPromptHash }
          : {}),
      });
      nextActions = existing.map((item, i) => (i === idx ? merged : item));
    } else {
      if (existing.length >= CUSTOM_ACTION_LIMITS.maxPerRepo) return state;
      nextActions = [...existing, sanitized];
    }
    const nextActiveId = action.selectAsActive
      ? sanitized.id
      : prev?.activeActionId && nextActions.some((item) => item.id === prev.activeActionId)
        ? prev.activeActionId
        : undefined;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    return {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          customActions: nextActions,
          ...(nextActiveId ? { activeActionId: nextActiveId } : {}),
        },
      },
      settingsModified: true,
    };
  }
  if (action.type === 'deleteCustomAction') {
    const prev = state.repositorySettings[action.repositoryId];
    const existing = prev?.customActions ?? [];
    const nextActions = existing.filter((item) => item.id !== action.actionId);
    if (nextActions.length === existing.length) return state;
    const nextActiveId = prev?.activeActionId === action.actionId ? undefined : prev?.activeActionId ?? undefined;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    return {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          ...(nextActions.length ? { customActions: nextActions } : {}),
          ...(nextActiveId ? { activeActionId: nextActiveId } : {}),
        },
      },
      settingsModified: true,
    };
  }
  if (action.type === 'syncCustomActionToAllRepos') {
    const { repositorySettings, result } = computeSyncCustomActionAcrossRepos(
      state,
      action.sourceRepositoryId,
      action.action,
      Boolean(action.overwrite),
    );
    if (result.added === 0 && result.updated === 0) return state;
    return {
      ...state,
      repositorySettings,
      settingsModified: true,
    };
  }
  if (action.type === 'trustCustomActionWrites' || action.type === 'resetCustomActionWriteTrust') {
    const prev = state.repositorySettings[action.repositoryId];
    const existing = prev?.customActions ?? [];
    let changed = false;
    const nextActions = existing.map((item) => {
      if (item.id !== action.actionId) return item;
      if (action.type === 'resetCustomActionWriteTrust') {
        if (!item.skipWriteConfirm && !item.trustedPromptHash) return item;
        changed = true;
        return sanitizeCustomActionConfig({
          id: item.id,
          ...(item.templateKey ? { templateKey: item.templateKey } : {}),
          label: item.label,
          prompt: item.prompt,
        });
      }
      const expected = computeCustomActionPromptHash(item.prompt);
      if (expected !== action.trustedPromptHash) return item;
      changed = true;
      return {
        ...item,
        skipWriteConfirm: true,
        trustedPromptHash: expected,
      };
    });
    if (!changed) return state;
    const keepPaths = localKeepPathsFor(state, action.repositoryId);
    return {
      ...state,
      repositorySettings: {
        ...state.repositorySettings,
        [action.repositoryId]: {
          editor: effectiveEditorFor(state, action.repositoryId),
          commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
          ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
          customActions: nextActions,
          ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
        },
      },
      settingsModified: true,
    };
  }
  if (action.type === 'cPushStarted') {
    const t = getMessages(state.locale).workbench;
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || (isActiveSession(tree) && !tree.cPush?.planOnly)) return tree;
        const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
        const progressMsg: Message = {
          id: `progress-${action.runId}`,
          kind: 'progress',
          title: t.cPushInitialTitle,
          description: t.cPushInitialDescription,
          scope: t.cPushInitialScope(action.frozenFileIds.length),
          progress: { completed: 1, total: 3, label: t.cPushInitialProgressLabel },
        };
        return {
          ...tree,
          requests: { ...tree.requests, plan: undefined },
          sessionStatus: 'running',
          dockExpanded: action.expandDock ? true : tree.dockExpanded,
          messages: [...reviewMessages, progressMsg],
          cPush: {
            runId: action.runId,
            frozenTarget: action.frozenTarget,
            frozenFileIds: action.frozenFileIds,
            promptRevisionAtStart: tree.prompt.revision,
            modelId: action.modelId,
            timeline: [
              { state: 'done', label: t.cPushTimelineFrozen(action.frozenFileIds.length) },
              { state: 'active', label: t.cPushTimelinePlanning },
            ],
            qaLog: [],
            commits: [],
            canRetryRemaining: false,
            planOnly: false,
          },
        };
      }),
    };
  }
  if (action.type === 'cPushSettled') return finishCPushCommand(state, action.reply, action.expandDock);
  if (action.type === 'cPushStartFailed') {
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || tree.cPush?.runId !== action.runId || tree.cPush.commits.length > 0) {
          return tree;
        }
        return {
          ...tree,
          sessionStatus: 'idle',
          dockExpanded: false,
          messages: tree.messages.filter((msg) => msg.kind === 'review'),
          cPush: undefined,
        };
      }),
    };
  }
  if (action.type === 'repositoryBranchStarted') return { ...state, navigationRevision: state.navigationRevision + 1, repositoryBranches: { ...state.repositoryBranches, [action.repositoryId]: { requestId: action.requestId, navigationRevision: state.navigationRevision + 1 } } };
  if (action.type === 'repositoryBranchFinished') return finishRepositoryBranch(state, action.repositoryId, action.reply);
  if (action.type === 'repositoryBranchFailed') {
    if (state.repositoryBranches?.[action.repositoryId]?.requestId !== action.requestId) return state;
    const repositoryBranches = { ...state.repositoryBranches }; delete repositoryBranches[action.repositoryId];
    return { ...state, repositoryBranches };
  }
  if (action.type === 'repositoryReadStarted') {
    const navigationRevision = state.navigationRevision + (action.key === 'open' ? 1 : 0);
    return { ...state, navigationRevision, repositoryReads: { ...state.repositoryReads, [action.key]: { requestId: action.requestId, navigationRevision } } };
  }
  if (action.type === 'repositoryReadFinished') return finishRepositoryRead(state, action.key, action.reply);
  if (action.type === 'repositoryReadFailed') {
    if (state.repositoryReads[action.key]?.requestId !== action.requestId) return state;
    const repositoryReads = { ...state.repositoryReads }; delete repositoryReads[action.key];
    const fallbackError = getMessages(state.locale).workbench.readFailedFallback;
    return { ...state, repositoryReads, worktrees: state.worktrees.map((tree) => tree.worktreeId === action.worktreeId
      ? { ...tree, readError: action.error ?? fallbackError,
        review: { ...tree.review, status: tree.review.status === 'running' ? tree.requests.review?.previousReviewStatus ?? 'idle' : tree.review.status }, requests: {} } : tree) };
  }
  if (action.type === 'localKeepUpdated') {
    return applyLocalKeepUpdate(state, action.repositoryId, action.paths, action.keep);
  }
  if (action.type === 'repositoryLocalKeepFinished') {
    return finishRepositoryLocalKeep(state, action.repositoryId, action.reply);
  }
  if (action.type === 'locale') {
    return { ...state, locale: action.locale, settingsModified: true };
  }
  if (action.type === 'windowPreferencesUpdated') {
    return {
      ...state,
      windowPrefs: {
        ...action.preferences,
        ...(action.preferences.fullSize ? { fullSize: { ...action.preferences.fullSize } } : {}),
        ...(action.preferences.fullPosition ? { fullPosition: { ...action.preferences.fullPosition } } : {}),
        ...(action.preferences.compactPosition ? { compactPosition: { ...action.preferences.compactPosition } } : {}),
      },
      settingsModified: true,
    };
  }
  if (action.type === 'windowSnapshotSynced') {
    if (state.settingsModified || state.windowPrefs.compactMode || state.windowPrefs.fullPosition) {
      return state;
    }
    return {
      ...state,
      windowPrefs: {
        ...state.windowPrefs,
        pinned: action.pinned,
        fullPosition: { ...action.position },
        fullSize: { ...action.size },
      },
    };
  }
  if (action.type === 'windowGeometryObserved') {
    if (state.windowPrefs.compactMode) {
      const prev = state.windowPrefs.compactPosition;
      if (prev && prev.x === action.position.x && prev.y === action.position.y) return state;
      return {
        ...state,
        windowPrefs: {
          ...state.windowPrefs,
          compactPosition: { ...action.position },
        },
        settingsModified: true,
      };
    }
    const validFullSize = action.size.width >= WINDOW_DIMENSIONS.minFullWidth
      && action.size.height >= WINDOW_DIMENSIONS.minFullHeight;
    const nextSize = validFullSize ? { ...action.size } : state.windowPrefs.fullSize;
    const prevPos = state.windowPrefs.fullPosition;
    const prevSize = state.windowPrefs.fullSize;
    if (
      prevPos?.x === action.position.x
      && prevPos?.y === action.position.y
      && prevSize?.width === nextSize?.width
      && prevSize?.height === nextSize?.height
    ) {
      return state;
    }
    return {
      ...state,
      windowPrefs: {
        ...state.windowPrefs,
        fullPosition: { ...action.position },
        ...(nextSize ? { fullSize: nextSize } : {}),
      },
      settingsModified: true,
    };
  }
  if (action.type === 'editor') {
    if (action.repositoryId) {
      const prev = state.repositorySettings[action.repositoryId];
      const keepPaths = localKeepPathsFor(state, action.repositoryId);
      return {
        ...state,
        editor: action.editor,
        repositorySettings: {
          ...state.repositorySettings,
          [action.repositoryId]: {
            editor: action.editor,
            commitLanguage: effectiveCommitLanguageFor(state, action.repositoryId),
            ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
            ...(prev?.customActions?.length ? { customActions: prev.customActions } : {}),
            ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
          },
        },
        settingsModified: true,
      };
    }
    return { ...state, editor: action.editor, settingsModified: true };
  }
  if (action.type === 'commitLanguage') {
    if (action.repositoryId) {
      const prev = state.repositorySettings[action.repositoryId];
      const keepPaths = localKeepPathsFor(state, action.repositoryId);
      return {
        ...state,
        repositorySettings: {
          ...state.repositorySettings,
          [action.repositoryId]: {
            editor: effectiveEditorFor(state, action.repositoryId),
            commitLanguage: action.commitLanguage,
            ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
            ...(prev?.customActions?.length ? { customActions: prev.customActions } : {}),
            ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
          },
        },
        settingsModified: true,
      };
    }
    return { ...state, defaultCommitLanguage: action.commitLanguage, settingsModified: true };
  }
  if (action.type === 'catalogSynced') {
    return {
      ...state,
      connections: action.connections,
      models: action.models,
      activeModelId: action.activeModelId,
    };
  }
  if (action.type === 'providerKeyHintSynced') {
    return {
      ...state,
      connections: state.connections.map((conn) =>
        conn.id === action.connectionId ? { ...conn, keyHint: action.keyHint } : conn,
      ),
    };
  }
  if (action.type === 'modelSelected') return state.models.some((model) => model.id === action.modelId) ? { ...state, activeModelId: action.modelId } : state;
  if (action.type === 'modelDeleted') {
    const models = state.models.filter((model) => model.id !== action.modelId);
    return { ...state, models, activeModelId: state.activeModelId === action.modelId ? models[0]?.id ?? null : state.activeModelId };
  }
  if (action.type === 'providerDeleted') {
    const connections = state.connections.filter((conn) => conn.id !== action.connectionId);
    const models = state.models.filter((model) => model.connectionId !== action.connectionId);
    const activeModelId = models.some((model) => model.id === state.activeModelId)
      ? state.activeModelId
      : models[0]?.id ?? null;
    return { ...state, connections, models, activeModelId };
  }
  if (action.type === 'switch') {
    if (!state.worktrees.some((worktree) => worktree.worktreeId === action.worktreeId)) return state;
    return { ...state, activeWorktreeId: action.worktreeId, navigationRevision: state.navigationRevision + 1, worktrees: state.worktrees.map((worktree) => worktree.worktreeId === action.worktreeId && action.expand
      ? { ...worktree, dockExpanded: true } : worktree) };
  }
  if ((action.type === 'prompt' || action.type === 'clearPrompt') && action.worktreeId === null) {
    return { ...state, emptyPrompt: action.type === 'prompt' ? action.text : '' };
  }
  return { ...state, worktrees: state.worktrees.map((worktree) => {
    if (action.type === 'event') return receiveEvent(worktree, action.event, new Set(localKeepPathsFor(state, worktree.repositoryId)));
    if (action.type === 'request' || action.type === 'requestFailed') {
      if (!matchesTarget(worktree, action.target)) return worktree;
      if (action.type === 'requestFailed') {
        if (worktree.requests[action.kind]?.id !== action.requestId) return worktree;
        if (action.kind === 'review') {
          const prevStatus = worktree.requests.review?.previousReviewStatus ?? 'idle';
          return {
            ...worktree,
            requests: { ...worktree.requests, review: undefined },
            review: {
              ...worktree.review,
              status: action.error && prevStatus !== 'done' ? 'failed' : prevStatus,
              error: action.error,
            },
          };
        }
        return { ...worktree, requests: { ...worktree.requests, plan: undefined } };
      }
      if (action.kind === 'plan') {
        return {
          ...worktree,
          requests: {
            ...worktree.requests,
            plan: {
              id: action.requestId,
              promptRevision: worktree.prompt.revision,
              planRevision: worktree.planRevision,
              fileIds: action.fileIds ?? worktree.selectedIds,
            },
          },
        };
      }
      return {
        ...worktree,
        requests: {
          ...worktree.requests,
          review: {
            id: action.requestId,
            promptRevision: worktree.prompt.revision,
            fileIds: action.fileIds ?? worktree.selectedIds,
            previousReviewStatus: worktree.requests.review?.previousReviewStatus ?? worktree.review.status,
          },
        },
        review: { ...worktree.review, status: 'running', error: undefined },
      };
    }
    if (worktree.worktreeId !== action.worktreeId) return worktree;
    switch (action.type) {
      case 'select': {
        if (!worktree.files.some((file) => file.id === action.fileId)) return worktree;
        const selectedIds = worktree.selectedIds.includes(action.fileId) ? worktree.selectedIds.filter((id) => id !== action.fileId) : [...worktree.selectedIds, action.fileId];
        return updateWorktreeSelection(worktree, selectedIds);
      }
      case 'selectAll': {
        if (!worktree.files.length) return worktree;
        const normal = normalChanges(worktree);
        if (!normal.length) {
          return worktree.selectedIds.length ? updateWorktreeSelection(worktree, []) : worktree;
        }
        const allNormalSelected = normal.every((file) => worktree.selectedIds.includes(file.id));
        const selectedIds = allNormalSelected
          ? []
          : [...new Set([...worktree.selectedIds, ...normal.map((file) => file.id)])];
        return updateWorktreeSelection(worktree, selectedIds);
      }
      case 'selectFiles': {
        const liveIds = new Set(worktree.files.map((file) => file.id));
        const targetIds = [...new Set(action.fileIds)].filter((id) => liveIds.has(id));
        if (!targetIds.length) return worktree;
        const targetSet = new Set(targetIds);
        const allTargetSelected = targetIds.every((id) => worktree.selectedIds.includes(id));
        const selectedIds = allTargetSelected
          ? worktree.selectedIds.filter((id) => !targetSet.has(id))
          : [...worktree.selectedIds, ...targetIds.filter((id) => !worktree.selectedIds.includes(id))];
        return updateWorktreeSelection(worktree, selectedIds);
      }
      case 'prompt': return { ...worktree, prompt: { text: action.text, revision: worktree.prompt.revision + 1 } };
      case 'clearPrompt': return { ...worktree, prompt: { text: '', revision: worktree.prompt.revision + 1 } };
      case 'planGroupEdited': {
        if (!worktree.plan || !worktree.plan.groups.some((group) => group.id === action.groupId)) return worktree;
        return {
          ...worktree,
          plan: {
            ...worktree.plan,
            groups: worktree.plan.groups.map((group) => (group.id === action.groupId ? { ...group, message: action.message } : group)),
          },
          planRevision: worktree.planRevision + 1,
        };
      }
      case 'planCleared': {
        if (!worktree.plan && !worktree.requests.plan) return worktree;
        return {
          ...worktree,
          plan: undefined,
          planRevision: worktree.planRevision + 1,
          requests: { ...worktree.requests, plan: undefined },
        };
      }
      case 'tab': return { ...worktree, tab: action.tab, dockExpanded: false };
      case 'dock': return { ...worktree, dockExpanded: action.expanded };
      case 'dismissMessage': {
        const nextMessages = worktree.messages.filter((message) => message.id !== action.messageId);
        const nextReplies = { ...worktree.replies };
        delete nextReplies[action.messageId];
        const stillHasDone = nextMessages.some((message) => message.kind === 'done');
        const nextSessionStatus = worktree.sessionStatus === 'done' && !stillHasDone
          ? 'idle'
          : worktree.sessionStatus;
        return {
          ...worktree,
          messages: nextMessages,
          replies: nextReplies,
          sessionStatus: nextSessionStatus,
          customAction: nextSessionStatus === 'idle' ? undefined : worktree.customAction,
          dockExpanded: false,
        };
      }
      case 'reply': return { ...worktree, replies: { ...worktree.replies, [action.messageId]: action.text } };
    }
  }) };
}
