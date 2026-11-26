import { MODEL_TOKEN_LIMITS, type TokenLimitValidationCode } from '../../../shared/model-defaults';

export interface SettingsMessages {
  settings: {
    title: string;
    tabsLabel: string;
    tabs: {
      general: string;
      models: string;
      skills: string;
    };
    appSectionHeading: string;
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
    skills: {
      heading: string;
      modeBundled: string;
      modeSynced: string;
      syncButton: string;
      syncingButton: string;
      restoreBundledButton: string;
      restoringButton: string;
      sourceRepoLabel: string;
      editSourceButton: string;
      cancelEditSource: string;
      resetSourceDefault: string;
      sourceRepoPlaceholder: string;
      commitLabel(shortSha: string): string;
      syncedAtLabel(formatted: string): string;
      bundledSummary: string;
      enabledSummary(enabled: number, total: number): string;
      toggleListLabel(expanded: boolean): string;
      badgeSynced: string;
      badgeBuiltIn: string;
      badgeCustom: string;
      refsCount(count: number): string;
      bundledReadonlyHint: string;
      nextRunHint: string;
      syncSuccessNotice(count: number, shortSha: string): string;
      resetSuccessNotice: string;
      syncErrorFallback: string;
      toggleErrorFallback: string;
      resetErrorFallback: string;
    };
    updates: {
      heading: string;
      currentVersion(version: string): string;
      checkButton: string;
      checking: string;
      upToDate: string;
      availableTitle(version: string): string;
      updateAndRestart: string;
      downloading(percent: number, downloadedMb: string, totalMb: string): string;
      downloadingUnknown(downloadedMb: string): string;
      installing: string;
      downloadFromGithub: string;
      retry: string;
      gitBusyNote: string;
      blockers: {
        devBuild: string;
        readOnlyVolume: string;
        appTranslocation: string;
        signingKeyUnconfigured: string;
      };
    };
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
    fetchModels: string;
    refreshModels: string;
    fetchingModels: string;
    fetchedModelsCount(count: number): string;
    fetchedModelsEmpty: string;
    fetchModelsErrorFallback: string;
    toggleFetchedModels(count: number): string;
    fetchedModelsListLabel: string;
    noMatchingFetchedModels: string;
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
}

export const settingsEn: SettingsMessages = {
  settings: {
    title: 'Settings',
    tabsLabel: 'Settings sections',
    tabs: {
      general: 'General',
      models: 'Models',
      skills: 'Skills',
    },
    appSectionHeading: 'App',
    currentProject: 'Current Project',
    projectDefaults: 'Project Defaults',
    noProjectBadge: 'No repo open',
    editorIntegration: 'Editor',
    editorIntegrationTipLabel: 'Editor integration info',
    editorIntegrationTip: (cli, hasRepository) =>
      `Uses ${cli} for ${hasRepository ? 'this project' : 'new projects'}.`,
    noEditorsDetected: 'No supported editors detected',
    noEditorsHelp: 'Install IDEA, RustRover, WebStorm, VS Code, or Cursor.',
    preferredEditor: 'Preferred Editor',
    commitLanguage: 'Commit Language',
    commitLanguageTipLabel: 'Commit message language info',
    commitLanguageTip: (hasRepository) =>
      `Commit message language for ${hasRepository ? 'this project' : 'new projects'}.`,
    appLanguage: 'Language',
    modelsHeading: 'Models',
    addProvider: 'Provider',
    addChildModel: 'Model',
    noModelsConfigured: 'No models yet',
    addFirstProvider: 'Add First Provider',
    activeModelBadge: 'Active',
    connectionModelCount: (count) => `${count} model${count === 1 ? '' : 's'}`,
    emptyConnectionHint: 'No models yet.',
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
    skills: {
      heading: 'Skills',
      modeBundled: 'Built-in',
      modeSynced: 'Synced',
      syncButton: 'Sync',
      syncingButton: 'Syncing…',
      restoreBundledButton: 'Restore Built-in',
      restoringButton: 'Restoring…',
      sourceRepoLabel: 'Skill source repository',
      editSourceButton: 'Edit',
      cancelEditSource: 'Cancel',
      resetSourceDefault: 'Default',
      sourceRepoPlaceholder: 'https://github.com/dpbs-715/skills',
      commitLabel: (shortSha) => shortSha,
      syncedAtLabel: (formatted) => formatted,
      bundledSummary: 'Built-in',
      enabledSummary: (enabled, total) => `${enabled}/${total}`,
      toggleListLabel: (expanded) => (expanded ? 'Collapse skills list' : 'Expand skills list'),
      badgeSynced: 'Synced',
      badgeBuiltIn: 'Built-in',
      badgeCustom: 'Custom',
      refsCount: (count) => `+${count}`,
      bundledReadonlyHint: '',
      nextRunHint: '',
      syncSuccessNotice: (_count, shortSha) => `Synced (${shortSha}).`,
      resetSuccessNotice: 'Restored built-in skills.',
      syncErrorFallback: 'Sync failed; existing skills kept.',
      toggleErrorFallback: 'Unable to update skills.',
      resetErrorFallback: 'Unable to restore built-in skills.',
    },
    updates: {
      heading: 'Version',
      currentVersion: (version) => `v${version}`,
      checkButton: 'Check for Updates',
      checking: 'Checking…',
      upToDate: 'Up to date',
      availableTitle: (version) => `v${version} available`,
      updateAndRestart: 'Update & Restart',
      downloading: (percent, downloadedMb, totalMb) => `Downloading… ${percent}% (${downloadedMb} / ${totalMb} MB)`,
      downloadingUnknown: (downloadedMb) => `Downloading… ${downloadedMb} MB`,
      installing: 'Installing & restarting…',
      downloadFromGithub: 'Download on GitHub',
      retry: 'Retry',
      gitBusyNote: 'Wait for the active Git task to finish first.',
      blockers: {
        devBuild: 'Requires the packaged macOS app bundle.',
        readOnlyVolume: 'Move Picket.app to Applications first.',
        appTranslocation: 'Move Picket.app to Applications and reopen.',
        signingKeyUnconfigured: 'Signing key not configured; download on GitHub.',
      },
    },
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
    connectionNamePlaceholder: 'Optional',
    parentConnectionLabel: 'Connection',
    connectionIdentityLockedNote: '',
    rotateKeyImpactNote: (count) => `Updates all ${count} model${count === 1 ? '' : 's'} under this connection.`,
    testNeedsModelHint: 'Add a model first to test.',
    advancedToggle: 'Advanced',
    apiFormatLabel: 'API Format',
    displayNameLabel: 'Display Name',
    displayNamePlaceholder: 'Optional',
    modelIdLabel: 'Model ID',
    fetchModels: 'Fetch Models',
    refreshModels: 'Refresh',
    fetchingModels: 'Fetching…',
    fetchedModelsCount: (count) => `${count} models`,
    fetchedModelsEmpty: 'No models returned; enter manually.',
    fetchModelsErrorFallback: 'Unable to fetch models; enter manually.',
    toggleFetchedModels: (count) => `Toggle fetched models list (${count})`,
    fetchedModelsListLabel: 'Fetched provider models',
    noMatchingFetchedModels: 'No matches — Enter uses custom ID.',
    baseUrlLabel: 'Base URL',
    apiKeyTipLabel: 'API Key storage info',
    apiKeyTipText: 'Saved locally in ~/.picket/credentials.json (0600).',
    apiKeyKeepPlaceholder: 'Leave blank to keep',
    apiKeyEnterPlaceholder: 'API key',
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
};

export const settingsZh: SettingsMessages = {
  settings: {
    title: '设置',
    tabsLabel: '设置分区',
    tabs: {
      general: '通用',
      models: '模型',
      skills: '技能',
    },
    appSectionHeading: '应用',
    currentProject: '当前项目',
    projectDefaults: '默认项目配置',
    noProjectBadge: '未打开仓库',
    editorIntegration: '编辑器',
    editorIntegrationTipLabel: '编辑器联动说明',
    editorIntegrationTip: (cli, hasRepository) =>
      `${hasRepository ? '当前项目' : '新项目'}使用 ${cli}。`,
    noEditorsDetected: '未检测到受支持编辑器',
    noEditorsHelp: '请安装 IDEA、RustRover、WebStorm、VS Code 或 Cursor。',
    preferredEditor: '首选编辑器',
    commitLanguage: '提交语言',
    commitLanguageTipLabel: '提交说明语言说明',
    commitLanguageTip: (hasRepository) =>
      `${hasRepository ? '当前项目' : '新项目'}的提交说明语言。`,
    appLanguage: '界面语言',
    modelsHeading: '模型',
    addProvider: '供应商',
    addChildModel: '模型',
    noModelsConfigured: '尚未配置模型',
    addFirstProvider: '添加第一个供应商',
    activeModelBadge: '当前使用',
    connectionModelCount: (count) => `${count} 个模型`,
    emptyConnectionHint: '暂无模型。',
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
    skills: {
      heading: '技能',
      modeBundled: '随包内置',
      modeSynced: '已热同步',
      syncButton: '同步',
      syncingButton: '同步中…',
      restoreBundledButton: '恢复内置',
      restoringButton: '恢复中…',
      sourceRepoLabel: '技能源仓库',
      editSourceButton: '修改',
      cancelEditSource: '取消',
      resetSourceDefault: '默认',
      sourceRepoPlaceholder: 'https://github.com/dpbs-715/skills',
      commitLabel: (shortSha) => shortSha,
      syncedAtLabel: (formatted) => formatted,
      bundledSummary: '随包内置',
      enabledSummary: (enabled, total) => `${enabled}/${total}`,
      toggleListLabel: (expanded) => (expanded ? '收起技能列表' : '展开技能列表'),
      badgeSynced: '热同步',
      badgeBuiltIn: '内置',
      badgeCustom: '自定义',
      refsCount: (count) => `+${count}`,
      bundledReadonlyHint: '',
      nextRunHint: '',
      syncSuccessNotice: (_count, shortSha) => `已同步（${shortSha}）。`,
      resetSuccessNotice: '已恢复内置技能。',
      syncErrorFallback: '同步失败，原有技能不变。',
      toggleErrorFallback: '更新技能失败。',
      resetErrorFallback: '恢复内置技能失败。',
    },
    updates: {
      heading: '版本',
      currentVersion: (version) => `v${version}`,
      checkButton: '检查更新',
      checking: '检查中…',
      upToDate: '已是最新',
      availableTitle: (version) => `发现新版本 v${version}`,
      updateAndRestart: '更新并重启',
      downloading: (percent, downloadedMb, totalMb) => `正在下载… ${percent}%（${downloadedMb} / ${totalMb} MB）`,
      downloadingUnknown: (downloadedMb) => `正在下载… ${downloadedMb} MB`,
      installing: '正在安装并重启…',
      downloadFromGithub: '前往 GitHub 下载',
      retry: '重试',
      gitBusyNote: '请等当前 Git 任务完成后再更新。',
      blockers: {
        devBuild: '仅适用于已打包的 macOS 应用。',
        readOnlyVolume: '请先将 Picket.app 移入「应用程序」。',
        appTranslocation: '请移入「应用程序」后重新打开。',
        signingKeyUnconfigured: '未配置更新公钥，请前往 GitHub 下载。',
      },
    },
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
    connectionNamePlaceholder: '选填',
    parentConnectionLabel: '所属连接',
    connectionIdentityLockedNote: '',
    rotateKeyImpactNote: (count) => `将同步更新该连接下的 ${count} 个模型。`,
    testNeedsModelHint: '请先添加模型后再测试。',
    advancedToggle: '高级选项',
    apiFormatLabel: '接口格式',
    displayNameLabel: '显示名称',
    displayNamePlaceholder: '选填',
    modelIdLabel: '模型 ID',
    fetchModels: '拉取列表',
    refreshModels: '刷新',
    fetchingModels: '拉取中…',
    fetchedModelsCount: (count) => `${count} 个模型`,
    fetchedModelsEmpty: '未返回模型列表，请手动填写。',
    fetchModelsErrorFallback: '拉取失败，请手动填写模型 ID。',
    toggleFetchedModels: (count) => `展开或收起已拉取的 ${count} 个模型`,
    fetchedModelsListLabel: '从供应商拉取的模型列表',
    noMatchingFetchedModels: '无匹配项，回车可使用当前输入。',
    baseUrlLabel: '接口地址（Base URL）',
    apiKeyTipLabel: 'API Key 存储说明',
    apiKeyTipText: '以 0600 权限保存于本机 ~/.picket/credentials.json。',
    apiKeyKeepPlaceholder: '留空保持不变',
    apiKeyEnterPlaceholder: '输入 API Key',
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
};
