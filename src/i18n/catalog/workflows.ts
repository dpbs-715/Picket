export interface WorkflowMessages {
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
  actions: {
    customActionBadge: string;
    previewEditAction: string;
    switchBackCPush: string;
    previewContextHint: string;
    newModalTitle: string;
    editModalTitle: string;
    manageModalTitle: string;
    repoScopedNote: string;
    creationModeLabel: string;
    modeManual: string;
    modeCreateWithPi: string;
    createShortButton: string;
    createNeedsModel: string;
    createDescribeLabel: string;
    createDescribePlaceholder: string;
    createContextHint: string;
    createInspectingStatus: string;
    createFailedFallback: string;
    createPromptRequired: string;
    generateButton: string;
    cancelGenerateButton: string;
    labelField: string;
    labelPlaceholder: string;
    promptField: string;
    promptPlaceholder: string;
    selectAfterSaveLabel: string;
    writeTrustActiveNote: string;
    resetWriteTrustButton: string;
    syncReposHint(otherRepoCount: number): string;
    syncToAllButton: string;
    syncResultSummary(added: number, updated: number, skipped: number): string;
    syncOverwriteSkippedButton(skipped: number): string;
    deleteButton: string;
    saveButton: string;
    activeBadge: string;
    autoApproveBadge: string;
    emptyManagerHint: string;
    editActionLabel(label: string): string;
    deleteActionLabel(label: string): string;
    validationLabelRequired: string;
    validationPromptRequired: string;
    validationDuplicateLabel: string;
    validationBeforeSync: string;
    actionSavedNotice(label: string): string;
    actionDeletedNotice(label: string): string;
    actionCancelledNotice: string;
  };
}

export const workflowsEn: WorkflowMessages = {
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
  actions: {
    customActionBadge: 'Custom Action',
    previewEditAction: 'Edit',
    switchBackCPush: 'C-Push',
    previewContextHint: 'Runs with current branch, checked files, and optional + Prompt instructions.',
    newModalTitle: 'New Custom Action',
    editModalTitle: 'Edit Custom Action',
    manageModalTitle: 'Manage Actions',
    repoScopedNote: 'Bound to this repository',
    creationModeLabel: 'Creation mode',
    modeManual: 'Manual',
    modeCreateWithPi: 'Create with Picket',
    createShortButton: 'Create',
    createNeedsModel: 'Configure a model first so Picket can inspect this repository and draft the button for you.',
    createDescribeLabel: 'Describe what this button should do',
    createDescribePlaceholder: 'e.g. Bump a semver git tag (v0.x.y) and push it to origin, or review open PRs on GitHub…',
    createContextHint: 'Picket inspects your remotes, branches, tags, and built-in Git skills read-only before drafting an editable button.',
    createInspectingStatus: 'Inspecting repository context…',
    createFailedFallback: 'Could not generate button draft. Check your model connection or switch to Manual.',
    createPromptRequired: 'Describe the workflow you want Picket to generate.',
    generateButton: 'Generate Draft',
    cancelGenerateButton: 'Stop',
    labelField: 'Button Label',
    labelPlaceholder: 'e.g. Release Tag, Review PR, Open PR',
    promptField: 'Prompt or Skill Command',
    promptPlaceholder: 'e.g. git tag v0.2.0 and push to origin, or create-pr to main…',
    selectAfterSaveLabel: 'Switch to this action after saving',
    writeTrustActiveNote: 'Write confirmations skipped',
    resetWriteTrustButton: 'Reset',
    syncReposHint: (count) => `${count} other repo${count === 1 ? '' : 's'}`,
    syncToAllButton: 'Sync to All Repos',
    syncResultSummary: (added, updated, skipped) => {
      const parts: string[] = [];
      if (added > 0) parts.push(`added to ${added}`);
      if (updated > 0) parts.push(`updated ${updated}`);
      if (skipped > 0) parts.push(`skipped ${skipped}`);
      return parts.length ? `Synced: ${parts.join(', ')}.` : 'Already in all other repos.';
    },
    syncOverwriteSkippedButton: (skipped) => `Overwrite ${skipped}`,
    deleteButton: 'Delete',
    saveButton: 'Save Action',
    activeBadge: 'Active',
    autoApproveBadge: 'No write prompt',
    emptyManagerHint: 'No custom actions yet.',
    editActionLabel: (label) => `Edit action ${label}`,
    deleteActionLabel: (label) => `Delete action ${label}`,
    validationLabelRequired: 'Enter a button label.',
    validationPromptRequired: 'Enter a prompt or command for this button.',
    validationDuplicateLabel: 'Another button in this repository already uses that label.',
    validationBeforeSync: 'Fill in both the label and prompt before syncing to other repositories.',
    actionSavedNotice: (label) => `Saved custom action "${label}".`,
    actionDeletedNotice: (label) => `Deleted custom action "${label}".`,
    actionCancelledNotice: 'Cancelled running custom action.',
  },
};

export const workflowsZh: WorkflowMessages = {
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
  actions: {
    customActionBadge: '自定义按钮',
    previewEditAction: '编辑',
    switchBackCPush: '切回 C-Push',
    previewContextHint: '点击主按钮将自动附带当前分支、已选文件范围以及可选的 + 提示词。',
    newModalTitle: '新建自定义按钮',
    editModalTitle: '编辑自定义按钮',
    manageModalTitle: '管理自定义按钮',
    repoScopedNote: '仅绑定当前仓库',
    creationModeLabel: '创建方式',
    modeManual: '手动填写',
    modeCreateWithPi: '跟 Picket 对话创建',
    createShortButton: '对话创建',
    createNeedsModel: '请先配置模型，Picket 才能探测当前仓库上下文并帮你生成按钮草稿。',
    createDescribeLabel: '用自然语言描述你想要的一键流程',
    createDescribePlaceholder: '例如：帮我建个发版打 semver tag 并推送 origin 的按钮，或一键审查 GitHub PR…',
    createContextHint: 'Picket 将只读探测当前仓库远端、分支、Tag 习惯和内置 Git 技能，生成后可自由微调再保存。',
    createInspectingStatus: '正在探测仓库上下文…',
    createFailedFallback: '生成按钮草稿失败，请检查模型连接或切到手动填写。',
    createPromptRequired: '请先描述你希望这个按钮执行的流程。',
    generateButton: '生成草稿',
    cancelGenerateButton: '停止生成',
    labelField: '按钮名称',
    labelPlaceholder: '例如：发版打 Tag、审 PR、提 PR',
    promptField: '预设指令或提示词',
    promptPlaceholder: '例如：git tag v0.2.0 && git push origin v0.2.0，或 create-pr to main…',
    selectAfterSaveLabel: '保存后切换到此按钮',
    writeTrustActiveNote: '已开启写入免确认',
    resetWriteTrustButton: '重置',
    syncReposHint: (count) => `其他 ${count} 个仓库`,
    syncToAllButton: '同步到所有仓库',
    syncResultSummary: (added, updated, skipped) => {
      const parts: string[] = [];
      if (added > 0) parts.push(`新增 ${added} 个`);
      if (updated > 0) parts.push(`覆盖 ${updated} 个`);
      if (skipped > 0) parts.push(`跳过 ${skipped} 个`);
      return parts.length ? `已同步：${parts.join('，')}。` : '其他仓库均已存在。';
    },
    syncOverwriteSkippedButton: (skipped) => `覆盖 ${skipped} 个`,
    deleteButton: '删除',
    saveButton: '保存按钮',
    activeBadge: '当前选中',
    autoApproveBadge: '写入免确认',
    emptyManagerHint: '暂无自定义按钮。',
    editActionLabel: (label) => `编辑按钮 ${label}`,
    deleteActionLabel: (label) => `删除按钮 ${label}`,
    validationLabelRequired: '请填写按钮名称。',
    validationPromptRequired: '请填写预设指令或提示词。',
    validationDuplicateLabel: '当前仓库已存在同名按钮，请换一个名称。',
    validationBeforeSync: '请先填好按钮名称和指令再同步到其他仓库。',
    actionSavedNotice: (label) => `已保存自定义按钮「${label}」。`,
    actionDeletedNotice: (label) => `已删除自定义按钮「${label}」。`,
    actionCancelledNotice: '已取消正在执行的自定义按钮。',
  },
};
