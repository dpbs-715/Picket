export interface RunnerMessages {
  run: {
    gitTabLabel: string;
    runTabLabel: string;
    viewSwitchLabel: string;
    panelAriaLabel: string;
    panelTitle: string;
    panelSummary: (total: number, running: number) => string;
    openLogsHint: string;
    buttonLabel: string;
    runningBadgeTitle: (count: number) => string;
    popoverTitle: string;
    emptyTitle: string;
    aiDiscoverButton: string;
    aiRediscoverButton: string;
    addManualButton: string;
    sharedEnvButton: string;
    sharedEnvButtonActive: (count: number) => string;
    sharedEnvModalTitle: string;
    sharedEnvDescription: string;
    sharedEnvPresetsLabel: string;
    sharedEnvField: string;
    sharedEnvPlaceholder: string;
    sharedEnvClearButton: string;
    sharedEnvSaveButton: string;
    sharedEnvSavedNotice: (count: number) => string;
    refreshEnvButton: string;
    refreshEnvDone: (shell: string, pathCount: number) => string;
    filterPlaceholder: string;
    filterClearAction: string;
    filterEmptyResult: (query: string) => string;
    runningSectionTitle: string;
    ungroupedSectionTitle: string;
    groupRunningBadge: (count: number) => string;
    startAction: string;
    stopAction: string;
    stopAllAction: string;
    restartAction: string;
    logsAction: string;
    editAction: string;
    deleteAction: string;
    statusStarting: string;
    statusRunning: string;
    statusStopping: string;
    statusExited: (code?: number | null) => string;
    statusFailed: (code?: number | null) => string;
    statusIdle: string;
    modalNewTitle: string;
    modalEditTitle: string;
    iconField: string;
    iconAutoOption: string;
    iconLabels: Record<string, string>;
    labelField: string;
    labelPlaceholder: string;
    groupField: string;
    groupPlaceholder: string;
    commandField: string;
    commandPlaceholder: string;
    cwdField: string;
    cwdPlaceholder: string;
    envField: string;
    envPlaceholder: string;
    saveButton: string;
    validationLabelRequired: string;
    validationCommandRequired: string;
    validationDuplicateLabel: string;
    validationInvalidGroup: string;
    validationInvalidCwd: string;
    validationInvalidEnv: string;
    validationInvalidCharacters: string;
    validationLimitReached: string;
    deleteRunningConfirmTitle: string;
    deleteRunningConfirmMessage: (count: number) => string;
    deleteRunningConfirmAction: string;
    deleteRunningStopping: string;
    discoverModalTitle: string;
    discoverNoModelNote: string;
    discoverConfigureModel: string;
    discoverScanning: string;
    discoverScanStepsTitle: string;
    discoverCancel: string;
    discoverRescan: string;
    discoverSelectAll: string;
    discoverEditDone: string;
    discoverAddSelected: (count: number) => string;
    discoverAlreadyAdded: string;
    discoverEmptyResult: string;
    discoverAddedNotice: (count: number) => string;
    logsWindowTitle: string;
    logsEmptyTitle: string;
    logsEmptyDescription: string;
    logsClearButton: string;
    logsFollowBottom: string;
    logsNoOutputYet: string;
    logsViewPreviousHint: string;
    logsOpenUrlTitle: (url: string) => string;
    compactEmptyHint: string;
    compactManageInMain: string;
    trayRunningSectionTitle: (count: number) => string;
    trayStopButton: string;
    trayOpenLogsTitle: string;
    startedNotice: (label: string) => string;
    stoppedNotice: (label: string) => string;
    stoppedAllNotice: (count: number) => string;
    restartedNotice: (label: string) => string;
    savedNotice: (label: string) => string;
    deletedNotice: (label: string) => string;
  };
}

export const runnerEn: RunnerMessages = {
  run: {
    gitTabLabel: 'Git',
    runTabLabel: 'Run',
    viewSwitchLabel: 'Workbench view',
    panelAriaLabel: 'Run services',
    panelTitle: 'Run Services',
    panelSummary: (total, running) => (running > 0 ? `${total} commands · ${running} running` : `${total} command${total === 1 ? '' : 's'}`),
    openLogsHint: 'Open live logs window',
    buttonLabel: 'Run',
    runningBadgeTitle: (count) => `${count} service${count === 1 ? '' : 's'} running`,
    popoverTitle: 'Run Commands',
    emptyTitle: 'No run commands yet',
    aiDiscoverButton: 'Scan with AI',
    aiRediscoverButton: 'AI Scan',
    addManualButton: 'Add Command',
    sharedEnvButton: 'Shared Env',
    sharedEnvButtonActive: (count) => `Shared Env (${count})`,
    sharedEnvModalTitle: 'Repository Shared Env',
    sharedEnvDescription: 'Shared across every run command in this repository (individual command env overrides these). Setting JAVA_HOME automatically prepends <JAVA_HOME>/bin to PATH.',
    sharedEnvPresetsLabel: 'Quick insert environment variables',
    sharedEnvField: 'Shared Variables (KEY=VALUE)',
    sharedEnvPlaceholder: 'JAVA_HOME=/Users/you/Library/Java/JavaVirtualMachines/jbr-25/Contents/Home\nSPRING_PROFILES_ACTIVE=dev\nMAVEN_OPTS=-Xmx1024m',
    sharedEnvClearButton: 'Clear',
    sharedEnvSaveButton: 'Save Shared Env',
    sharedEnvSavedNotice: (count) => (count > 0 ? `Saved ${count} shared env variable${count === 1 ? '' : 's'} for this repository.` : 'Cleared repository shared env variables.'),
    refreshEnvButton: 'Refresh Shell Env',
    refreshEnvDone: (shell, pathCount) => `Reloaded user environment from ${shell} (${pathCount} PATH entries).`,
    filterPlaceholder: 'Filter services, groups, or commands...',
    filterClearAction: 'Clear filter',
    filterEmptyResult: (query) => `No services match "${query}".`,
    runningSectionTitle: 'Running Now',
    ungroupedSectionTitle: 'Other',
    groupRunningBadge: (count) => `${count} running`,
    startAction: 'Start',
    stopAction: 'Stop',
    stopAllAction: 'Stop All',
    restartAction: 'Restart',
    logsAction: 'Logs',
    editAction: 'Edit',
    deleteAction: 'Delete',
    statusStarting: 'Starting',
    statusRunning: 'Running',
    statusStopping: 'Stopping',
    statusExited: (code) => (code !== undefined && code !== null ? `Exited (${code})` : 'Stopped'),
    statusFailed: (code) => (code !== undefined && code !== null ? `Failed (${code})` : 'Failed'),
    statusIdle: 'Idle',
    modalNewTitle: 'New Run Command',
    modalEditTitle: 'Edit Run Command',
    iconField: 'Icon',
    iconAutoOption: 'Auto',
    iconLabels: {
      node: 'Node.js',
      pnpm: 'pnpm / Package',
      vite: 'Frontend / Vite',
      java: 'Java / JVM',
      spring: 'Spring Boot',
      rust: 'Rust / Cargo',
      python: 'Python',
      go: 'Go',
      docker: 'Docker',
      desktop: 'Desktop App',
      database: 'Database',
      terminal: 'Shell',
    },
    labelField: 'Name',
    labelPlaceholder: 'e.g. Dev Server, Backend API, Desktop Dev',
    groupField: 'Group (optional)',
    groupPlaceholder: 'e.g. Maven, Spring Boot, Frontend',
    commandField: 'Shell Command',
    commandPlaceholder: 'e.g. pnpm desktop dev --no-watch',
    cwdField: 'Working Directory (optional)',
    cwdPlaceholder: 'Repository root by default, e.g. apps/web',
    envField: 'Environment Variables (optional)',
    envPlaceholder: 'PORT=3000\nNODE_ENV=development',
    saveButton: 'Save Command',
    validationLabelRequired: 'Enter a name for this run command.',
    validationCommandRequired: 'Enter the command to execute.',
    validationDuplicateLabel: 'Another run command in this repository already uses that name.',
    validationInvalidGroup: 'Group name must be a single line (max 48 characters).',
    validationInvalidCwd: 'Working directory must be a relative subdirectory inside this repository.',
    validationInvalidEnv: 'Invalid environment entry. Use KEY=VALUE with valid variable names.',
    validationInvalidCharacters: 'Name and command cannot contain NUL characters.',
    validationLimitReached: 'This repository reached the maximum of 64 run commands.',
    deleteRunningConfirmTitle: 'Stop running service and delete?',
    deleteRunningConfirmMessage: (count) => (
      count === 1
        ? 'This command is currently running in 1 worktree. Stop it and delete this command?'
        : `This command is currently running in ${count} worktrees. Stop all running instances and delete this command?`
    ),
    deleteRunningConfirmAction: 'Stop & Delete',
    deleteRunningStopping: 'Stopping...',
    discoverModalTitle: 'Discover Run Commands',
    discoverNoModelNote: 'Configure a model first to scan with AI, or add commands manually.',
    discoverConfigureModel: 'Configure Model',
    discoverScanning: 'Scanning project manifests...',
    discoverScanStepsTitle: 'Inspecting workspace',
    discoverCancel: 'Cancel',
    discoverRescan: 'Scan Again',
    discoverSelectAll: 'Select all',
    discoverEditDone: 'Done',
    discoverAddSelected: (count) => `Add Selected (${count})`,
    discoverAlreadyAdded: 'Already added',
    discoverEmptyResult: 'No runnable commands were detected automatically. You can add commands manually.',
    discoverAddedNotice: (count) => `Added ${count} run command${count === 1 ? '' : 's'}.`,
    logsWindowTitle: 'Run Logs',
    logsEmptyTitle: 'No active or recent runs',
    logsEmptyDescription: 'Start a command from Picket’s Run tab and click Logs to inspect live output here.',
    logsClearButton: 'Clear',
    logsFollowBottom: 'Jump to latest',
    logsNoOutputYet: 'Waiting for output...',
    logsViewPreviousHint: 'View run logs',
    logsOpenUrlTitle: (url) => `Open ${url} in browser`,
    compactEmptyHint: 'No run commands configured for this repository.',
    compactManageInMain: 'Open Run Panel',
    trayRunningSectionTitle: (count) => `Running Services (${count})`,
    trayStopButton: 'Stop',
    trayOpenLogsTitle: 'Open Run Logs',
    startedNotice: (label) => `Started ${label}.`,
    stoppedNotice: (label) => `Stopped ${label}.`,
    stoppedAllNotice: (count) => `Stopped ${count} running service${count === 1 ? '' : 's'}.`,
    restartedNotice: (label) => `Restarted ${label}.`,
    savedNotice: (label) => `Saved run command "${label}".`,
    deletedNotice: (label) => `Deleted run command "${label}".`,
  },
};

export const runnerZh: RunnerMessages = {
  run: {
    gitTabLabel: 'Git',
    runTabLabel: '运行',
    viewSwitchLabel: '工作台视图切换',
    panelAriaLabel: '运行服务列表',
    panelTitle: '本地服务',
    panelSummary: (total, running) => (running > 0 ? `共 ${total} 项 · ${running} 运行中` : `共 ${total} 项服务`),
    openLogsHint: '点击弹出运行日志窗口',
    buttonLabel: '运行',
    runningBadgeTitle: (count) => `当前有 ${count} 个服务正在运行`,
    popoverTitle: '运行命令',
    emptyTitle: '暂未配置运行命令',
    aiDiscoverButton: 'AI 扫描项目',
    aiRediscoverButton: 'AI 扫描',
    addManualButton: '手动添加',
    sharedEnvButton: '共享环境变量',
    sharedEnvButtonActive: (count) => `共享环境变量 (${count})`,
    sharedEnvModalTitle: '仓库共享环境变量',
    sharedEnvDescription: '对当前仓库所有运行命令生效（单条命令中的同名变量可覆盖此处）。配置 JAVA_HOME 时会自动将 <JAVA_HOME>/bin 放到 PATH 最前。',
    sharedEnvPresetsLabel: '快捷插入常用变量',
    sharedEnvField: '共享变量（每行 KEY=VALUE）',
    sharedEnvPlaceholder: 'JAVA_HOME=/Users/you/Library/Java/JavaVirtualMachines/jbr-25/Contents/Home\nSPRING_PROFILES_ACTIVE=dev\nMAVEN_OPTS=-Xmx1024m',
    sharedEnvClearButton: '清空',
    sharedEnvSaveButton: '保存共享环境',
    sharedEnvSavedNotice: (count) => (count > 0 ? `已为当前仓库保存 ${count} 条共享环境变量。` : '已清空当前仓库的共享环境变量。'),
    refreshEnvButton: '刷新终端环境',
    refreshEnvDone: (shell, pathCount) => `已重新加载本机 ${shell} 环境（共 ${pathCount} 条 PATH）。`,
    filterPlaceholder: '快速过滤服务名、分组或命令...',
    filterClearAction: '清空过滤',
    filterEmptyResult: (query) => `没有匹配「${query}」的服务。`,
    runningSectionTitle: '运行中',
    ungroupedSectionTitle: '其他',
    groupRunningBadge: (count) => `${count} 运行中`,
    startAction: '启动',
    stopAction: '停止',
    stopAllAction: '全部停止',
    restartAction: '重启',
    logsAction: '日志',
    editAction: '编辑',
    deleteAction: '删除',
    statusStarting: '启动中',
    statusRunning: '运行中',
    statusStopping: '停止中',
    statusExited: (code) => (code !== undefined && code !== null ? `已退出 (${code})` : '已停止'),
    statusFailed: (code) => (code !== undefined && code !== null ? `异常退出 (${code})` : '启动失败'),
    statusIdle: '未运行',
    modalNewTitle: '新建运行命令',
    modalEditTitle: '编辑运行命令',
    iconField: '图标',
    iconAutoOption: '自动识别',
    iconLabels: {
      node: 'Node.js',
      pnpm: 'pnpm / 包脚本',
      vite: '前端 / Vite',
      java: 'Java',
      spring: 'Spring',
      rust: 'Rust',
      python: 'Python',
      go: 'Go',
      docker: 'Docker',
      desktop: '桌面应用',
      database: '数据库',
      terminal: '终端命令',
    },
    labelField: '名称',
    labelPlaceholder: '例如：前端开发、后端服务、桌面端开发',
    groupField: '分组（可选）',
    groupPlaceholder: '例如：Maven、Spring Boot、前端',
    commandField: '启动命令',
    commandPlaceholder: '例如：pnpm desktop dev --no-watch',
    cwdField: '工作目录（可选）',
    cwdPlaceholder: '默认仓库根目录，例如 apps/web',
    envField: '额外环境变量（可选）',
    envPlaceholder: 'PORT=3000\nNODE_ENV=development',
    saveButton: '保存命令',
    validationLabelRequired: '请输入运行命令的名称。',
    validationCommandRequired: '请输入要执行的命令。',
    validationDuplicateLabel: '当前仓库已有同名的运行命令。',
    validationInvalidGroup: '分组名称须为单行文本且不超过 48 个字符。',
    validationInvalidCwd: '工作目录必须是当前仓库内部的相对路径。',
    validationInvalidEnv: '环境变量格式无效，请每行按 KEY=VALUE 填写合法变量名。',
    validationInvalidCharacters: '名称和命令中不能包含非法空字符。',
    validationLimitReached: '当前仓库已达到最多 64 条运行命令的上限。',
    deleteRunningConfirmTitle: '停止运行中的服务并删除？',
    deleteRunningConfirmMessage: (count) => `该命令当前正在 ${count} 个工作区中运行。是否先停止所有运行中的实例再删除？`,
    deleteRunningConfirmAction: '停止并删除',
    deleteRunningStopping: '正在停止...',
    discoverModalTitle: 'AI 扫描项目运行命令',
    discoverNoModelNote: '请先配置模型再使用 AI 扫描，或直接手动添加。',
    discoverConfigureModel: '配置模型',
    discoverScanning: '正在扫描项目清单...',
    discoverScanStepsTitle: '正在分析项目',
    discoverCancel: '取消',
    discoverRescan: '重新扫描',
    discoverSelectAll: '全选',
    discoverEditDone: '完成',
    discoverAddSelected: (count) => `添加所选 (${count})`,
    discoverAlreadyAdded: '已添加',
    discoverEmptyResult: '未自动识别到可运行的命令，你可以手动添加。',
    discoverAddedNotice: (count) => `已添加 ${count} 条运行命令。`,
    logsWindowTitle: '运行日志',
    logsEmptyTitle: '暂无运行中的服务',
    logsEmptyDescription: '在 Picket 的「运行」视图启动命令后，点击「日志」即可在此窗口查看实时输出。',
    logsClearButton: '清空',
    logsFollowBottom: '滚动到底部',
    logsNoOutputYet: '等待日志输出...',
    logsViewPreviousHint: '查看本次日志',
    logsOpenUrlTitle: (url) => `在浏览器中打开 ${url}`,
    compactEmptyHint: '当前仓库暂未配置运行命令。',
    compactManageInMain: '打开运行面板',
    trayRunningSectionTitle: (count) => `运行中服务 (${count})`,
    trayStopButton: '停止',
    trayOpenLogsTitle: '查看运行日志',
    startedNotice: (label) => `已启动「${label}」。`,
    stoppedNotice: (label) => `已停止「${label}」。`,
    stoppedAllNotice: (count) => `已停止全部 ${count} 个运行中服务。`,
    restartedNotice: (label) => `已重启「${label}」。`,
    savedNotice: (label) => `已保存运行命令「${label}」。`,
    deletedNotice: (label) => `已删除运行命令「${label}」。`,
  },
};
