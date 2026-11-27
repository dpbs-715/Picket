export interface WorkflowMessages {
  commit: {
    promptToggle: string;
    promptActiveToggle: string;
    promptTooltip: string;
    promptActiveTooltip: string;
    hidePromptLabel: string;
    clearPromptLabel: string;
    promptLabel: string;
    promptPlaceholder: string;
    configureModelButton: string;
  };
  actions: {
    customActionBadge: string;
    previewEditAction: string;
    switchBackCPush: string;
    expandActionPreview: string;
    collapseActionPreview: string;
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
    promptToggle: '+ Prompt',
    promptActiveToggle: 'Prompt',
    promptTooltip: 'Add extra instructions (optional)',
    promptActiveTooltip: 'Extra instructions active (click to focus)',
    hidePromptLabel: 'Hide Extra Instructions',
    clearPromptLabel: 'Clear Extra Instructions',
    promptLabel: 'Extra instructions',
    promptPlaceholder: 'Optional instructions, e.g. split lockfile separately…',
    configureModelButton: 'Configure Model',
  },
  actions: {
    customActionBadge: 'Custom Action',
    previewEditAction: 'Edit',
    switchBackCPush: 'C-Push',
    expandActionPreview: 'Show action prompt',
    collapseActionPreview: 'Hide action prompt',
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
    promptToggle: '+ 提示词',
    promptActiveToggle: '提示词',
    promptTooltip: '补充提示词（可选）',
    promptActiveTooltip: '已填写补充提示词（点击聚焦）',
    hidePromptLabel: '收起扩展提示词',
    clearPromptLabel: '清空扩展提示词',
    promptLabel: '扩展提示词',
    promptPlaceholder: '补充要求（可选），如：lock 文件单独提交…',
    configureModelButton: '配置模型',
  },
  actions: {
    customActionBadge: '自定义按钮',
    previewEditAction: '编辑',
    switchBackCPush: '切回 C-Push',
    expandActionPreview: '展开按钮指令',
    collapseActionPreview: '收起按钮指令',
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
