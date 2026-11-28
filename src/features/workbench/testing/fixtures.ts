import { emptyWorkbench } from '../state/workbench';
import type { Change, Message, WorkbenchState, Worktree } from '../state/workbench';

export const scenarios = ['empty', 'compact', 'running', 'question', 'typing', 'done', 'rich-card', 'push-failed', 'conflict', 'manual', 'resolved', 'terminal-done', 'no-model', 'single', 'clean', 'unselected', 'remote-error', 'long-paths'] as const;
export type Scenario = typeof scenarios[number];

export function sampleFiles(): Change[] {
  return [
    { id: 'order', path: 'src/service/Order.ts', status: 'M', range: { kind: 'partial', added: 28, removed: 4 } },
    { id: 'pay', path: 'src/api/pay.ts', status: 'M', range: { kind: 'unstaged', added: 15, removed: 2 } },
    { id: 'retry', path: 'src/utils/retry.ts', status: 'A', range: { kind: 'unstaged', added: 68, removed: 0 } },
    { id: 'release', path: 'RELEASE_NOTES.md', status: 'M', range: { kind: 'unstaged', added: 18, removed: 2 } },
    { id: 'lock', path: 'pnpm-lock.yaml', status: 'M', range: { kind: 'unstaged', added: 840, removed: 9 } },
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

export function sampleRichCardMessage(): Message {
  return {
    id: 'done-action-review-1',
    kind: 'done',
    title: 'Code review complete',
    description: 'Found 1 high-risk issue and 1 suggestion.',
    links: [{ label: 'PR #128', url: 'https://github.com/acme/orders/pull/128' }],
    card: {
      tone: 'warning',
      title: 'Code review complete',
      subtitle: 'feat/pay · 4 files inspected',
      blocks: [
        {
          type: 'facts',
          items: [
            { label: 'Scope', value: '4 files', mono: true },
            { label: 'High Risk', value: '1', tone: 'danger' },
            { label: 'Suggestions', value: '1', tone: 'warning' },
            { label: 'Skipped', value: '1 lockfile' },
          ],
        },
        {
          type: 'findings',
          items: [
            {
              severity: 'high',
              tag: 'P1',
              title: 'Signature failure log prints full request body',
              detail: 'Payload may include channel secrets or credentials; log only traceId and orderId.',
              path: 'src/api/pay.ts',
              line: 46,
            },
            {
              severity: 'low',
              tag: 'P2',
              title: 'Extract idempotency lock TTL to a named constant',
              detail: 'Consolidate the hardcoded 300s value into PAY_CALLBACK_LOCK_TTL_SEC.',
              path: 'src/service/Order.ts',
              line: 82,
            },
          ],
          skipped: [{ path: 'pnpm-lock.yaml', reason: 'Lockfile skipped by default' }],
        },
        {
          type: 'checklist',
          items: [
            { status: 'pass', label: 'TypeScript check', detail: '0 errors' },
            { status: 'warn', label: 'Secret redaction check', detail: '1 warning in pay.ts' },
          ],
        },
        {
          type: 'code',
          title: 'diff excerpt',
          content: 'line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10',
        },
        {
          type: 'files',
          items: [{ path: 'src/api/pay.ts', line: 46, badge: 'M' }],
        },
      ],
    },
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
    sessionStatus: 'idle',
    messages: [],
    dockExpanded: false,
    replies: {},
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
    connections: [
      { id: 'conn-anthropic', name: 'Anthropic', provider: 'anthropic', keyHint: 'sk-a…1024' },
      { id: 'conn-openai', name: 'OpenAI', provider: 'openai', keyHint: 'sk-p…8842' },
    ],
    models: [
      { id: 'test-claude', connectionId: 'conn-anthropic', name: 'Claude Sonnet 4', provider: 'anthropic', modelId: 'claude-sonnet-4-20250514' },
      { id: 'test-gpt', connectionId: 'conn-openai', name: 'GPT-4.1', provider: 'openai', modelId: 'gpt-4.1' },
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
    case 'question':
      tree.sessionStatus = 'needsDecision';
      tree.messages = [releaseQuestion()];
      tree.dockExpanded = true;
      break;
    case 'rich-card':
      tree.sessionStatus = 'done';
      tree.dockExpanded = true;
      tree.activeRun = {
        profile: 'customAction',
        runId: 'run-review-1',
        actionId: 'act-review',
        actionLabel: 'Review Diff',
        frozenTarget: {
          repositoryId: tree.repositoryId,
          worktreeId: tree.worktreeId,
          sessionId: tree.sessionId,
          scopeVersion: tree.scopeVersion,
        },
        frozenFileIds: [],
        promptRevisionAtStart: 0,
        modelId: 'test-claude',
        timeline: [{ state: 'done', label: 'Inspected 4 files' }],
        qaLog: [],
        commits: [],
        links: [{ label: 'PR #128', url: 'https://github.com/acme/orders/pull/128' }],
        canRetryRemaining: false,
      };
      tree.messages = [sampleRichCardMessage()];
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
      state.connections = [];
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
