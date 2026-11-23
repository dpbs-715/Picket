import type { ModelConfiguration } from '../../settings/modelProviders';
import type { CPushCommandReply, CPushLandedCommit, CPushProgressEvent, CPushQaItem, CPushTimelineItem } from '../../../../shared/cpush-protocol';
import type { GitBranchReply } from '../../../../shared/git-branch-protocol';
import type { GitCommitReply } from '../../../../shared/git-commit-protocol';
import type { GitReadReply, GitReadTarget, GitRemoteBranch } from '../../../../shared/git-read-protocol';
import type { WorkbenchRestoreReply } from '../../../../shared/workbench-persistence-protocol';
import { finishCPushCommand, finishRepositoryRead, finishRepositoryBranch, finishRepositoryCommit, hydrateWorkbench, readScopeVersion, receiveCPushProgress } from './repositories';

export type Target = GitReadTarget;

export interface Change {
  id: string;
  path: string;
  previousPath?: string;
  status: 'A' | 'M' | 'D' | 'R' | 'U' | 'T' | 'C';
  gitStatus?: string;
  range: { kind: 'partial' | 'staged' | 'unstaged'; added: number; removed: number };
  ignoredForReview?: boolean;
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
  options?: { id: string; label: string; recommended?: boolean }[];
  progress?: { completed: number; total: number; label: string };
}

export interface ActiveCPushState {
  runId: string;
  frozenTarget: Target;
  frozenFileIds: string[];
  draftRevisionAtStart: number;
  modelId: string;
  timeline: CPushTimelineItem[];
  qaLog: CPushQaItem[];
  commits: CPushLandedCommit[];
  canRetryRemaining: boolean;
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
  draft: { text: string; editing: boolean; outdated: boolean; revision: number };
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
  tab: 'commit' | 'review';
  dockExpanded: boolean;
  replies: Record<string, string>;
  requests: Partial<Record<'generate' | 'review', { id: string; draftRevision: number; fileIds: string[]; previousReviewStatus?: Worktree['review']['status'] }>>;
}

export interface Repository {
  id: string;
  name: string;
  branches: { name: string; worktreeId?: string }[];
  remoteBranches?: GitRemoteBranch[];
  readSequence?: number;
}

export interface WorkbenchState {
  repositories: Repository[];
  worktrees: Worktree[];
  activeWorktreeId: string | null;
  emptyDraft: string;
  emptyEditing: boolean;
  models: ModelConfiguration[];
  activeModelId: string | null;
  editor: EditorId;
  installedEditors?: EditorId[];
  editorModified?: boolean;
  navigationRevision: number;
  repositoryBranches: Record<string, { requestId: string; navigationRevision: number }>;
  repositoryCommits: Record<string, { requestId: string; worktreeId: string; draftRevision: number; committedFileIds: string[] }>;
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

export function emptyWorkbench(): WorkbenchState {
  return { repositories: [], worktrees: [], activeWorktreeId: null, emptyDraft: '', emptyEditing: false, models: [], activeModelId: null, editor: 'idea', navigationRevision: 0, repositoryReads: {}, repositoryBranches: {}, repositoryCommits: {} };
}

export function targetOf(worktree: Worktree): Target {
  return { repositoryId: worktree.repositoryId, worktreeId: worktree.worktreeId, sessionId: worktree.sessionId, scopeVersion: worktree.scopeVersion };
}

export function selectedFiles(worktree: Worktree): Change[] {
  return worktree.files.filter((file) => worktree.selectedIds.includes(file.id));
}

export function scopeSummary(worktree?: Worktree): string {
  if (worktree?.readError) return '状态不可用';
  if (!worktree || !worktree.files.length) return '无变更';
  const selected = selectedFiles(worktree);
  const conflicts = worktree.files.filter((file) => file.status === 'U').length;
  const summary = !selected.length ? '未选择变更' : `已选 ${selected.length}/${worktree.files.length} 项${selected.some((file) => file.range.kind === 'partial') ? ' · 含部分暂存' : ''}`;
  return conflicts ? `${summary} · ${conflicts} 个冲突` : summary;
}

export function splitPath(path: string) {
  const separator = path.lastIndexOf('/');
  return { directory: path.slice(0, Math.max(separator, 0)), name: path.slice(separator + 1) };
}

export function groupFiles(files: Change[]) {
  const groups = new Map<string, Change[]>();
  for (const file of files) {
    const directory = splitPath(file.path).directory || '(根目录)';
    groups.set(directory, [...(groups.get(directory) ?? []), file]);
  }
  return [...groups].map(([directory, files]) => ({ directory, files }));
}

export const messageLabels: Record<Message['kind'], string> = {
  progress: '执行中', question: '待决定', conflict: '代码冲突', waitingManual: '待合并',
  resolved: '待推送', pushFailed: '推送失败', done: '已完成', review: '审查完成', error: '需要处理',
};
const priorities: Record<Message['kind'], number> = {
  conflict: 100, question: 95, waitingManual: 90, resolved: 85, pushFailed: 80, error: 75, progress: 60, review: 20, done: 30,
};
export function orderedMessages(messages: Message[]) {
  return messages.toSorted((a, b) => priorities[b.kind] - priorities[a.kind]);
}
export function isProtected(branch: string) { return branch === 'main' || branch === 'master'; }
export function isActiveSession(worktree: Worktree) { return worktree.sessionStatus !== 'idle' && worktree.sessionStatus !== 'done'; }

export function primaryAction(worktree: Worktree | undefined, hasModel: boolean) {
  if (worktree && isActiveSession(worktree)) {
    const labels = { running: '执行中…', needsDecision: 'C-Push · 待决定', conflict: '处理冲突', waitingManual: 'C-Push · 待决定', pushFailed: '重试推送' };
    return { label: labels[worktree.sessionStatus as keyof typeof labels], resume: true, disabled: false };
  }
  return { label: hasModel ? 'C-Push' : '提交', resume: false, disabled: !worktree || !!worktree.readError || selectedFiles(worktree).length === 0 };
}

export type WorkbenchEvent =
  | { type: 'message'; target: Target; message: Message; sessionStatus?: Worktree['sessionStatus'] }
  | { type: 'generated'; target: Target; requestId: string; text: string }
  | { type: 'reviewed'; target: Target; requestId: string; findings: Finding[]; skippedPaths: string[]; reviewedFileIds?: string[] }
  | { type: 'files'; target: Target; nextScopeVersion: string; files: Change[]; changedPaths: string[] }
  | { type: 'cPushProgress'; progress: CPushProgressEvent };

export type WorkbenchAction =
  | { type: 'workbenchHydrated'; restored: WorkbenchRestoreReply }
  | { type: 'cPushStarted'; worktreeId: string; runId: string; frozenTarget: Target; frozenFileIds: string[]; modelId: string; expandDock: boolean }
  | { type: 'cPushSettled'; reply: CPushCommandReply; expandDock?: boolean }
  | { type: 'cPushStartFailed'; worktreeId: string; runId: string }
  | { type: 'repositoryBranchStarted'; repositoryId: string; requestId: string }
  | { type: 'repositoryBranchFinished'; repositoryId: string; reply: GitBranchReply }
  | { type: 'repositoryBranchFailed'; repositoryId: string; requestId: string }
  | { type: 'repositoryCommitStarted'; repositoryId: string; worktreeId: string; requestId: string; fileIds: string[] }
  | { type: 'repositoryCommitFinished'; repositoryId: string; reply: GitCommitReply }
  | { type: 'repositoryCommitFailed'; repositoryId: string; requestId: string }
  | { type: 'repositoryReadStarted'; key: string; requestId: string }
  | { type: 'repositoryReadFinished'; key: string; reply: GitReadReply }
  | { type: 'repositoryReadFailed'; key: string; requestId: string; worktreeId?: string; error?: string }
  | { type: 'switch'; worktreeId: string; expand?: boolean }
  | { type: 'select'; worktreeId: string; fileId: string }
  | { type: 'selectAll'; worktreeId: string }
  | { type: 'selectFiles'; worktreeId: string; fileIds: string[] }
  | { type: 'draft'; worktreeId: string | null; text: string }
  | { type: 'edit' | 'clear'; worktreeId: string | null }
  | { type: 'tab'; worktreeId: string; tab: Worktree['tab'] }
  | { type: 'dock'; worktreeId: string; expanded: boolean }
  | { type: 'reply'; worktreeId: string; messageId: string; text: string }
  | { type: 'request'; target: Target; kind: 'generate' | 'review'; requestId: string; fileIds?: string[] }
  | { type: 'requestFailed'; target: Target; kind: 'generate' | 'review'; requestId: string }
  | { type: 'editor'; editor: EditorId }
  | { type: 'modelSaved'; model: ModelConfiguration }
  | { type: 'modelSelected'; modelId: string }
  | { type: 'modelDeleted'; modelId: string }
  | { type: 'modelsSynced'; models: ModelConfiguration[]; activeModelId: string | null }
  | { type: 'event'; event: WorkbenchEvent };

function updateWorktreeSelection(worktree: Worktree, selectedIds: string[]): Worktree {
  return {
    ...worktree,
    selectedIds,
    scopeVersion: readScopeVersion({ ...worktree, selectedIds }),
    draft: { ...worktree.draft, outdated: worktree.draft.outdated || !!worktree.draft.text },
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

function receiveEvent(worktree: Worktree, event: WorkbenchEvent): Worktree {
  if (event.type === 'cPushProgress') return worktree;
  if (!matchesTarget(worktree, event.target)) return worktree;
  switch (event.type) {
    case 'message':
      return { ...worktree, sessionStatus: event.sessionStatus ?? worktree.sessionStatus,
        dockExpanded: event.sessionStatus === 'done' ? false : worktree.dockExpanded,
        messages: [...worktree.messages.filter((message) => message.id !== event.message.id && (event.sessionStatus !== 'done' || message.kind === 'review')), event.message] };
    case 'generated': {
      const request = worktree.requests.generate;
      if (request?.id !== event.requestId) return worktree;
      if (request.draftRevision !== worktree.draft.revision) return { ...worktree, requests: { ...worktree.requests, generate: undefined } };
      return { ...worktree, requests: { ...worktree.requests, generate: undefined },
        draft: { text: event.text, editing: true, outdated: false, revision: worktree.draft.revision + 1 } };
    }
    case 'reviewed': {
      const request = worktree.requests.review;
      if (request?.id !== event.requestId) return worktree;
      const reviewedIds = new Set(request.fileIds);
      const preserved = worktree.review.findings.filter((finding) => !reviewedIds.has(finding.fileId) && worktree.selectedIds.includes(finding.fileId));
      const reviewedPaths = worktree.files.filter((file) => reviewedIds.has(file.id)).map((file) => file.path);
      const matchesSkipped = (entry: string, path: string) => entry === path || entry.startsWith(`${path}（`);
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
      const fileIds = new Set(event.files.map((file) => file.id));
      const selectedPaths = selectedFiles(worktree).flatMap((file) => [file.path, file.previousPath].filter((path): path is string => !!path));
      const relevant = event.nextScopeVersion !== worktree.scopeVersion || event.changedPaths.some((path) => selectedPaths.includes(path));
      const reviewedPaths = worktree.files.filter((file) => worktree.review.scopeFileIds?.includes(file.id)).map((file) => file.path);
      return { ...worktree, files: event.files, scopeVersion: event.nextScopeVersion,
        selectedIds: worktree.selectedIds.filter((id) => fileIds.has(id)),
        draft: { ...worktree.draft, outdated: worktree.draft.outdated || (relevant && !!worktree.draft.text) },
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
  if (action.type === 'cPushStarted') {
    return {
      ...state,
      worktrees: state.worktrees.map((tree) => {
        if (tree.worktreeId !== action.worktreeId || isActiveSession(tree)) return tree;
        const reviewMessages = tree.messages.filter((msg) => msg.kind === 'review');
        const progressMsg: Message = {
          id: `progress-${action.runId}`,
          kind: 'progress',
          title: '正在分析并拆分已选改动…',
          description: '已固定本次授权范围；未选文件与未暂存草稿保留在原处。',
          scope: `${action.frozenFileIds.length} 个已选文件`,
          progress: { completed: 1, total: 3, label: '读取范围 → 规划拆分 → 写入提交' },
        };
        return {
          ...tree,
          sessionStatus: 'running',
          dockExpanded: action.expandDock ? true : tree.dockExpanded,
          messages: [...reviewMessages, progressMsg],
          cPush: {
            runId: action.runId,
            frozenTarget: action.frozenTarget,
            frozenFileIds: action.frozenFileIds,
            draftRevisionAtStart: tree.draft.revision,
            modelId: action.modelId,
            timeline: [
              { state: 'done', label: `已固定 ${action.frozenFileIds.length} 个已选文件范围` },
              { state: 'active', label: '正在分析改动归属并规划拆分提交…' },
            ],
            qaLog: [],
            commits: [],
            canRetryRemaining: false,
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
  if (action.type === 'repositoryCommitStarted') {
    const tree = state.worktrees.find((item) => item.worktreeId === action.worktreeId);
    return { ...state, repositoryCommits: { ...state.repositoryCommits, [action.repositoryId]: { requestId: action.requestId, worktreeId: action.worktreeId, draftRevision: tree?.draft.revision ?? 0, committedFileIds: action.fileIds } } };
  }
  if (action.type === 'repositoryCommitFinished') return finishRepositoryCommit(state, action.repositoryId, action.reply);
  if (action.type === 'repositoryCommitFailed') {
    if (state.repositoryCommits?.[action.repositoryId]?.requestId !== action.requestId) return state;
    const repositoryCommits = { ...state.repositoryCommits }; delete repositoryCommits[action.repositoryId];
    return { ...state, repositoryCommits };
  }
  if (action.type === 'repositoryReadStarted') {
    const navigationRevision = state.navigationRevision + (action.key === 'open' ? 1 : 0);
    return { ...state, navigationRevision, repositoryReads: { ...state.repositoryReads, [action.key]: { requestId: action.requestId, navigationRevision } } };
  }
  if (action.type === 'repositoryReadFinished') return finishRepositoryRead(state, action.key, action.reply);
  if (action.type === 'repositoryReadFailed') {
    if (state.repositoryReads[action.key]?.requestId !== action.requestId) return state;
    const repositoryReads = { ...state.repositoryReads }; delete repositoryReads[action.key];
    return { ...state, repositoryReads, worktrees: state.worktrees.map((tree) => tree.worktreeId === action.worktreeId
      ? { ...tree, readError: action.error ?? '状态读取失败，请重新刷新；草稿与选择已保留。',
        review: { ...tree.review, status: tree.review.status === 'running' ? tree.requests.review?.previousReviewStatus ?? 'idle' : tree.review.status }, requests: {} } : tree) };
  }
  if (action.type === 'editor') return { ...state, editor: action.editor, editorModified: true };
  if (action.type === 'modelsSynced') return { ...state, models: action.models, activeModelId: action.activeModelId };
  if (action.type === 'modelSaved') {
    const exists = state.models.some((model) => model.id === action.model.id);
    return exists
      ? {
          ...state,
          models: state.models.map((model) => (model.id === action.model.id ? action.model : model)),
          activeModelId: state.activeModelId ?? action.model.id,
        }
      : {
          ...state,
          models: [...state.models, action.model],
          activeModelId: action.model.id,
        };
  }
  if (action.type === 'modelSelected') return state.models.some((model) => model.id === action.modelId) ? { ...state, activeModelId: action.modelId } : state;
  if (action.type === 'modelDeleted') {
    const models = state.models.filter((model) => model.id !== action.modelId);
    return { ...state, models, activeModelId: state.activeModelId === action.modelId ? models[0]?.id ?? null : state.activeModelId };
  }
  if (action.type === 'switch') {
    if (!state.worktrees.some((worktree) => worktree.worktreeId === action.worktreeId)) return state;
    return { ...state, activeWorktreeId: action.worktreeId, navigationRevision: state.navigationRevision + 1, worktrees: state.worktrees.map((worktree) => worktree.worktreeId === action.worktreeId && action.expand
      ? { ...worktree, dockExpanded: true } : worktree) };
  }
  if ((action.type === 'draft' || action.type === 'edit' || action.type === 'clear') && action.worktreeId === null) {
    return { ...state, emptyDraft: action.type === 'draft' ? action.text : action.type === 'clear' ? '' : state.emptyDraft,
      emptyEditing: action.type !== 'clear' };
  }
  return { ...state, worktrees: state.worktrees.map((worktree) => {
    if (action.type === 'event') return receiveEvent(worktree, action.event);
    if (action.type === 'request' || action.type === 'requestFailed') {
      if (!matchesTarget(worktree, action.target)) return worktree;
      if (action.type === 'requestFailed') {
        if (worktree.requests[action.kind]?.id !== action.requestId) return worktree;
        return { ...worktree, requests: { ...worktree.requests, [action.kind]: undefined },
          review: action.kind === 'review' ? { ...worktree.review, status: worktree.requests.review?.previousReviewStatus ?? 'idle' } : worktree.review };
      }
      return { ...worktree, requests: { ...worktree.requests, [action.kind]: { id: action.requestId, draftRevision: worktree.draft.revision, fileIds: action.fileIds ?? worktree.selectedIds,
        previousReviewStatus: worktree.requests.review?.previousReviewStatus ?? worktree.review.status } },
        review: action.kind === 'review' ? { ...worktree.review, status: 'running' } : worktree.review };
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
        const allSelected = worktree.files.every((file) => worktree.selectedIds.includes(file.id));
        const selectedIds = allSelected ? [] : worktree.files.map((file) => file.id);
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
      case 'draft': return { ...worktree, draft: { ...worktree.draft, text: action.text, editing: true, revision: worktree.draft.revision + 1 } };
      case 'edit': return { ...worktree, draft: { ...worktree.draft, editing: true } };
      case 'clear': return { ...worktree, draft: { text: '', editing: false, outdated: false, revision: worktree.draft.revision + 1 } };
      case 'tab': return { ...worktree, tab: action.tab, dockExpanded: false };
      case 'dock': return { ...worktree, dockExpanded: action.expanded };
      case 'reply': return { ...worktree, replies: { ...worktree.replies, [action.messageId]: action.text } };
    }
  }) };
}
