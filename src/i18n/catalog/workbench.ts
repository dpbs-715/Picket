import type { GitOpenFileAction } from '../../../shared/ide-protocol';

export interface WorkbenchMessages {
  header: {
    opening: string;
    openRepository: string;
    repositoriesTitle: string;
    singleWorktree: string;
    worktreeCount(count: number): string;
    noRepositoriesOpened: string;
    openLocalRepository: string;
    sessionHistory: string;
    byWorktree: string;
    noSessionHistory: string;
    idle: string;
    noActiveSession: string;
    collapseWindow: string;
    pinWindow: string;
    unpinWindow: string;
    settings: string;
    noBranchSelected: string;
    branchesTitle: string;
    localBranches: string;
    remoteBranches: string;
    worktrees: string;
    jumpToWorktree(label: string): string;
    noLocalBranches: string;
    switchAction: string;
    checkoutAction: string;
    noRemoteBranches: string;
    worktreeUnavailable: string;
    worktreeChangesCount(count: number): string;
    worktreeClean: string;
    remoteDetails: string;
    untracked: string;
    trackedRemoteBranch: string;
    remoteCountTipLabel: string;
    remoteCountTipText: string;
    aheadBehind(ahead: number, behind: number): string;
    syncing: string;
    pullAndRebase: string;
  };
  changes: {
    panelLabel: string;
    selectAll: string;
    deselectAll: string;
    statusUnavailable: string;
    noChanges: string;
    noneSelected: string;
    selectedSummary(selected: number, total: number, hasPartial: boolean, conflicts: number): string;
    switchToTreeView: string;
    switchToListView: string;
    refreshChanges: string;
    selectFile(path: string): string;
    gitStatus(status: string): string;
    ideActions: {
      merge: string;
      stagedDiff: string;
      open: string;
      diff: string;
    };
    conflictBadge: string;
    partialStagedBadge: string;
    skipReviewBadge: string;
    findingCountLabel(path: string, count: number): string;
    emptyRepoTitle: string;
    browserPreviewHint: string;
    openingRepo: string;
    openLocalRepository: string;
    worktreeUnavailableTitle: string;
    retryRead: string;
    workingTreeClean: string;
    rootDirectory: string;
    selectDirectory(directory: string): string;
    deselectDirectory(directory: string): string;
    localKeepTitle: string;
    localKeepBadge: string;
    localKeepConflicts(count: number): string;
    localKeepEmptyHint: string;
    noNormalChanges: string;
    keepLocallyAction: string;
    moveBackToChangesAction: string;
    dropToLocalKeepHint: string;
    dropToChangesHint: string;
    localKeepSelectedNotice(count: number): string;
    uncheckLocalKeepAction: string;
    fileActionsMenuLabel(path: string): string;
    batchFileActionsMenuLabel(count: number): string;
  };
  dock: {
    expandMessage(title: string): string;
    activeRegionLabel: string;
    collapseMessage: string;
    taskProgressLabel: string;
    timelineLabel: string;
    qaLogLabel: string;
    qaEntry(question: string, answer: string): string;
    viewReview: string;
    openConflictInEditor(editorShort: string): string;
    gotIt: string;
    dismissDone: string;
    recommendedBadge: string;
    multiSelectHint: string;
    showPreview: string;
    hidePreview: string;
    submitAnswers: string;
    cancelQuestion: string;
    dontAskAgainDefault: string;
    customOptionPlaceholder: string;
    replyLabel: string;
    conflictReplyPlaceholder: string;
    questionReplyPlaceholder: string;
    sendReply: string;
    cancelAction: string;
  };
  footer: {
    addModel: string;
    selectModelTitle: string;
    manageModelsMenuItem: string;
    switchActionTitle: string;
    switchActionAriaLabel: string;
    defaultCPushSubtitle: string;
    newActionMenuItem: string;
    createWithPiMenuItem: string;
    manageActionsMenuItem: string;
  };
  compact: {
    expandWindow: string;
    barLabel: string;
    switchProject: string;
    projectDrawerLabel: string;
    filesDrawerLabel: string;
    actionsDrawerLabel: string;
    selectedTrigger(selected: number, total: number): string;
    noModelBadge: string;
    planBadge(count: number, outdated: boolean): string;
    status: {
      noRepo: string;
      clean: string;
      readError: string;
      detachedHead: string;
      noneSelected(total: number): string;
      selectedNoModel(selected: number, total: number): string;
      selectedReady(selected: number, total: number): string;
      planReady(count: number): string;
      planOutdated(count: number): string;
      emptyPlanMessage: string;
      planning: string;
      committingStep(completed: number, total: number): string;
      running: string;
      needsDecision: string;
      waitingManual: string;
      pushFailed: string;
      conflict: string;
    };
    notifications: {
      needsDecisionTitle: string;
      pushFailedTitle: string;
      conflictTitle: string;
      fallbackBody(branch: string): string;
    };
  };
  workbench: {
    ariaLabel: string;
    lowerWorkspaceLabel: string;
    workspaceTabsLabel: string;
    loadStateFailedFallback: string;
    saveStateFailedFallback: string;
    readPinFailedFallback: string;
    setPinFailedFallback: string;
    openModelSettingsAction: string;
    reviewNoRisksSummary: string;
    reviewFindingsSummary(high: number, suggestion: number): string;
    reviewCompleteTitle(summary: string): string;
    reviewCompleteDescription: string;
    pullRebaseTrackedDone(branch: string): string;
    pullRebaseDone: string;
    openFileMismatchError: string;
    openedInEditor(shortName: string, action: GitOpenFileAction, fileName: string, line?: number): string;
    branchMismatchError: string;
    remoteBranchCheckedOut(landedBranch: string, remoteBranch: string): string;
    branchSwitched(landedBranch: string): string;
    readMismatchError: string;
    genericOperationFailed: string;
    noChangedFilesToReReview: string;
    cancelledPlan: string;
    cancelledReview: string;
    detachedPrimaryBlocked: string;
    readFailedFallback: string;
    missingWorktreeError: string;
    cPushInitialTitle: string;
    cPushInitialDescription: string;
    cPushInitialScope(count: number): string;
    cPushInitialProgressLabel: string;
    cPushTimelineFrozen(count: number): string;
    cPushTimelinePlanning: string;
  };
}

export const workbenchEn: WorkbenchMessages = {
  header: {
    opening: 'Opening…',
    openRepository: 'Open Repository',
    repositoriesTitle: 'Repositories',
    singleWorktree: 'Single tree',
    worktreeCount: (count) => `${count} worktrees`,
    noRepositoriesOpened: 'No repository opened yet',
    openLocalRepository: 'Open Local Repository…',
    sessionHistory: 'C-Push Session History',
    byWorktree: 'By worktree',
    noSessionHistory: 'No session history',
    idle: 'Idle',
    noActiveSession: 'No active session',
    collapseWindow: 'Collapse to Compact Mode',
    pinWindow: 'Pin Window',
    unpinWindow: 'Unpin Window',
    settings: 'Settings',
    noBranchSelected: 'No branch',
    branchesTitle: 'Branches',
    localBranches: 'Local Branches',
    remoteBranches: 'Remote Branches',
    worktrees: 'Worktrees',
    jumpToWorktree: (label) => `Jump to ${label}`,
    noLocalBranches: 'No local branches',
    switchAction: 'Switch',
    checkoutAction: 'Checkout',
    noRemoteBranches: 'No remote tracking branches',
    worktreeUnavailable: 'Unavailable',
    worktreeChangesCount: (count) => `${count} changed`,
    worktreeClean: 'Clean',
    remoteDetails: 'Remote Details',
    untracked: 'Untracked',
    trackedRemoteBranch: 'Tracked Remote Branch',
    remoteCountTipLabel: 'Remote count info',
    remoteCountTipText: 'Based on local tracking refs only (not fetched over network yet). Click "Pull & Rebase" to sync with remote.',
    aheadBehind: (ahead, behind) => `${ahead} ahead · ${behind} behind`,
    syncing: 'Syncing…',
    pullAndRebase: 'Pull & Rebase',
  },
  changes: {
    panelLabel: 'Changed Files',
    selectAll: 'Select all changes',
    deselectAll: 'Deselect all changes',
    statusUnavailable: 'Status unavailable',
    noChanges: 'No changes',
    noneSelected: 'No changes selected',
    selectedSummary: (selected, total, hasPartial, conflicts) => {
      const base = `${selected}/${total} selected${hasPartial ? ' · partial staged' : ''}`;
      return conflicts ? `${base} · ${conflicts} conflict${conflicts === 1 ? '' : 's'}` : base;
    },
    switchToTreeView: 'Switch to directory tree view',
    switchToListView: 'Switch to list view',
    refreshChanges: 'Refresh Changes',
    selectFile: (path) => `Select ${path}`,
    gitStatus: (status) => `Git status ${status}`,
    ideActions: {
      merge: 'Merge',
      stagedDiff: 'Staged Diff',
      open: 'Open',
      diff: 'Diff',
    },
    conflictBadge: '· Conflict',
    partialStagedBadge: '· Partial staged',
    skipReviewBadge: '· Skip review',
    findingCountLabel: (path, count) => `${path}: ${count} review finding${count === 1 ? '' : 's'}`,
    emptyRepoTitle: 'Open Your Git Repository',
    browserPreviewHint: 'Browser preview cannot read local directories',
    openingRepo: 'Opening…',
    openLocalRepository: 'Open Local Repository…',
    worktreeUnavailableTitle: 'Worktree Status Unavailable',
    retryRead: 'Retry Read',
    workingTreeClean: 'Working Tree Clean',
    rootDirectory: '(root)',
    selectDirectory: (directory) => `Select directory ${directory}`,
    deselectDirectory: (directory) => `Deselect directory ${directory}`,
    localKeepTitle: 'Local Keep',
    localKeepBadge: '· Local keep',
    localKeepConflicts: (count) => `${count} conflict${count === 1 ? '' : 's'}`,
    localKeepEmptyHint: 'Drag files here or right-click a file to keep locally without committing',
    noNormalChanges: 'No changes outside Local Keep',
    keepLocallyAction: 'Keep Locally (Do Not Commit)',
    moveBackToChangesAction: 'Move Back to Changes',
    dropToLocalKeepHint: 'Drop to keep locally',
    dropToChangesHint: 'Drop to move back to changes',
    localKeepSelectedNotice: (count) =>
      `${count} Local Keep file${count === 1 ? ' is' : 's are'} checked and will be included`,
    uncheckLocalKeepAction: 'Uncheck',
    fileActionsMenuLabel: (path) => `Actions for ${path}`,
    batchFileActionsMenuLabel: (count) => `Actions for ${count} selected files`,
  },
  dock: {
    expandMessage: (title) => `Expand message: ${title}`,
    activeRegionLabel: 'Active Session',
    collapseMessage: 'Collapse Message',
    taskProgressLabel: 'Task progress',
    timelineLabel: 'Session execution steps',
    qaLogLabel: 'Session Q&A log',
    qaEntry: (question, answer) => `Q&A: ${question} → ${answer}`,
    viewReview: 'View',
    openConflictInEditor: (editorShort) => `Open conflicted file in ${editorShort}`,
    gotIt: 'Got it',
    dismissDone: 'Dismiss completed notice',
    recommendedBadge: 'Recommended',
    multiSelectHint: 'Select one or more',
    showPreview: 'Preview',
    hidePreview: 'Hide preview',
    submitAnswers: 'Submit',
    cancelQuestion: 'Cancel',
    dontAskAgainDefault: 'Do not ask again for this action',
    customOptionPlaceholder: 'Or enter a custom answer…',
    replyLabel: 'Reply to active message',
    conflictReplyPlaceholder: 'Enter merge instructions and press Enter…',
    questionReplyPlaceholder: 'Or enter custom instructions and press Enter…',
    sendReply: 'Send Reply',
    cancelAction: 'Cancel',
  },
  footer: {
    addModel: 'Add Model',
    selectModelTitle: 'Select Model',
    manageModelsMenuItem: 'Manage Models…',
    switchActionTitle: 'Switch Action',
    switchActionAriaLabel: 'Switch primary action',
    defaultCPushSubtitle: 'Default smart split commit & push',
    newActionMenuItem: 'New Action…',
    createWithPiMenuItem: 'Create with Picket…',
    manageActionsMenuItem: 'Manage Actions…',
  },
  compact: {
    expandWindow: 'Expand Workbench',
    barLabel: 'Compact Workbench',
    switchProject: 'Switch project',
    projectDrawerLabel: 'Opened repositories',
    filesDrawerLabel: 'Changed files',
    actionsDrawerLabel: 'Switch primary action',
    selectedTrigger: (selected, total) => `Selected ${selected}/${total}`,
    noModelBadge: 'No model',
    planBadge: (count, outdated) => (outdated ? `Plan ${count} · Outdated ▸` : `Plan ${count} ▸`),
    status: {
      noRepo: 'Open a repository ▸',
      clean: 'Working tree clean',
      readError: 'Status unavailable ▸',
      detachedHead: 'Detached HEAD ▸',
      noneSelected: (total) => `0/${total} selected ▸`,
      selectedNoModel: (selected, total) => `Selected ${selected}/${total} · No model`,
      selectedReady: (selected, total) => `Selected ${selected}/${total} · Ready`,
      planReady: (count) => `Plan: ${count} commit${count === 1 ? '' : 's'} ▸`,
      planOutdated: (count) => `Plan: ${count} commit${count === 1 ? '' : 's'} (Outdated) ▸`,
      emptyPlanMessage: 'Plan has empty message ▸',
      planning: 'Generating plan…',
      committingStep: (completed, total) => `Committing ${completed}/${total}…`,
      running: 'Running C-Push…',
      needsDecision: 'Needs decision ▸',
      waitingManual: 'Manual merge ▸',
      pushFailed: 'Push failed ▸ Retry',
      conflict: 'Conflict ▸',
    },
    notifications: {
      needsDecisionTitle: 'Picket · Decision Needed',
      pushFailedTitle: 'Picket · Push Failed',
      conflictTitle: 'Picket · Merge Conflict',
      fallbackBody: (branch) => `${branch} needs your attention in Picket.`,
    },
  },
  workbench: {
    ariaLabel: 'Picket Workbench',
    lowerWorkspaceLabel: 'Commit plan, review, and messages',
    workspaceTabsLabel: 'Workspace',
    loadStateFailedFallback: 'Unable to load saved local workbench state.',
    saveStateFailedFallback: 'Unable to save local prompt state; current window input is preserved.',
    readPinFailedFallback: 'Unable to read window pin state. Try pinning the window again.',
    setPinFailedFallback: 'Unable to change window pin state.',
    openModelSettingsAction: 'Model Settings',
    reviewNoRisksSummary: 'No risks found',
    reviewFindingsSummary: (high, suggestion) => {
      const parts: string[] = [];
      if (high) parts.push(`${high} high-risk`);
      if (suggestion) parts.push(`${suggestion} suggestion${suggestion === 1 ? '' : 's'}`);
      return parts.join(', ');
    },
    reviewCompleteTitle: (summary) => `Review complete: ${summary}`,
    reviewCompleteDescription: 'Review findings updated.',
    pullRebaseTrackedDone: (branch) => `Pull & rebase complete (${branch}); working tree changes preserved.`,
    pullRebaseDone: 'Pull & rebase complete.',
    openFileMismatchError: 'Desktop did not return a matching editor launch result.',
    openedInEditor: (shortName, action, fileName, line) => {
      if (action === 'goto') return `Jumped to ${fileName}:${line} in ${shortName}`;
      if (action === 'merge') return `Opened conflict merge in ${shortName}: ${fileName}`;
      if (action === 'stagedDiff') return `Opened staged diff in ${shortName}: ${fileName}`;
      if (action === 'open') return `Opened file in ${shortName}: ${fileName}`;
      return `Opened diff in ${shortName}: ${fileName}`;
    },
    branchMismatchError: 'Git did not return a matching branch result. Refresh to verify status.',
    remoteBranchCheckedOut: (landedBranch, remoteBranch) =>
      `Checked out ${landedBranch} (tracking ${remoteBranch}); prompt and selection preserved.`,
    branchSwitched: (landedBranch) => `Switched to ${landedBranch}; prompt and selection preserved.`,
    readMismatchError: 'Git did not return a matching read result; prompt and selection preserved.',
    genericOperationFailed: 'Operation did not complete; prompt and selection preserved.',
    noChangedFilesToReReview: 'No modified files to re-review in the selected scope.',
    cancelledPlan: 'Cancelled commit plan generation; prompt and selection preserved.',
    cancelledReview: 'Cancelled code review; prompt and selection preserved.',
    detachedPrimaryBlocked: 'HEAD is detached. Switch to a local branch in the header or in your IDE/terminal first; prompt and selection preserved.',
    readFailedFallback: 'Status read failed. Refresh to try again; prompt and selection preserved.',
    missingWorktreeError: 'This worktree is no longer in Git’s active worktree list; prompt and selection preserved. Reopen the directory.',
    cPushInitialTitle: 'Analyzing and splitting selected changes…',
    cPushInitialDescription: 'Authorized scope frozen; unselected files and unstaged drafts stay untouched.',
    cPushInitialScope: (count) => `${count} selected file${count === 1 ? '' : 's'}`,
    cPushInitialProgressLabel: 'Read scope → Plan split → Commit',
    cPushTimelineFrozen: (count) => `Froze scope for ${count} selected file${count === 1 ? '' : 's'}`,
    cPushTimelinePlanning: 'Analyzing change ownership and planning split commits…',
  },
};

export const workbenchZh: WorkbenchMessages = {
  header: {
    opening: '正在打开…',
    openRepository: '打开仓库',
    repositoriesTitle: '仓库',
    singleWorktree: '单目录',
    worktreeCount: (count) => `${count} 工作树`,
    noRepositoriesOpened: '尚未打开仓库',
    openLocalRepository: '打开本机仓库…',
    sessionHistory: 'C-Push 会话记录',
    byWorktree: '按工作树',
    noSessionHistory: '暂无会话记录',
    idle: '空闲',
    noActiveSession: '暂无进行中的会话',
    collapseWindow: '收起为紧凑模式',
    pinWindow: '窗口置顶',
    unpinWindow: '取消置顶',
    settings: '设置',
    noBranchSelected: '未选择分支',
    branchesTitle: '分支',
    localBranches: '本地分支',
    remoteBranches: '远程分支',
    worktrees: '工作树',
    jumpToWorktree: (label) => `跳转 ${label}`,
    noLocalBranches: '尚无本地分支',
    switchAction: '切换',
    checkoutAction: '检出',
    noRemoteBranches: '暂无远程跟踪分支',
    worktreeUnavailable: '不可用',
    worktreeChangesCount: (count) => `${count} 项`,
    worktreeClean: '干净',
    remoteDetails: '远端详情',
    untracked: '未同步',
    trackedRemoteBranch: '跟踪远端分支',
    remoteCountTipLabel: '远端计数说明',
    remoteCountTipText: '仅基于本地跟踪引用，尚未联网检查远端；点击“拉取并变基”可联网同步。',
    aheadBehind: (ahead, behind) => `领先 ${ahead} 个提交　落后 ${behind} 个提交`,
    syncing: '同步中…',
    pullAndRebase: '拉取并变基',
  },
  changes: {
    panelLabel: '变更文件',
    selectAll: '全选变更',
    deselectAll: '取消全选变更',
    statusUnavailable: '状态不可用',
    noChanges: '无变更',
    noneSelected: '未选择变更',
    selectedSummary: (selected, total, hasPartial, conflicts) => {
      const base = `已选 ${selected}/${total} 项${hasPartial ? ' · 含部分暂存' : ''}`;
      return conflicts ? `${base} · ${conflicts} 个冲突` : base;
    },
    switchToTreeView: '切换目录树视图',
    switchToListView: '切换列表视图',
    refreshChanges: '刷新变更',
    selectFile: (path) => `选择 ${path}`,
    gitStatus: (status) => `Git 状态 ${status}`,
    ideActions: {
      merge: '合并',
      stagedDiff: '已暂存 Diff',
      open: '打开',
      diff: 'Diff',
    },
    conflictBadge: '· 冲突',
    partialStagedBadge: '· 部分暂存',
    skipReviewBadge: '· 忽略审查',
    findingCountLabel: (path, count) => `${path}：${count} 项审查意见`,
    emptyRepoTitle: '打开你的 Git 仓库',
    browserPreviewHint: '浏览器预览不读取本机目录',
    openingRepo: '正在打开…',
    openLocalRepository: '打开本机仓库…',
    worktreeUnavailableTitle: '工作树状态不可用',
    retryRead: '重试读取',
    workingTreeClean: '工作区干净',
    rootDirectory: '(根目录)',
    selectDirectory: (directory) => `选择目录 ${directory}`,
    deselectDirectory: (directory) => `取消选择目录 ${directory}`,
    localKeepTitle: '本地保留',
    localKeepBadge: '· 本地保留',
    localKeepConflicts: (count) => `${count} 个冲突`,
    localKeepEmptyHint: '将文件拖入此处，或在文件上右键选择“本地保留（不提交）”',
    noNormalChanges: '除本地保留外暂无其他变更',
    keepLocallyAction: '本地保留（不提交）',
    moveBackToChangesAction: '移回变更列表',
    dropToLocalKeepHint: '松开移入本地保留',
    dropToChangesHint: '松开移回变更列表',
    localKeepSelectedNotice: (count) => `已勾选 ${count} 个本地保留文件，本次操作将一并包含`,
    uncheckLocalKeepAction: '取消勾选',
    fileActionsMenuLabel: (path) => `${path} 操作菜单`,
    batchFileActionsMenuLabel: (count) => `已选 ${count} 个文件的操作菜单`,
  },
  dock: {
    expandMessage: (title) => `展开消息：${title}`,
    activeRegionLabel: '当前事项',
    collapseMessage: '收起消息',
    taskProgressLabel: '任务进度',
    timelineLabel: '会话执行步骤',
    qaLogLabel: '会话问答记录',
    qaEntry: (question, answer) => `问答：${question} → ${answer}`,
    viewReview: '查看',
    openConflictInEditor: (editorShort) => `在 ${editorShort} 中打开冲突文件`,
    gotIt: '知道了',
    dismissDone: '关闭已完成提示',
    recommendedBadge: '推荐',
    multiSelectHint: '可多选',
    showPreview: '预览',
    hidePreview: '收起预览',
    submitAnswers: '提交选择',
    cancelQuestion: '取消',
    dontAskAgainDefault: '此按钮不再提示',
    customOptionPlaceholder: '或输入自定义说明…',
    replyLabel: '回复当前事项',
    conflictReplyPlaceholder: '输入合并要求，回车发送…',
    questionReplyPlaceholder: '或输入更具体的要求，回车发送…',
    sendReply: '发送回复',
    cancelAction: '取消执行',
  },
  footer: {
    addModel: '添加模型',
    selectModelTitle: '选择模型',
    manageModelsMenuItem: '管理模型…',
    switchActionTitle: '切换主动作',
    switchActionAriaLabel: '切换主按钮动作',
    defaultCPushSubtitle: '默认智能拆分提交并推送',
    newActionMenuItem: '新建自定义按钮…',
    createWithPiMenuItem: '跟 Picket 对话创建…',
    manageActionsMenuItem: '管理按钮…',
  },
  compact: {
    expandWindow: '展开完整工作台',
    barLabel: '紧凑工作台',
    switchProject: '切换项目',
    projectDrawerLabel: '已打开仓库',
    filesDrawerLabel: '变更文件',
    actionsDrawerLabel: '切换主按钮动作',
    selectedTrigger: (selected, total) => `已选 ${selected}/${total}`,
    noModelBadge: '未配模型',
    planBadge: (count, outdated) => (outdated ? `计划 ${count} · 已过期 ▸` : `计划 ${count} ▸`),
    status: {
      noRepo: '打开仓库 ▸',
      clean: '工作区干净',
      readError: '状态不可用 ▸',
      detachedHead: '分离 HEAD ▸',
      noneSelected: (total) => `已选 0/${total} ▸`,
      selectedNoModel: (selected, total) => `已选 ${selected}/${total} · 未配置模型`,
      selectedReady: (selected, total) => `已选 ${selected}/${total} · 就绪`,
      planReady: (count) => `计划：${count} 个提交 ▸`,
      planOutdated: (count) => `计划：${count} 个提交（已过期）▸`,
      emptyPlanMessage: '计划含空白说明 ▸',
      planning: '正在生成计划…',
      committingStep: (completed, total) => `正在提交 ${completed}/${total}…`,
      running: '正在执行 C-Push…',
      needsDecision: '待你决定 ▸',
      waitingManual: '等待手动合并 ▸',
      pushFailed: '推送失败 ▸ 重试',
      conflict: '有冲突 ▸',
    },
    notifications: {
      needsDecisionTitle: 'Picket · 待你决定',
      pushFailedTitle: 'Picket · 推送失败',
      conflictTitle: 'Picket · 代码冲突',
      fallbackBody: (branch) => `${branch} 在 Picket 中需要你处理。`,
    },
  },
  workbench: {
    ariaLabel: 'Picket 工作台',
    lowerWorkspaceLabel: '提交计划、审查与消息',
    workspaceTabsLabel: '工作区',
    loadStateFailedFallback: '无法读取本地工作台保存记录。',
    saveStateFailedFallback: '无法保存本地提示词状态；当前窗口输入仍已保留。',
    readPinFailedFallback: '无法读取窗口置顶状态，请重试窗口置顶操作。',
    setPinFailedFallback: '无法设置窗口置顶',
    openModelSettingsAction: '模型设置',
    reviewNoRisksSummary: '未发现风险项',
    reviewFindingsSummary: (high, suggestion) =>
      `${high ? `${high} 项高危` : ''}${high && suggestion ? '、' : ''}${suggestion ? `${suggestion} 项建议` : ''}`,
    reviewCompleteTitle: (summary) => `审查完成：${summary}`,
    reviewCompleteDescription: '已更新审查意见。',
    pullRebaseTrackedDone: (branch) => `已完成拉取并变基（${branch}），工作区改动已保全。`,
    pullRebaseDone: '已完成拉取并变基。',
    openFileMismatchError: '桌面层未返回匹配的编辑器联动结果。',
    openedInEditor: (shortName, action, fileName, line) => {
      const detail = action === 'goto' ? `定位到 ${fileName}:${line}`
        : action === 'merge' ? `打开冲突合并：${fileName}`
        : action === 'stagedDiff' ? `打开已暂存 Diff：${fileName}`
        : action === 'open' ? `打开文件：${fileName}`
        : `打开 Diff：${fileName}`;
      return `已在 ${shortName} ${detail}`;
    },
    branchMismatchError: 'Git 未返回匹配的分支结果，请刷新核对实际状态。',
    remoteBranchCheckedOut: (landedBranch, remoteBranch) =>
      `已检出 ${landedBranch}（跟踪 ${remoteBranch}），提示词和选择已保留。`,
    branchSwitched: (landedBranch) => `已切换到 ${landedBranch}，提示词和选择已保留。`,
    readMismatchError: 'Git 未返回匹配的读取结果，当前提示词与选择已保留。',
    genericOperationFailed: '操作未完成，当前提示词与选择已保留。',
    noChangedFilesToReReview: '当前所选范围内没有待复审的变动文件。',
    cancelledPlan: '已取消预生成提交计划；提示词与选择已保留。',
    cancelledReview: '已取消代码审查；提示词与选择已保留。',
    detachedPrimaryBlocked: '当前处于分离 HEAD，请先在顶部切换已有分支，或在 IDE / 终端切好分支。提示词和所选范围已保留。',
    readFailedFallback: '状态读取失败，请重新刷新；提示词与选择已保留。',
    missingWorktreeError: '该工作树已不在 Git 的最新列表中，提示词和选择已保留，请重新打开目录。',
    cPushInitialTitle: '正在分析并拆分已选改动…',
    cPushInitialDescription: '已固定本次授权范围；未选文件与未暂存草稿保留在原处。',
    cPushInitialScope: (count) => `${count} 个已选文件`,
    cPushInitialProgressLabel: '读取范围 → 规划拆分 → 写入提交',
    cPushTimelineFrozen: (count) => `已固定 ${count} 个已选文件范围`,
    cPushTimelinePlanning: '正在分析改动归属并规划拆分提交…',
  },
};
