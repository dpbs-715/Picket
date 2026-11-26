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
import type {
  CustomActionCommandReply,
  CustomActionConfig,
  CustomActionLink,
  CustomActionProgressEvent,
  CustomActionTimelineItem,
} from '../../../../shared/custom-action-protocol';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitLocalKeepReply, GitReadReply, GitReadTarget, GitRemoteBranch } from '../../../../shared/git-read-protocol';
import type { AppLocale, CommitLanguage } from '../../../../shared/language-protocol';
import type {
  PersistedWindowPreferences,
  WindowPosition,
  WindowSize,
  WorkbenchRestoreReply,
} from '../../../../shared/workbench-persistence-protocol';

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
  skipPush?: boolean;
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

export type CompactExpandTarget = 'changes' | 'dock' | 'plan' | 'repo';
export type CompactStatusTone = 'muted' | 'ready' | 'running' | 'warning' | 'danger' | 'success';

export interface CompactStatusSummary {
  tone: CompactStatusTone;
  label: string;
  expandTarget?: CompactExpandTarget;
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
