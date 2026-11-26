import type { ModelApiFormat, ModelProviderId } from '../../../shared/model-protocol';

export interface CommonMessages {
  common: {
    closeOverlay(title: string): string;
    back: string;
    backTo(label: string): string;
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
}

export const commonEn: CommonMessages = {
  common: {
    closeOverlay: (title) => `Close ${title}`,
    back: 'Back',
    backTo: (label) => `Back to ${label}`,
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
};

export const commonZh: CommonMessages = {
  common: {
    closeOverlay: (title) => `关闭${title}`,
    back: '返回',
    backTo: (label) => `返回${label}`,
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
};
