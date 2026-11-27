export interface DesktopMessages {
  desktop: {
    disconnectedModelOrCPush: string;
    disconnectedGit: string;
    disconnectedTestModel: string;
    disconnectedSaveModel: string;
    disconnectedRevealModelKey: string;
    browserPinUnsupported: string;
    browserWindowUnsupported: string;
    disconnectedUpdater: string;
    disconnectedSkills: string;
    cPushStartMismatch: string;
    pullRebaseMismatch: string;
    cPushRespondMismatch: string;
    modelTestMismatch: string;
    modelSaveMismatch: string;
    modelSelectMismatch: string;
    modelDeleteFailed: string;
    modelRevealMismatch: string;
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
  tray: {
    ariaLabel: string;
    refresh: string;
    settings: string;
    emptyTitle: string;
    emptySubtitle: string;
    clean: string;
    changesCount(count: number): string;
    localKeepOnly(count: number): string;
    statusUnavailable: string;
    detachedHead: string;
    aheadBehind(ahead: number, behind: number): string;
    statusRunning: string;
    statusNeedsDecision: string;
    statusConflict: string;
    statusWaitingManual: string;
    statusPushFailed: string;
    foldedCleanRow(count: number): string;
    openRepositoryHint(name: string): string;
    openMainWindow: string;
    quit: string;
  };
}

export const desktopEn: DesktopMessages = {
  desktop: {
    disconnectedModelOrCPush: 'Model and C-Push services are not connected in browser preview; prompt and selection are preserved.',
    disconnectedGit: 'Desktop Git service is not connected in browser preview; no commits, pushes, or repository changes were made, and prompt and selection are preserved.',
    disconnectedTestModel: 'Desktop model service is not connected; no connection request was sent, and input is preserved.',
    disconnectedSaveModel: 'Desktop credential storage is not connected in browser preview; no configuration or API key was saved, and input is preserved.',
    disconnectedRevealModelKey: 'System authentication and desktop credential storage are unavailable in browser preview; no API key was read.',
    browserPinUnsupported: 'Window pinning is unavailable in browser preview. Run pnpm desktop dev to open the desktop app.',
    browserWindowUnsupported: 'Window resizing and pinning are unavailable in browser preview; compact layout is previewed inside the page.',
    disconnectedUpdater: 'Desktop update service is unavailable in browser preview.',
    disconnectedSkills: 'Desktop skill sync is unavailable in browser preview.',
    cPushStartMismatch: 'C-Push start reply target or request identity mismatch; ignored reply.',
    pullRebaseMismatch: 'Pull & rebase reply target or request identity mismatch. Refresh to verify Git status.',
    cPushRespondMismatch: 'C-Push response reply session or request identity mismatch; ignored reply.',
    modelTestMismatch: 'Model connection test reply did not match the active request; ignored result.',
    modelSaveMismatch: 'Saved model configuration did not match submitted parameters. Reopen Settings to verify.',
    modelSelectMismatch: 'Model selection reply mismatch. Select the model again.',
    modelDeleteFailed: 'Model deletion did not take effect. Reopen Settings and try again.',
    modelRevealMismatch: 'Revealed API key reply did not match the requested model; ignored reply.',
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
  tray: {
    ariaLabel: 'Picket Menu Bar Status',
    refresh: 'Refresh repositories',
    settings: 'Open Settings',
    emptyTitle: 'No repositories opened yet',
    emptySubtitle: 'Open the main window to add a local Git repository.',
    clean: 'Working tree clean',
    changesCount: (count) => (count === 1 ? '1 change' : `${count} changes`),
    localKeepOnly: (count) => `${count} local keep only`,
    statusUnavailable: 'Status unavailable',
    detachedHead: 'Detached HEAD',
    aheadBehind: (ahead, behind) => {
      const parts: string[] = [];
      if (ahead > 0) parts.push(`↑${ahead}`);
      if (behind > 0) parts.push(`↓${behind}`);
      return parts.join(' ');
    },
    statusRunning: 'Running…',
    statusNeedsDecision: 'Needs decision',
    statusConflict: 'Merge conflict',
    statusWaitingManual: 'Waiting for IDE merge',
    statusPushFailed: 'Push failed',
    foldedCleanRow: (count) =>
      count === 1 ? '1 other clean workspace' : `${count} other clean workspaces`,
    openRepositoryHint: (name) => `Open ${name} in Picket`,
    openMainWindow: 'Open Main Window',
    quit: 'Quit',
  },
};

export const desktopZh: DesktopMessages = {
  desktop: {
    disconnectedModelOrCPush: '当前环境未连接桌面模型或 C-Push 服务，此操作尚未接入浏览器预览；提示词和选择已保留。',
    disconnectedGit: '当前环境未连接桌面 Git 服务，此操作尚未接入浏览器预览；不会提交、推送或修改仓库，提示词和选择已保留。',
    disconnectedTestModel: '当前环境未连接桌面模型服务，未发送连接请求；输入已保留。',
    disconnectedSaveModel: '当前环境未连接桌面凭据服务，没有保存配置或密钥；输入已保留。',
    disconnectedRevealModelKey: '当前环境未连接系统身份验证与本地凭据服务，无法显示密钥。',
    browserPinUnsupported: '浏览器预览不能设置窗口置顶。请使用 pnpm desktop dev 打开桌面应用。',
    browserWindowUnsupported: '浏览器预览不支持调整原生窗口大小或置顶；当前仅在页面内预览紧凑布局。',
    disconnectedUpdater: '浏览器预览未连接桌面更新服务。',
    disconnectedSkills: '浏览器预览未连接桌面技能同步服务。',
    cPushStartMismatch: 'C-Push 启动回执的目标工作树或请求身份不匹配，已忽略该回执。',
    pullRebaseMismatch: '拉取并变基回执的目标工作树或请求身份不匹配，请刷新核对实际 Git 状态。',
    cPushRespondMismatch: 'C-Push 确认回执的目标会话或请求身份不匹配，已忽略该回执。',
    modelTestMismatch: '模型连接测试回执与当前请求不匹配，已忽略该结果。',
    modelSaveMismatch: '保存的模型配置与提交参数不一致，请刷新设置核对。',
    modelSelectMismatch: '模型切换回执不匹配，请重新选择模型。',
    modelDeleteFailed: '模型删除未生效，请刷新设置后重试。',
    modelRevealMismatch: '显示密钥回执的模型标识不匹配，已忽略该回执。',
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
  tray: {
    ariaLabel: 'Picket 状态栏概览',
    refresh: '刷新仓库状态',
    settings: '打开设置',
    emptyTitle: '尚未打开仓库',
    emptySubtitle: '打开主窗口以添加本机 Git 仓库。',
    clean: '工作区干净',
    changesCount: (count) => `${count} 项变更`,
    localKeepOnly: (count) => `仅 ${count} 项本地保留`,
    statusUnavailable: '状态不可用',
    detachedHead: '分离 HEAD',
    aheadBehind: (ahead, behind) => {
      const parts: string[] = [];
      if (ahead > 0) parts.push(`↑${ahead}`);
      if (behind > 0) parts.push(`↓${behind}`);
      return parts.join(' ');
    },
    statusRunning: '执行中…',
    statusNeedsDecision: '待你决定',
    statusConflict: '有代码冲突',
    statusWaitingManual: '等待外部合并',
    statusPushFailed: '推送失败',
    foldedCleanRow: (count) => `其余 ${count} 个工作区干净`,
    openRepositoryHint: (name) => `在主窗口打开 ${name}`,
    openMainWindow: '打开主窗口',
    quit: '退出',
  },
};
