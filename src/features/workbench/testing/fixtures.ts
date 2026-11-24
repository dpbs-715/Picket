import { emptyWorkbench } from '../state/workbench';
import type { Change, Finding, Message, WorkbenchState, Worktree } from '../state/workbench';

export const scenarios = ['empty', 'compact', 'running', 'generating', 'question', 'typing', 'done', 'review-during-question', 'push-failed', 'conflict', 'manual', 'resolved', 'terminal-done', 'no-model', 'single', 'clean', 'unselected', 'changed', 'remote-error', 'long-paths'] as const;
export type Scenario = typeof scenarios[number];

export function sampleFiles(): Change[] {
  return [
    { id: 'order', path: 'src/service/Order.ts', status: 'M', range: { kind: 'partial', added: 28, removed: 4 } },
    { id: 'pay', path: 'src/api/pay.ts', status: 'M', range: { kind: 'unstaged', added: 15, removed: 2 } },
    { id: 'retry', path: 'src/utils/retry.ts', status: 'A', range: { kind: 'unstaged', added: 68, removed: 0 } },
    { id: 'release', path: 'RELEASE_NOTES.md', status: 'M', range: { kind: 'unstaged', added: 18, removed: 2 } },
    { id: 'lock', path: 'pnpm-lock.yaml', status: 'M', range: { kind: 'unstaged', added: 840, removed: 9 }, ignoredForReview: true },
  ];
}

export function sampleFindings(): Finding[] {
  return [
    {
      id: 'finding-pay',
      fileId: 'pay',
      path: 'src/api/pay.ts',
      line: 46,
      severity: 'high',
      title: 'Signature failure log prints full request body',
      reason: 'Payload may include channel secrets or credentials; log only traceId and orderId.',
    },
    {
      id: 'finding-order',
      fileId: 'order',
      path: 'src/service/Order.ts',
      line: 82,
      severity: 'suggestion',
      title: 'Extract idempotency lock TTL to a named constant',
      reason: 'Consolidate the hardcoded 300s value into PAY_CALLBACK_LOCK_TTL_SEC.',
    },
  ];
}

export function releaseQuestion(): Message {
  return {
    id: 'release-question',
    kind: 'question',
    title: 'Include release notes in this commit?',
    scope: 'Affects commit 2/2 "docs"',
    description: 'Payment callback fix and retry helper are grouped into commit 1 (preserving staged Order.ts hunks). Confirm how to handle RELEASE_NOTES.md:',
    files: [
      { id: 'release', label: 'RELEASE_NOTES.md', badge: 'Confirm' },
      { id: 'order', label: 'Order.ts', badge: 'Staged only' },
    ],
    options: [
      { id: 'merge', label: 'Merge into commit 1' },
      { id: 'split', label: 'Commit separately & push together', recommended: true },
      { id: 'skip', label: 'Skip for now' },
    ],
  };
}

export function sampleWorktree(): Worktree {
  return {
    repositoryId: 'orders',
    worktreeId: 'pay-tree',
    sessionId: 'pay-session',
    scopeVersion: 'scope-1',
    label: 'wt/pay-retry',
    path: '/test-only/order-pay',
    branch: 'feat/pay',
    remote: { kind: 'tracked', branch: 'upstream/feat/pay', ahead: 2, behind: 1 },
    files: sampleFiles(),
    selectedIds: ['order', 'pay', 'retry', 'release'],
    unselectedIds: ['lock'],
    prompt: {
      text: 'split release notes into its own commit and write detailed bullets',
      revision: 0,
    },
    planRevision: 0,
    review: { status: 'idle', findings: [], skippedPaths: [], changedPaths: [] },
    sessionStatus: 'idle',
    messages: [],
    tab: 'commit',
    dockExpanded: false,
    replies: {},
    requests: {},
  };
}

export function fixture(scenario: Scenario = 'typing'): WorkbenchState {
  if (scenario === 'empty') return emptyWorkbench();
  const tree = sampleWorktree();
  const state: WorkbenchState = {
    ...emptyWorkbench(),
    repositorySettings: {
      orders: { editor: 'idea', commitLanguage: 'en' },
      cli: { editor: 'idea', commitLanguage: 'en' },
    },
    repositories: [
      {
        id: 'orders',
        name: 'my-order-service',
        branches: [
          { name: 'feat/pay', worktreeId: 'pay-tree' },
          { name: 'fix/npe', worktreeId: 'fix-tree' },
          { name: 'main', worktreeId: 'main-tree' },
          { name: 'feat/invoice' },
        ],
        remoteBranches: [
          { name: 'upstream/main', remote: 'upstream', localName: 'main' },
          { name: 'upstream/feat/pay', remote: 'upstream', localName: 'feat/pay' },
          { name: 'upstream/feat/reconcile', remote: 'upstream', localName: 'feat/reconcile' },
        ],
      },
      {
        id: 'cli',
        name: 'picket-cli',
        branches: [{ name: 'main', worktreeId: 'cli-tree' }],
        remoteBranches: [
          { name: 'origin/main', remote: 'origin', localName: 'main' },
          { name: 'origin/release/v1', remote: 'origin', localName: 'release/v1' },
        ],
      },
    ],
    worktrees: [
      tree,
      {
        ...sampleWorktree(),
        worktreeId: 'fix-tree',
        sessionId: 'fix-session',
        label: 'wt/fix-npe',
        branch: 'fix/npe',
        path: '/test-only/order-fix',
        files: [],
        selectedIds: [],
        unselectedIds: [],
        prompt: { text: 'write concise bullets', revision: 0 },
      },
      {
        ...sampleWorktree(),
        worktreeId: 'main-tree',
        sessionId: 'main-session',
        label: 'main (primary)',
        branch: 'main',
        path: '/test-only/orders',
        files: [],
        selectedIds: [],
        unselectedIds: [],
      },
      {
        ...sampleWorktree(),
        repositoryId: 'cli',
        worktreeId: 'cli-tree',
        sessionId: 'cli-session',
        label: 'main',
        branch: 'main',
        path: '/test-only/cli',
        remote: { kind: 'missing', reason: 'No remote tracking branch configured' },
      },
    ],
    activeWorktreeId: tree.worktreeId,
    models: [
      { id: 'test-claude', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' },
      { id: 'test-gpt', name: 'GPT-4.1', provider: 'openai', modelId: 'gpt-4.1' },
    ],
    activeModelId: 'test-claude',
  };
  const commits = [
    { hash: 'a19e7c2', title: 'feat(order): add payment callback idempotency lock and backoff retry' },
    { hash: 'b84d120', title: 'docs: update release notes' },
  ];
  const conflictFiles = [{ id: 'order', label: 'Order.ts', badge: 'Conflict' }];
  switch (scenario) {
    case 'compact':
      state.windowPrefs = {
        ...state.windowPrefs,
        pinned: true,
        compactMode: true,
        pinnedExpanded: false,
      };
      break;
    case 'typing':
      tree.plan = {
        id: 'plan-ready',
        baseScopeVersion: tree.scopeVersion,
        modelId: 'test-claude',
        summary: 'Prepared 1 commit',
        groups: [{
          id: 'group-1',
          fileIds: ['order', 'pay', 'retry', 'release'],
          message: 'feat(order): add payment callback idempotency lock and backoff retry\n\n- keep staged Order.ts changes\n- add exponential backoff helper and release notes',
        }],
        skippedFileIds: [],
      };
      break;
    case 'running':
      tree.sessionStatus = 'running';
      tree.dockExpanded = true;
      tree.messages = [{
        id: 'progress',
        kind: 'progress',
        title: 'Analyzing and splitting selected changes',
        description: 'Scope frozen; unselected lockfile and unstaged drafts remain untouched.',
        scope: '4 files · staged only +28 -4',
        progress: { completed: 1, total: 4, label: 'Read scope -> Plan -> Commit -> Push' },
      }];
      break;
    case 'generating':
      tree.prompt = { text: '', revision: 0 };
      tree.requests = { plan: { id: 'plan-demo', promptRevision: 0, planRevision: 0, fileIds: tree.selectedIds } };
      tree.sessionStatus = 'done';
      tree.dockExpanded = false;
      tree.messages = [{
        id: 'done',
        kind: 'done',
        title: '2 commits pushed to upstream/feat/pay',
        description: 'Selected scope completed. Unselected lockfile and unstaged Order.ts draft remain untouched.',
        commits,
      }];
      break;
    case 'question':
    case 'review-during-question':
      tree.sessionStatus = 'needsDecision';
      tree.messages = [releaseQuestion()];
      tree.dockExpanded = scenario === 'question';
      if (scenario === 'review-during-question') {
        tree.review = {
          status: 'done',
          findings: sampleFindings(),
          skippedPaths: ['pnpm-lock.yaml'],
          changedPaths: [],
          scopeFileIds: ['order', 'pay', 'retry', 'release'],
        };
        tree.messages.push({
          id: 'review-notice',
          kind: 'review',
          title: 'Review complete: 1 high-risk, 1 suggestion',
          description: 'Review findings updated.',
        });
      }
      break;
    case 'done':
    case 'terminal-done':
      tree.sessionStatus = 'done';
      tree.dockExpanded = scenario !== 'terminal-done';
      tree.messages = [{
        id: 'done',
        kind: 'done',
        title: '2 commits pushed to upstream/feat/pay',
        description: 'Selected scope completed. Unselected lockfile and unstaged Order.ts draft remain untouched.',
        commits,
      }];
      break;
    case 'push-failed':
      tree.sessionStatus = 'pushFailed';
      tree.dockExpanded = true;
      tree.messages = [{
        id: 'push-failed',
        kind: 'pushFailed',
        title: 'Committed locally, push failed',
        description: 'Unable to reach remote. Local commits are preserved; retrying only pushes existing commits.',
        scope: 'upstream/feat/pay',
        commits,
        options: [{ id: 'retry-push', label: 'Retry Push', recommended: true }],
      }];
      break;
    case 'conflict':
      tree.sessionStatus = 'conflict';
      tree.files[0] = { ...tree.files[0], status: 'U' };
      tree.dockExpanded = true;
      tree.messages = [{
        id: 'conflict',
        kind: 'conflict',
        title: 'Merge conflict encountered while syncing remote',
        description: 'Rebase paused. Agent only modifies the conflicted files below after your authorization.',
        scope: 'Rebase · 1 conflicted file',
        files: conflictFiles,
        options: [
          { id: 'agent-merge', label: 'Let Agent Merge', recommended: true },
          { id: 'manual-merge', label: 'Merge Manually' },
        ],
      }];
      break;
    case 'manual':
      tree.sessionStatus = 'waitingManual';
      tree.dockExpanded = true;
      tree.messages = [{
        id: 'manual',
        kind: 'waitingManual',
        title: 'Waiting for manual merge in IDE',
        description: 'Write operations paused while preserving staged and unstaged changes. Git status is verified on return.',
        files: conflictFiles,
      }];
      break;
    case 'resolved':
      tree.sessionStatus = 'needsDecision';
      tree.dockExpanded = true;
      tree.messages = [{
        id: 'resolved',
        kind: 'resolved',
        title: 'Conflict resolved. Continue pushing?',
        description: 'Local commits are preserved and not yet pushed to remote.',
        commits,
        options: [{ id: 'continue-push', label: 'Continue Push', recommended: true }],
      }];
      break;
    case 'no-model':
      state.models = [];
      state.activeModelId = null;
      tree.prompt = { text: '', revision: 0 };
      break;
    case 'single':
      state.activeWorktreeId = 'cli-tree';
      break;
    case 'clean':
      tree.files = [];
      tree.selectedIds = [];
      tree.unselectedIds = [];
      break;
    case 'unselected':
      tree.selectedIds = [];
      tree.unselectedIds = tree.files.map((file) => file.id);
      break;
    case 'changed':
      tree.plan = {
        id: 'plan-stale',
        baseScopeVersion: 'scope-0',
        modelId: 'test-claude',
        summary: 'Planned 2 commits',
        groups: [
          {
            id: 'group-1',
            fileIds: ['order', 'pay', 'retry'],
            message: 'feat(order): add payment callback idempotency lock and backoff retry\n\n- keep staged Order.ts changes\n- add exponential backoff helper',
          },
          {
            id: 'group-2',
            fileIds: ['release'],
            message: 'docs: update release notes\n\n- document payment retry behavior',
          },
        ],
        skippedFileIds: [],
      };
      tree.review = {
        status: 'done',
        findings: sampleFindings(),
        skippedPaths: ['pnpm-lock.yaml'],
        changedPaths: ['src/service/Order.ts'],
        scopeFileIds: ['order', 'pay'],
      };
      break;
    case 'remote-error':
      tree.remote = { kind: 'error', reason: 'Remote status read failed; ahead/behind counts unavailable' };
      break;
    case 'long-paths':
      tree.files = [
        ...tree.files,
        ...Array.from({ length: 30 }, (_, index): Change => ({
          id: `long-${index}`,
          path: `src/\u5305\u542b\u4e2d\u6587\u548c\u7a7a\u683c\u7684\u957f\u76ee\u5f55/\u591a\u7ea7\u76ee\u5f55/\u6781\u957f\u6587\u4ef6\u540d-with-a-very-long-name-${index}.tsx`,
          previousPath: `src/old-name-${index}.tsx`,
          status: 'R',
          range: { kind: 'staged', added: 120, removed: 80 },
        })),
      ];
      break;
  }
  return state;
}
