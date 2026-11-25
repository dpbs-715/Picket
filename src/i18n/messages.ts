import { MODEL_TOKEN_LIMITS, type TokenLimitValidationCode } from '../../shared/model-defaults';
import { DEFAULT_APP_LOCALE, type AppLocale } from '../../shared/language-protocol';
import type { ModelApiFormat, ModelProviderId } from '../../shared/model-protocol';
import type { GitOpenFileAction } from '../../shared/ide-protocol';

export const LANGUAGE_OPTIONS: readonly { id: AppLocale; label: string }[] = [
  { id: 'en', label: 'English' },
  { id: 'zh', label: '简体中文' },
];

export interface Messages {
  common: {
    closeOverlay(title: string): string;
    dismissNotice: string;
    cancel: string;
    binary: string;
    detachedHead: string;
    protectedBranch: string;
  };
  providers: Record<ModelProviderId, string>;
  apiFormats: Record<ModelApiFormat, { label: string; short: string }>;
  messageLabels: {
    progress: string;
    question: string;
    conflict: string;
    waitingManual: string;
    resolved: string;
    pushFailed: string;
    done: string;
    review: string;
    error: string;
  };
  primaryActions: {
    running: string;
    needsDecision: string;
    conflict: string;
    waitingManual: string;
    pushFailed: string;
    cPush: string;
    configureModel: string;
  };
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
  commit: {
    tabLabel: string;
    planningBadge: string;
    planOutdatedBadge: string;
    planCountBadge(count: number): string;
    promptToggle: string;
    promptActiveToggle: string;
    hidePromptLabel: string;
    clearPromptLabel: string;
    promptLabel: string;
    promptPlaceholder: string;
    planningStatus: string;
    cancelPlanning: string;
    previewPlanButton: string;
    configureModelButton: string;
    planSummary(groupCount: number, skippedCount: number): string;
    regeneratePlanAction: string;
    clearPlanAction: string;
    groupHeading(index: number, total: number): string;
    groupMessageLabel(index: number, total: number): string;
    groupMessagePlaceholder: string;
    emptyPlanMessageHint: string;
    emptyPlanMessageError: string;
    allSkippedPlanNote(count: number): string;
  };
  review: {
    tabLabel: string;
    reReviewLabel: string;
    tabStatusRunning: string;
    tabStatusChanged: string;
    tabStatusUncovered: string;
    tabStatusPassed: string;
    tabStatusCounts(high: number, suggestion: number): string;
    runningStatus: string;
    cancelReview: string;
    startReview: string;
    addModelButton: string;
    uncoveredNotice(count: number): string;
    changedNotice(count: number): string;
    reReviewChangedButton: string;
    noFilesReviewedTitle: string;
    previousScopePassedTitle: string;
    passedTitle: string;
    resultTipLabel: string;
    noFilesReviewedTip: string;
    changedScopeTip: string;
    passedScopeTip: string;
    severityHigh: string;
    severitySuggestion: string;
    skippedHeading: string;
    skippedTipLabel: string;
    skippedTipText: string;
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
    replyLabel: string;
    conflictReplyPlaceholder: string;
    questionReplyPlaceholder: string;
    sendReply: string;
  };
  footer: {
    addModel: string;
    selectModelTitle: string;
    manageModelsMenuItem: string;
  };
  compact: {
    expandWindow: string;
    barLabel: string;
    switchProject: string;
    projectDrawerLabel: string;
    filesDrawerLabel: string;
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
  settings: {
    title: string;
    currentProject: string;
    projectDefaults: string;
    noProjectBadge: string;
    editorIntegration: string;
    editorIntegrationTipLabel: string;
    editorIntegrationTip(cli: string, hasRepository: boolean): string;
    noEditorsDetected: string;
    noEditorsHelp: string;
    preferredEditor: string;
    commitLanguage: string;
    commitLanguageTipLabel: string;
    commitLanguageTip(hasRepository: boolean): string;
    appLanguage: string;
    modelsHeading: string;
    addProvider: string;
    addChildModel: string;
    noModelsConfigured: string;
    addFirstProvider: string;
    activeModelBadge: string;
    connectionModelCount(count: number): string;
    emptyConnectionHint: string;
    toggleConnectionLabel(name: string, expanded: boolean): string;
    editConnectionLabel(name: string): string;
    deleteConnectionLabel(name: string): string;
    deleteConnectionConfirm(name: string, modelCount: number, includesActive: boolean): string;
    confirmDeleteConnection: string;
    editModelLabel(name: string): string;
    deleteModelLabel(name: string): string;
    modelDetailTitle(params: {
      connectionName: string;
      providerLabel: string;
      formatShort: string;
      modelId: string;
      contextWindow: number;
      maxOutputTokens: number;
      baseUrl?: string;
    }): string;
  };
  modelForm: {
    addProviderTitle: string;
    editConnectionTitle: string;
    addModelTitle: string;
    editModelTitle: string;
    providerLegend: string;
    connectionSectionHeading: string;
    firstModelSectionHeading: string;
    connectionNameLabel: string;
    connectionNamePlaceholder: string;
    parentConnectionLabel: string;
    connectionIdentityLockedNote: string;
    rotateKeyImpactNote(count: number): string;
    testNeedsModelHint: string;
    advancedToggle: string;
    apiFormatLabel: string;
    displayNameLabel: string;
    displayNamePlaceholder: string;
    modelIdLabel: string;
    baseUrlLabel: string;
    apiKeyTipLabel: string;
    apiKeyTipText: string;
    apiKeyKeepPlaceholder: string;
    apiKeyEnterPlaceholder: string;
    revealApiKeyLabel: string;
    hideApiKeyLabel: string;
    revealingApiKeyLabel: string;
    revealedKeyReadonlyLabel: string;
    revealKeyErrorFallback: string;
    contextWindowLabel: string;
    contextWindowTipLabel: string;
    contextWindowTipText(defaultValue: number): string;
    defaultPlaceholder(defaultValue: number): string;
    maxOutputTokensLabel: string;
    maxOutputTokensTipLabel: string;
    maxOutputTokensTipText(defaultValue: number): string;
    testing: string;
    testConnection: string;
    saving: string;
    saveChanges: string;
    saveModel: string;
    saveAnyway: string;
    saveAndUse: string;
    testErrorFallback: string;
    saveErrorFallback: string;
    validation: {
      missingModelId: string;
      missingBaseUrl: string;
      invalidUrlCredentialsOrProtocol: string;
      invalidUrlFormat: string;
      missingApiKey: string;
      tokenLimit(code: TokenLimitValidationCode): string;
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
  desktop: {
    disconnectedModelOrCPush: string;
    disconnectedGit: string;
    disconnectedTestModel: string;
    disconnectedSaveModel: string;
    disconnectedRevealModelKey: string;
    browserPinUnsupported: string;
    browserWindowUnsupported: string;
    cPushPlanMismatch: string;
    cPushStartMismatch: string;
    pullRebaseMismatch: string;
    cPushRespondMismatch: string;
    modelTestMismatch: string;
    modelSaveMismatch: string;
    modelSelectMismatch: string;
    modelDeleteFailed: string;
    modelRevealMismatch: string;
    reviewMismatch: string;
    missingModelScopeTokens: string;
    missingCPushScopeTokens: string;
    missingCPushRunId: string;
    invalidLineNumber: string;
    editorReplyMismatch: string;
    branchReplyMismatch: string;
    gitReadReplyMismatch: string;
    localKeepReplyMismatch: string;
  };
  foundation: {
    phaseLabel: string;
    heading: string;
    intro: string;
    browserPreviewNoticePrefix: string;
    browserPreviewNoticeSuffix: string;
    piSectionHeading: string;
    piStatus: {
      idle: string;
      running: string;
      completed: string;
      cancelled: string;
      failed: string;
    };
    piDescription: string;
    verifyStreamButton: string;
    cancelTaskButton: string;
    streamOutputLabel: string;
    waitingPlaceholder: string;
    piReportStats(pid: number, deltaCount: number, exitCode: number): string;
    gitSectionHeading: string;
    gitStatus: {
      idle: string;
      running: string;
      done: string;
      failed: string;
    };
    gitDescription: string;
    verifyTempGitButton: string;
    gitCasesPassed(gitVersion: string, count: number): string;
    gitCommitsSummary(commitCount: number, removed: boolean): string;
    footerLine1: string;
    footerLine2: string;
  };
}

const en: Messages = {
  common: {
    closeOverlay: (title) => `Close ${title}`,
    dismissNotice: 'Dismiss notice',
    cancel: 'Cancel',
    binary: 'Binary',
    detachedHead: 'Detached HEAD',
    protectedBranch: 'Protected',
  },
  providers: {
    anthropic: 'Anthropic',
    openai: 'OpenAI',
    deepseek: 'DeepSeek',
    ollama: 'Ollama (Local)',
    custom: 'Custom Compatible',
  },
  apiFormats: {
    'openai-completions': {
      label: 'OpenAI Chat Completions (/chat/completions)',
      short: 'Chat Completions',
    },
    'openai-responses': {
      label: 'OpenAI Responses (/responses)',
      short: 'Responses',
    },
    'anthropic-messages': {
      label: 'Anthropic Messages (/v1/messages)',
      short: 'Anthropic',
    },
  },
  messageLabels: {
    progress: 'Running',
    question: 'Needs Decision',
    conflict: 'Conflict',
    waitingManual: 'Manual Merge',
    resolved: 'Ready to Push',
    pushFailed: 'Push Failed',
    done: 'Completed',
    review: 'Review Done',
    error: 'Action Needed',
  },
  primaryActions: {
    running: 'Running…',
    needsDecision: 'C-Push · Decide',
    conflict: 'Resolve Conflict',
    waitingManual: 'C-Push · Decide',
    pushFailed: 'Retry Push',
    cPush: 'C-Push',
    configureModel: 'Configure Model',
  },
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
  commit: {
    tabLabel: 'Commit Plan',
    planningBadge: 'Planning',
    planOutdatedBadge: 'Outdated',
    planCountBadge: (count) => `${count} commit${count === 1 ? '' : 's'}`,
    promptToggle: '+ Prompt',
    promptActiveToggle: 'Prompt',
    hidePromptLabel: 'Hide Extra Instructions',
    clearPromptLabel: 'Clear Extra Instructions',
    promptLabel: 'Extra instructions',
    promptPlaceholder: 'Optional instructions, e.g. split lockfile separately…',
    planningStatus: 'Generating commit plan…',
    cancelPlanning: 'Cancel',
    previewPlanButton: 'Preview Plan',
    configureModelButton: 'Configure Model',
    planSummary: (groupCount, skippedCount) => {
      const base = `Plan · ${groupCount} commit${groupCount === 1 ? '' : 's'}`;
      return skippedCount ? `${base} · ${skippedCount} skipped` : base;
    },
    regeneratePlanAction: 'Regenerate Plan',
    clearPlanAction: 'Clear Plan',
    groupHeading: (index, total) => `Commit ${index}/${total}`,
    groupMessageLabel: (index, total) => `Commit ${index} of ${total} message`,
    groupMessagePlaceholder: 'Edit commit message for this group…',
    emptyPlanMessageHint: 'Enter a commit message or clear the plan.',
    emptyPlanMessageError: 'Fill in every commit message in the plan, or clear the plan before running C-Push.',
    allSkippedPlanNote: (count) => `All ${count} selected file${count === 1 ? ' is' : 's are'} marked as skipped in this plan.`,
  },
  review: {
    tabLabel: 'Code Review',
    reReviewLabel: 'Re-review Selected Scope',
    tabStatusRunning: 'Reviewing',
    tabStatusChanged: 'Changed',
    tabStatusUncovered: 'Uncovered',
    tabStatusPassed: 'Passed',
    tabStatusCounts: (high, suggestion) =>
      high
        ? `${high} High${suggestion ? ` · ${suggestion} Sugg.` : ''}`
        : `${suggestion} Sugg.`,
    runningStatus: 'Reviewing selected scope…',
    cancelReview: 'Cancel Review',
    startReview: 'Review Selected Scope',
    addModelButton: 'Add Model',
    uncoveredNotice: (count) =>
      `${count} selected file${count === 1 ? ' has' : 's have'} not been reviewed yet. Re-review the selected scope.`,
    changedNotice: (count) => `${count} file${count === 1 ? '' : 's'} changed since review`,
    reReviewChangedButton: 'Re-review Changed Files',
    noFilesReviewedTitle: 'No files in selected scope were reviewed',
    previousScopePassedTitle: 'No risks found in previously reviewed scope',
    passedTitle: 'Review passed',
    resultTipLabel: 'Review result info',
    noFilesReviewedTip: 'Lockfiles and binary files skip review by default and can still be committed normally.',
    changedScopeTip: 'The selected scope has changed. Re-review to inspect the latest changes.',
    passedScopeTip: 'No risk items were found within the reviewed scope.',
    severityHigh: 'High',
    severitySuggestion: 'Suggestion',
    skippedHeading: 'Skipped during review',
    skippedTipLabel: 'Skipped review info',
    skippedTipText: 'Lockfiles, binary files, or oversized diffs skip review by default without blocking commits.',
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
    replyLabel: 'Reply to active message',
    conflictReplyPlaceholder: 'Enter merge instructions and press Enter…',
    questionReplyPlaceholder: 'Or enter custom instructions and press Enter…',
    sendReply: 'Send Reply',
  },
  footer: {
    addModel: 'Add Model',
    selectModelTitle: 'Select Model',
    manageModelsMenuItem: 'Manage Models…',
  },
  compact: {
    expandWindow: 'Expand Workbench',
    barLabel: 'Compact Workbench',
    switchProject: 'Switch project',
    projectDrawerLabel: 'Opened repositories',
    filesDrawerLabel: 'Changed files',
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
  settings: {
    title: 'Settings',
    currentProject: 'Current Project',
    projectDefaults: 'Project Defaults',
    noProjectBadge: 'No repo open',
    editorIntegration: 'Editor Integration',
    editorIntegrationTipLabel: 'Editor integration info',
    editorIntegrationTip: (cli, hasRepository) =>
      `Invokes ${cli} to open files, staged diffs, and review line numbers for ${hasRepository ? 'this project' : 'new projects'}; automatically detects installed editors.`,
    noEditorsDetected: 'No supported editors detected',
    noEditorsHelp: 'Install IntelliJ IDEA, RustRover, WebStorm, VS Code, or Cursor and reopen to detect.',
    preferredEditor: 'Preferred Editor',
    commitLanguage: 'Commit Message Language',
    commitLanguageTipLabel: 'Commit message language info',
    commitLanguageTip: (hasRepository) =>
      `Controls the summary and bullet language for previewed commit plans and C-Push commits in ${hasRepository ? 'this project' : 'new projects'}. Conventional Commit prefixes (feat, fix, etc.) stay in English.`,
    appLanguage: 'App Language',
    modelsHeading: 'Models',
    addProvider: 'Provider',
    addChildModel: 'Model',
    noModelsConfigured: 'No models configured yet',
    addFirstProvider: 'Add First Provider',
    activeModelBadge: 'Active',
    connectionModelCount: (count) => `${count} model${count === 1 ? '' : 's'}`,
    emptyConnectionHint: 'No models under this connection yet.',
    toggleConnectionLabel: (name, expanded) => `${expanded ? 'Collapse' : 'Expand'} connection ${name}`,
    editConnectionLabel: (name) => `Edit connection ${name}`,
    deleteConnectionLabel: (name) => `Delete connection ${name}`,
    deleteConnectionConfirm: (name, modelCount, includesActive) => {
      if (modelCount === 0) return `Delete connection "${name}" and its saved key?`;
      const activePart = includesActive ? ' (includes active model)' : '';
      return `Delete connection "${name}" along with ${modelCount} model${modelCount === 1 ? '' : 's'}${activePart} and its saved key?`;
    },
    confirmDeleteConnection: 'Delete Connection',
    editModelLabel: (name) => `Edit model ${name}`,
    deleteModelLabel: (name) => `Delete model ${name}`,
    modelDetailTitle: ({ connectionName, providerLabel, formatShort, modelId, contextWindow, maxOutputTokens, baseUrl }) =>
      `${connectionName} · ${providerLabel} (${formatShort}) · ${modelId} · Context ${contextWindow} / Max output ${maxOutputTokens}${baseUrl ? ` · ${baseUrl}` : ''}`,
  },
  modelForm: {
    addProviderTitle: 'Add Provider',
    editConnectionTitle: 'Edit Connection',
    addModelTitle: 'Add Model',
    editModelTitle: 'Edit Model',
    providerLegend: 'Provider',
    connectionSectionHeading: 'Connection',
    firstModelSectionHeading: 'First Model',
    connectionNameLabel: 'Connection Name',
    connectionNamePlaceholder: 'Defaults to provider name when empty',
    parentConnectionLabel: 'Connection',
    connectionIdentityLockedNote: 'Provider type and base URL identify this connection. Create a new provider connection to use a different endpoint.',
    rotateKeyImpactNote: (count) => `Applies to all ${count} model${count === 1 ? '' : 's'} under this connection.`,
    testNeedsModelHint: 'Add at least one model under this connection before testing.',
    advancedToggle: 'Advanced',
    apiFormatLabel: 'API Format',
    displayNameLabel: 'Display Name',
    displayNamePlaceholder: 'Defaults to Model ID when empty',
    modelIdLabel: 'Model ID',
    baseUrlLabel: 'Base URL',
    apiKeyTipLabel: 'API Key storage info',
    apiKeyTipText: 'Stored locally in ~/.picket/credentials.json (0600 permissions); never written to models.json, logs, or ordinary UI state.',
    apiKeyKeepPlaceholder: 'Leave empty to keep saved key, or enter a new key',
    apiKeyEnterPlaceholder: 'Enter API key',
    revealApiKeyLabel: 'Reveal saved API key',
    hideApiKeyLabel: 'Hide API key',
    revealingApiKeyLabel: 'Authenticating…',
    revealedKeyReadonlyLabel: 'Saved API key (read-only)',
    revealKeyErrorFallback: 'Unable to reveal API key.',
    contextWindowLabel: 'Context Window',
    contextWindowTipLabel: 'Context window info',
    contextWindowTipText: (defaultValue) =>
      `Maximum context window supported by the model. Leave empty for default (${defaultValue}).`,
    defaultPlaceholder: (defaultValue) => `Default ${defaultValue}`,
    maxOutputTokensLabel: 'Max Output Tokens',
    maxOutputTokensTipLabel: 'Max output tokens info',
    maxOutputTokensTipText: (defaultValue) =>
      `Maximum tokens per response (including reasoning). Leave empty for default (${defaultValue}); cannot exceed context window.`,
    testing: 'Testing…',
    testConnection: 'Test Connection',
    saving: 'Saving…',
    saveChanges: 'Save Changes',
    saveModel: 'Save',
    saveAnyway: 'Save Anyway',
    saveAndUse: 'Save & Use',
    testErrorFallback: 'Connection test did not complete. Check base URL, model ID, or network; input preserved.',
    saveErrorFallback: 'Save did not complete. No configuration or key was written; input preserved.',
    validation: {
      missingModelId: 'Enter a model ID',
      missingBaseUrl: 'Enter a base URL',
      invalidUrlCredentialsOrProtocol: 'Base URL must be an HTTP/HTTPS URL without username or password',
      invalidUrlFormat: 'Enter a valid HTTP/HTTPS base URL',
      missingApiKey: 'Enter an API key',
      tokenLimit: (code) => {
        if (code === 'context_range') {
          return `Context window must be an integer between ${MODEL_TOKEN_LIMITS.minContextWindow} and ${MODEL_TOKEN_LIMITS.maxContextWindow}`;
        }
        if (code === 'output_range') {
          return `Max output tokens must be an integer between ${MODEL_TOKEN_LIMITS.minMaxOutputTokens} and ${MODEL_TOKEN_LIMITS.maxMaxOutputTokens}`;
        }
        return 'Max output tokens cannot exceed context window';
      },
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
  desktop: {
    disconnectedModelOrCPush: 'Model and C-Push services are not connected in browser preview; prompt and selection are preserved.',
    disconnectedGit: 'Desktop Git service is not connected in browser preview; no commits, pushes, or repository changes were made, and prompt and selection are preserved.',
    disconnectedTestModel: 'Desktop model service is not connected; no connection request was sent, and input is preserved.',
    disconnectedSaveModel: 'Desktop credential storage is not connected in browser preview; no configuration or API key was saved, and input is preserved.',
    disconnectedRevealModelKey: 'System authentication and desktop credential storage are unavailable in browser preview; no API key was read.',
    browserPinUnsupported: 'Window pinning is unavailable in browser preview. Run pnpm desktop dev to open the desktop app.',
    browserWindowUnsupported: 'Window resizing and pinning are unavailable in browser preview; compact layout is previewed inside the page.',
    cPushPlanMismatch: 'C-Push plan reply target or request identity mismatch; ignored reply.',
    cPushStartMismatch: 'C-Push start reply target or request identity mismatch; ignored reply.',
    pullRebaseMismatch: 'Pull & rebase reply target or request identity mismatch. Refresh to verify Git status.',
    cPushRespondMismatch: 'C-Push response reply session or request identity mismatch; ignored reply.',
    modelTestMismatch: 'Model connection test reply did not match the active request; ignored result.',
    modelSaveMismatch: 'Saved model configuration did not match submitted parameters. Reopen Settings to verify.',
    modelSelectMismatch: 'Model selection reply mismatch. Select the model again.',
    modelDeleteFailed: 'Model deletion did not take effect. Reopen Settings and try again.',
    modelRevealMismatch: 'Revealed API key reply did not match the requested model; ignored reply.',
    reviewMismatch: 'Code review reply target or request identity mismatch; ignored result.',
    missingModelScopeTokens: 'Missing Rust-issued file scope tokens. Refresh changes and retry; prompt and selection are preserved.',
    missingCPushScopeTokens: 'Missing Rust-issued file scope tokens. Refresh changes before starting C-Push; prompt and selection are preserved.',
    missingCPushRunId: 'Missing active C-Push session identifier. Refresh and retry from the message bar.',
    invalidLineNumber: 'Invalid line number; line numbers must start at 1.',
    editorReplyMismatch: 'Editor launch reply target or request identity mismatch; ignored reply.',
    branchReplyMismatch: 'Branch operation reply target or request identity mismatch. Refresh to verify Git status.',
    gitReadReplyMismatch: 'Git read reply target or request identity mismatch; ignored result.',
    localKeepReplyMismatch: 'Local Keep update reply target or request identity mismatch. Refresh to verify status.',
  },
  foundation: {
    phaseLabel: 'M0 · Foundation Probe',
    heading: 'Verify Core Primitives First',
    intro: 'This diagnostic page verifies desktop IPC, sidecar lifecycle, and isolated Git scope safety without touching user repositories.',
    browserPreviewNoticePrefix: 'Running in browser preview. Run ',
    browserPreviewNoticeSuffix: ' to use desktop capabilities.',
    piSectionHeading: 'Isolated Pi Core Sidecar',
    piStatus: {
      idle: 'Unverified',
      running: 'Verifying',
      completed: 'Exited cleanly',
      cancelled: 'Cancelled & exited',
      failed: 'Verification failed',
    },
    piDescription: 'Bundled Node + Pi SDK with scripted responses; no external models, credentials, filesystem, or shell tools.',
    verifyStreamButton: 'Verify Streaming IPC',
    cancelTaskButton: 'Cancel Task',
    streamOutputLabel: 'Pi streaming output',
    waitingPlaceholder: 'Waiting for verification…',
    piReportStats: (pid, deltaCount, exitCode) =>
      `PID ${pid} · ${deltaCount} text chunks · exit code ${exitCode}`,
    gitSectionHeading: 'Git Scope & Draft Preservation',
    gitStatus: {
      idle: 'Unverified',
      running: 'Verifying',
      done: 'Verified',
      failed: 'Verification failed',
    },
    gitDescription: 'Creates and removes a temporary repository to verify partial staging, unselected staged files, split commits, and idempotency.',
    verifyTempGitButton: 'Verify Disposable Git Repo',
    gitCasesPassed: (gitVersion, count) => `${gitVersion} · ${count} checks passed`,
    gitCommitsSummary: (commitCount, removed) =>
      `Created ${commitCount} test commits; temporary repository ${removed ? 'removed' : 'not removed'}.`,
    footerLine1: 'Production UI follows docs/design/picket-mvp.tsx.',
    footerLine2: 'Isolated M0 diagnostic harness.',
  },
};

const zh: Messages = {
  common: {
    closeOverlay: (title) => `关闭${title}`,
    dismissNotice: '关闭操作提示',
    cancel: '取消',
    binary: '二进制',
    detachedHead: '分离 HEAD',
    protectedBranch: '保护分支',
  },
  providers: {
    anthropic: 'Anthropic',
    openai: 'OpenAI',
    deepseek: 'DeepSeek',
    ollama: 'Ollama（本地）',
    custom: '自定义兼容',
  },
  apiFormats: {
    'openai-completions': {
      label: 'OpenAI Chat Completions (/chat/completions)',
      short: 'Chat Completions',
    },
    'openai-responses': {
      label: 'OpenAI Responses (/responses)',
      short: 'Responses',
    },
    'anthropic-messages': {
      label: 'Anthropic Messages (/v1/messages)',
      short: 'Anthropic',
    },
  },
  messageLabels: {
    progress: '执行中',
    question: '待决定',
    conflict: '代码冲突',
    waitingManual: '待合并',
    resolved: '待推送',
    pushFailed: '推送失败',
    done: '已完成',
    review: '审查完成',
    error: '需要处理',
  },
  primaryActions: {
    running: '执行中…',
    needsDecision: 'C-Push · 待决定',
    conflict: '处理冲突',
    waitingManual: 'C-Push · 待决定',
    pushFailed: '重试推送',
    cPush: 'C-Push',
    configureModel: '配置模型',
  },
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
  commit: {
    tabLabel: '提交计划',
    planningBadge: '规划中',
    planOutdatedBadge: '已过期',
    planCountBadge: (count) => `${count} 个提交`,
    promptToggle: '+ 提示词',
    promptActiveToggle: '提示词',
    hidePromptLabel: '收起扩展提示词',
    clearPromptLabel: '清空扩展提示词',
    promptLabel: '扩展提示词',
    promptPlaceholder: '补充要求（可选），如：lock 文件单独提交…',
    planningStatus: '正在生成提交计划…',
    cancelPlanning: '取消',
    previewPlanButton: '预生成计划',
    configureModelButton: '配置模型',
    planSummary: (groupCount, skippedCount) => {
      const base = `计划 · ${groupCount} 个提交`;
      return skippedCount ? `${base} · 跳过 ${skippedCount} 项` : base;
    },
    regeneratePlanAction: '重新预生成',
    clearPlanAction: '清空计划',
    groupHeading: (index, total) => `提交 ${index}/${total}`,
    groupMessageLabel: (index, total) => `第 ${index}/${total} 个提交说明`,
    groupMessagePlaceholder: '编辑本组提交说明…',
    emptyPlanMessageHint: '请填写提交说明，或清空当前计划。',
    emptyPlanMessageError: '请补全预生成计划中的每条提交说明，或清空计划后再执行 C-Push。',
    allSkippedPlanNote: (count) => `当前计划已确认跳过全部 ${count} 个已选文件。`,
  },
  review: {
    tabLabel: '审查意见',
    reReviewLabel: '重新审查已选范围',
    tabStatusRunning: '审查中',
    tabStatusChanged: '已变化',
    tabStatusUncovered: '未覆盖',
    tabStatusPassed: '通过',
    tabStatusCounts: (high, suggestion) =>
      high
        ? `${high} 高危${suggestion ? ` · ${suggestion} 建议` : ''}`
        : `${suggestion} 建议`,
    runningStatus: '正在审查已选范围…',
    cancelReview: '取消审查',
    startReview: '开始审查已选范围',
    addModelButton: '添加模型',
    uncoveredNotice: (count) => `当前选择有 ${count} 个文件尚未审查，请重新审查已选范围。`,
    changedNotice: (count) => `审查后有 ${count} 个文件已变化`,
    reReviewChangedButton: '复审变动文件',
    noFilesReviewedTitle: '所选范围均未纳入审查',
    previousScopePassedTitle: '原审查范围未发现风险项',
    passedTitle: '审查通过',
    resultTipLabel: '审查结果说明',
    noFilesReviewedTip: '锁文件或二进制内容默认跳过审查，仍可正常勾选提交。',
    changedScopeTip: '当前范围已有变化，需重新审查后才能判断最新内容。',
    passedScopeTip: '仅代表实际已审范围内未发现风险项。',
    severityHigh: '高危',
    severitySuggestion: '建议',
    skippedHeading: '以下内容未覆盖审查',
    skippedTipLabel: '未覆盖审查说明',
    skippedTipText: '锁文件、二进制或超长改动默认跳过审查，不影响正常勾选提交。',
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
    replyLabel: '回复当前事项',
    conflictReplyPlaceholder: '输入合并要求，回车发送…',
    questionReplyPlaceholder: '或输入更具体的要求，回车发送…',
    sendReply: '发送回复',
  },
  footer: {
    addModel: '添加模型',
    selectModelTitle: '选择模型',
    manageModelsMenuItem: '管理模型…',
  },
  compact: {
    expandWindow: '展开完整工作台',
    barLabel: '紧凑工作台',
    switchProject: '切换项目',
    projectDrawerLabel: '已打开仓库',
    filesDrawerLabel: '变更文件',
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
  settings: {
    title: '设置',
    currentProject: '当前项目',
    projectDefaults: '默认项目配置',
    noProjectBadge: '未打开仓库',
    editorIntegration: '编辑器联动',
    editorIntegrationTipLabel: '编辑器联动说明',
    editorIntegrationTip: (cli, hasRepository) =>
      `为${hasRepository ? '当前项目' : '新打开项目'}调用 ${cli} 打开文件、已暂存 Diff 与审查行号；自动识别本机已安装编辑器并独立保存。`,
    noEditorsDetected: '未检测到已安装的受支持编辑器',
    noEditorsHelp: '安装 IntelliJ IDEA、RustRover、WebStorm、VS Code 或 Cursor 后重新打开即可识别。',
    preferredEditor: '首选编辑器',
    commitLanguage: '提交说明语言',
    commitLanguageTipLabel: '提交说明语言说明',
    commitLanguageTip: (hasRepository) =>
      `控制${hasRepository ? '当前项目' : '新打开项目'}预生成计划与 C-Push 拆分提交的标题描述及要点语言；Conventional Commit 前缀（如 feat、fix）保持英文。`,
    appLanguage: '界面语言',
    modelsHeading: '模型设置',
    addProvider: '供应商',
    addChildModel: '模型',
    noModelsConfigured: '尚未配置可用模型',
    addFirstProvider: '添加第一个供应商',
    activeModelBadge: '当前使用',
    connectionModelCount: (count) => `${count} 个模型`,
    emptyConnectionHint: '该连接下还没有模型。',
    toggleConnectionLabel: (name, expanded) => `${expanded ? '收起' : '展开'}供应商连接 ${name}`,
    editConnectionLabel: (name) => `编辑连接 ${name}`,
    deleteConnectionLabel: (name) => `删除连接 ${name}`,
    deleteConnectionConfirm: (name, modelCount, includesActive) => {
      if (modelCount === 0) return `确认删除连接“${name}”及其已保存密钥？`;
      const activePart = includesActive ? '（含当前使用模型）' : '';
      return `确认删除连接“${name}”、其下 ${modelCount} 个模型${activePart}及已保存密钥？`;
    },
    confirmDeleteConnection: '确认删除连接',
    editModelLabel: (name) => `编辑模型 ${name}`,
    deleteModelLabel: (name) => `删除模型 ${name}`,
    modelDetailTitle: ({ connectionName, providerLabel, formatShort, modelId, contextWindow, maxOutputTokens, baseUrl }) =>
      `${connectionName} · ${providerLabel}（${formatShort}）· ${modelId} · 上下文 ${contextWindow} / 最大输出 ${maxOutputTokens}${baseUrl ? ` · ${baseUrl}` : ''}`,
  },
  modelForm: {
    addProviderTitle: '添加供应商',
    editConnectionTitle: '编辑连接',
    addModelTitle: '添加模型',
    editModelTitle: '编辑模型',
    providerLegend: '供应商模板',
    connectionSectionHeading: '连接信息',
    firstModelSectionHeading: '首个模型',
    connectionNameLabel: '连接名称',
    connectionNamePlaceholder: '为空时使用供应商名称',
    parentConnectionLabel: '所属连接',
    connectionIdentityLockedNote: '供应商类型与接口地址为当前连接身份；如需更换目标地址，请新建一条连接。',
    rotateKeyImpactNote: (count) => `将用于这条连接下的 ${count} 个模型。`,
    testNeedsModelHint: '请先在该连接下至少添加一个模型后再测试连接。',
    advancedToggle: '高级选项',
    apiFormatLabel: '接口格式',
    displayNameLabel: '显示名称',
    displayNamePlaceholder: '为空时使用模型 ID',
    modelIdLabel: '模型 ID',
    baseUrlLabel: '接口地址（Base URL）',
    apiKeyTipLabel: 'API Key 存储说明',
    apiKeyTipText: '密钥以 0600 权限保存在本机 ~/.picket/credentials.json，不写入 models.json、日志或常规界面状态。',
    apiKeyKeepPlaceholder: '留空则保留原密钥；输入新值则替换',
    apiKeyEnterPlaceholder: '输入密钥',
    revealApiKeyLabel: '显示已保存的完整密钥',
    hideApiKeyLabel: '隐藏完整密钥',
    revealingApiKeyLabel: '正在验证身份…',
    revealedKeyReadonlyLabel: '当前已保存密钥（只读）',
    revealKeyErrorFallback: '无法显示完整密钥。',
    contextWindowLabel: '上下文长度',
    contextWindowTipLabel: '上下文长度说明',
    contextWindowTipText: (defaultValue) =>
      `模型支持的最大上下文窗口，留空回退默认值（${defaultValue}）。`,
    defaultPlaceholder: (defaultValue) => `默认 ${defaultValue}`,
    maxOutputTokensLabel: '最大输出 Token',
    maxOutputTokensTipLabel: '最大输出 Token 说明',
    maxOutputTokensTipText: (defaultValue) =>
      `单次回复（含推理思考）的最大输出上限，留空默认 ${defaultValue}，且不能超过上下文长度。`,
    testing: '测试中…',
    testConnection: '测试连接',
    saving: '保存中…',
    saveChanges: '保存修改',
    saveModel: '保存',
    saveAnyway: '仍然保存',
    saveAndUse: '保存并使用',
    testErrorFallback: '连接测试未完成，请检查接口地址、模型 ID 或网络连接；输入已保留。',
    saveErrorFallback: '保存未完成，没有写入配置或密钥；输入已保留。',
    validation: {
      missingModelId: '请填写模型 ID',
      missingBaseUrl: '请填写接口地址',
      invalidUrlCredentialsOrProtocol: '接口地址须为不含用户名和密码的 HTTP / HTTPS 地址',
      invalidUrlFormat: '请填写有效的 HTTP / HTTPS 接口地址',
      missingApiKey: '请填写 API Key',
      tokenLimit: (code) => {
        if (code === 'context_range') {
          return `上下文长度须为 ${MODEL_TOKEN_LIMITS.minContextWindow} 至 ${MODEL_TOKEN_LIMITS.maxContextWindow} 之间的整数`;
        }
        if (code === 'output_range') {
          return `最大输出 Token 须为 ${MODEL_TOKEN_LIMITS.minMaxOutputTokens} 至 ${MODEL_TOKEN_LIMITS.maxMaxOutputTokens} 之间的整数`;
        }
        return '最大输出 Token 不能超过上下文长度';
      },
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
  desktop: {
    disconnectedModelOrCPush: '当前环境未连接桌面模型或 C-Push 服务，此操作尚未接入浏览器预览；提示词和选择已保留。',
    disconnectedGit: '当前环境未连接桌面 Git 服务，此操作尚未接入浏览器预览；不会提交、推送或修改仓库，提示词和选择已保留。',
    disconnectedTestModel: '当前环境未连接桌面模型服务，未发送连接请求；输入已保留。',
    disconnectedSaveModel: '当前环境未连接桌面凭据服务，没有保存配置或密钥；输入已保留。',
    disconnectedRevealModelKey: '当前环境未连接系统身份验证与本地凭据服务，无法显示密钥。',
    browserPinUnsupported: '浏览器预览不能设置窗口置顶。请使用 pnpm desktop dev 打开桌面应用。',
    browserWindowUnsupported: '浏览器预览不支持调整原生窗口大小或置顶；当前仅在页面内预览紧凑布局。',
    cPushPlanMismatch: 'C-Push 预生成计划回执的目标或请求身份不匹配，已忽略该回执。',
    cPushStartMismatch: 'C-Push 启动回执的目标工作树或请求身份不匹配，已忽略该回执。',
    pullRebaseMismatch: '拉取并变基回执的目标工作树或请求身份不匹配，请刷新核对实际 Git 状态。',
    cPushRespondMismatch: 'C-Push 确认回执的目标会话或请求身份不匹配，已忽略该回执。',
    modelTestMismatch: '模型连接测试回执与当前请求不匹配，已忽略该结果。',
    modelSaveMismatch: '保存的模型配置与提交参数不一致，请刷新设置核对。',
    modelSelectMismatch: '模型切换回执不匹配，请重新选择模型。',
    modelDeleteFailed: '模型删除未生效，请刷新设置后重试。',
    modelRevealMismatch: '显示密钥回执的模型标识不匹配，已忽略该回执。',
    reviewMismatch: '代码审查回执的目标或请求身份不匹配，已忽略该结果。',
    missingModelScopeTokens: '缺少 Rust 签发的文件范围凭证，请刷新变更状态后重试模型操作；提示词与选择已保留。',
    missingCPushScopeTokens: '缺少 Rust 签发的文件范围凭证，请刷新变更状态后再发起 C-Push；提示词与选择已保留。',
    missingCPushRunId: '缺少当前 C-Push 会话标识，请刷新后在消息栏重试。',
    invalidLineNumber: '跳转行号无效，行号必须从 1 开始。',
    editorReplyMismatch: '编辑器联动结果的目标或请求身份不匹配，已忽略该回执。',
    branchReplyMismatch: '分支操作结果的目标或请求身份不匹配，请刷新核对实际 Git 状态。',
    gitReadReplyMismatch: 'Git 读取结果的目标或请求身份不匹配，已忽略该结果。',
    localKeepReplyMismatch: '本地保留更新回执的目标或请求身份不匹配，请刷新核对最新状态。',
  },
  foundation: {
    phaseLabel: 'M0 · 基础验证',
    heading: '先把底层跑通',
    intro: '这不是定稿工作台。当前只验证桌面、Pi 进程和安全提交方案，不操作你的项目仓库。',
    browserPreviewNoticePrefix: '当前是浏览器预览。请运行 ',
    browserPreviewNoticeSuffix: ' 使用桌面能力。',
    piSectionHeading: 'Pi Core 独立进程',
    piStatus: {
      idle: '未验证',
      running: '验证中',
      completed: '进程已正常退出',
      cancelled: '已取消并退出',
      failed: '验证失败',
    },
    piDescription: '随包 Node + Pi SDK；脚本化响应，不调用真实模型，不读取凭据，也没有文件或终端工具。',
    verifyStreamButton: '验证流式通信',
    cancelTaskButton: '取消任务',
    streamOutputLabel: 'Pi 流式输出',
    waitingPlaceholder: '等待验证…',
    piReportStats: (pid, deltaCount, exitCode) =>
      `PID ${pid} · ${deltaCount} 个文本片段 · 退出码 ${exitCode}`,
    gitSectionHeading: 'Git 范围与草稿保全',
    gitStatus: {
      idle: '未验证',
      running: '验证中',
      done: '验证完成',
      failed: '验证失败',
    },
    gitDescription: '创建并删除临时仓库，检查部分暂存、未选已暂存文件、拆分提交与重复执行。',
    verifyTempGitButton: '验证临时 Git 仓库',
    gitCasesPassed: (gitVersion, count) => `${gitVersion} · ${count} 项通过`,
    gitCommitsSummary: (commitCount, removed) =>
      `产生 ${commitCount} 个测试提交；临时仓库${removed ? '已删除' : '未删除'}。`,
    footerLine1: '正式 UI 以 docs/design/picket-mvp.tsx 为准。',
    footerLine2: '独立 M0 诊断环境。',
  },
};

const CATALOG: Record<AppLocale, Messages> = { en, zh };

export function getMessages(locale: AppLocale = DEFAULT_APP_LOCALE): Messages {
  return CATALOG[locale] ?? CATALOG[DEFAULT_APP_LOCALE];
}
