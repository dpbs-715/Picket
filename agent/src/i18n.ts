import { normalizeAppLocale, type AppLocale } from '../../shared/language-protocol';

export class LocalizedSidecarError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'LocalizedSidecarError';
  }
}

export interface SidecarMessages {
  missingBaseUrl: string;
  invalidBaseUrl: string;
  unsupportedBaseUrlProtocol: string;
  missingApiKey: string;
  unknownModelError: string;
  authFailed: (detail: string) => string;
  endpointOrModelNotFound: (detail: string) => string;
  rateLimitedOrQuota: (detail: string) => string;
  providerServiceError: (detail: string) => string;
  connectionFailed: (detail: string) => string;
  requestTimedOut: (detail: string) => string;
  toolsUnsupported: (detail: string) => string;
  requestFailed: (detail: string) => string;
  emptyCommitMessage: string;
  invalidReviewJson: string;
  missingReviewFindings: string;
  invalidReviewFindingItem: string;
  unknownReviewSeverity: string;
  missingReviewTitleOrReason: string;
  invalidCPushPlanJson: string;
  cPushPlanNotObject: string;
  emptyCPushFileRef: string;
  unauthorizedCPushFile: (fileRef: string) => string;
  missingCPushQuestionText: string;
  invalidCPushOptionCount: string;
  invalidCPushOptionFormat: string;
  invalidCPushOptionLabelOrDuplicateId: string;
  invalidCPushPlanKind: string;
  missingCPushGroups: string;
  unconfirmedCPushSkip: string;
  duplicateCPushFile: (fileId: string) => string;
  invalidCPushGroupFormat: string;
  emptyCPushGroupFiles: string;
  duplicateCPushGroupAssignment: (fileId: string) => string;
  noValidCPushGroups: string;
  missingSelectedCPushFile: (path: string) => string;
  defaultSingleCommitPlanSummary: string;
  defaultSplitCommitPlanSummary: (groupCount: number) => string;
  invalidConflictJson: string;
  conflictResultNotObject: string;
  missingResolvedFiles: string;
  invalidConflictFileEntry: string;
  unauthorizedConflictFile: (targetName: string) => string;
  duplicateConflictFile: (path: string) => string;
  missingMergedContent: (path: string) => string;
  leftoverConflictMarkers: (path: string) => string;
  emptyMergedContent: (path: string) => string;
  missingConflictFiles: string;
  defaultConflictMergeSummary: (count: number) => string;
  noResponseMessage: string;
  providerReturnedError: string;
  lengthTruncatedJson: string;
  thinkingTokensExhausted: string;
  defaultCustomActionSummary: string;
  invalidCustomActionDraftJson: string;
  missingCustomActionDraftFields: string;
  defaultCustomActionDraftSummary: string;
}

const SIDECAR_MESSAGES: Record<AppLocale, SidecarMessages> = {
  en: {
    missingBaseUrl: 'Missing model service Base URL.',
    invalidBaseUrl: 'Model service Base URL must be a valid HTTP or HTTPS URL.',
    unsupportedBaseUrlProtocol: 'Model service Base URL only supports HTTP or HTTPS without embedded credentials.',
    missingApiKey: 'Missing model API Key.',
    unknownModelError: 'Unknown model error',
    authFailed: (detail) => `Authentication failed (401/403). Verify your API key and permissions (${detail})`,
    endpointOrModelNotFound: (detail) => `Endpoint or model not found (404). Verify the model ID and Base URL (${detail})`,
    rateLimitedOrQuota: (detail) => `Rate limit or quota exceeded (402/429). Check your account quota or retry shortly (${detail})`,
    providerServiceError: (detail) => `Model provider service error (5xx). Please retry shortly (${detail})`,
    connectionFailed: (detail) => `Unable to connect to model service. Check your network or local server (${detail})`,
    requestTimedOut: (detail) => `Model request timed out. Please retry shortly (${detail})`,
    toolsUnsupported: (detail) => `The selected model does not support tool calling; switch to a tool-capable model in Model Settings (${detail})`,
    requestFailed: (detail) => `Model request failed: ${detail}`,
    emptyCommitMessage: 'Model returned an empty commit message.',
    invalidReviewJson: 'Model did not return valid structured review JSON.',
    missingReviewFindings: 'Model review response is missing a findings array.',
    invalidReviewFindingItem: 'Invalid review finding item format.',
    unknownReviewSeverity: 'Review finding contains an unknown severity level.',
    missingReviewTitleOrReason: 'Review finding is missing a title or reason.',
    invalidCPushPlanJson: 'Model did not return valid C-Push plan JSON.',
    cPushPlanNotObject: 'Model C-Push plan response is not a JSON object.',
    emptyCPushFileRef: 'C-Push plan contains an empty file reference.',
    unauthorizedCPushFile: (fileRef) => `Model attempted to include an unauthorized file (${fileRef}) in the C-Push plan.`,
    missingCPushQuestionText: 'C-Push question is missing a title or description.',
    invalidCPushOptionCount: 'C-Push question must provide between 1 and 6 options.',
    invalidCPushOptionFormat: 'Invalid C-Push question option format.',
    invalidCPushOptionLabelOrDuplicateId: 'C-Push question option is missing a label or has a duplicate ID.',
    invalidCPushPlanKind: 'C-Push plan is missing a valid kind ("ready" or "question").',
    missingCPushGroups: 'C-Push plan is missing a groups array.',
    unconfirmedCPushSkip: 'C-Push cannot skip selected files without user confirmation.',
    duplicateCPushFile: (fileId) => `File ${fileId} appears multiple times in the C-Push plan.`,
    invalidCPushGroupFormat: 'Invalid C-Push commit group format.',
    emptyCPushGroupFiles: 'Each C-Push commit group must contain at least one authorized file.',
    duplicateCPushGroupAssignment: (fileId) => `File ${fileId} was assigned to multiple commit groups.`,
    noValidCPushGroups: 'C-Push plan contains no valid commit groups.',
    missingSelectedCPushFile: (path) => `Selected file ${path} was omitted from the C-Push plan.`,
    defaultSingleCommitPlanSummary: 'Prepared a single-commit plan',
    defaultSplitCommitPlanSummary: (groupCount) => `Planned ${groupCount} separate commits`,
    invalidConflictJson: 'Model did not return valid conflict resolution JSON.',
    conflictResultNotObject: 'Model conflict resolution response is not a JSON object.',
    missingResolvedFiles: 'Model did not return merged file contents (missing resolvedFiles).',
    invalidConflictFileEntry: 'Invalid conflict resolution file entry format.',
    unauthorizedConflictFile: (targetName) => `Blocked unauthorized modification attempt on non-conflicted file (${targetName}).`,
    duplicateConflictFile: (path) => `Conflicted file ${path} appears multiple times in the merge result.`,
    missingMergedContent: (path) => `Conflicted file ${path} is missing merged text content.`,
    leftoverConflictMarkers: (path) => `Merged result for conflicted file ${path} still contains leftover conflict markers; write rejected.`,
    emptyMergedContent: (path) => `Merged result for conflicted file ${path} is empty; merge aborted to prevent accidental file wipe.`,
    missingConflictFiles: 'Model did not return merged results for all conflicted files.',
    defaultConflictMergeSummary: (count) => `Merged ${count} conflicted file(s)`,
    noResponseMessage: 'Model returned no response message.',
    providerReturnedError: 'Model service returned an error.',
    lengthTruncatedJson: 'Model output was truncated due to max output tokens before completing structured JSON; narrow the selected file scope and retry.',
    thinkingTokensExhausted: 'Model thinking tokens exhausted max output tokens before generating final output; narrow the selected scope or switch to a non-reasoning model.',
    defaultCustomActionSummary: 'Custom action completed.',
    invalidCustomActionDraftJson: 'Picket did not return a valid button draft JSON object.',
    missingCustomActionDraftFields: 'Generated button draft is missing a label or prompt.',
    defaultCustomActionDraftSummary: 'Generated from repository context; review or edit before saving.',
  },
  zh: {
    missingBaseUrl: '缺少模型服务接口地址（Base URL）',
    invalidBaseUrl: '模型服务接口地址不是有效的 HTTP / HTTPS URL',
    unsupportedBaseUrlProtocol: '模型服务接口地址仅支持不含账号密码的 HTTP / HTTPS 协议',
    missingApiKey: '缺少模型 API Key',
    unknownModelError: '未知模型错误',
    authFailed: (detail) => `鉴权失败，请检查 API Key 或服务权限（${detail}）`,
    endpointOrModelNotFound: (detail) => `未找到对应接口或模型，请检查模型 ID 与接口地址（${detail}）`,
    rateLimitedOrQuota: (detail) => `请求过于频繁或账户额度不足，请稍后重试（${detail}）`,
    providerServiceError: (detail) => `模型服务异常（5xx），请稍后重试（${detail}）`,
    connectionFailed: (detail) => `无法连接到模型服务，请确认网络或本地服务已启动（${detail}）`,
    requestTimedOut: (detail) => `连接模型服务超时，请稍后重试（${detail}）`,
    toolsUnsupported: (detail) => `当前选中的模型不支持工具调用（Tool Calling），请在模型设置中切换支持工具调用的模型（${detail}）`,
    requestFailed: (detail) => `模型请求未完成：${detail}`,
    emptyCommitMessage: '模型返回了空的提交说明',
    invalidReviewJson: '模型未返回有效的结构化审查 JSON',
    missingReviewFindings: '模型返回的审查结果缺少 findings 数组',
    invalidReviewFindingItem: '审查结果项格式无效',
    unknownReviewSeverity: '审查结果包含未知严重程度',
    missingReviewTitleOrReason: '审查结果缺少标题或原因说明',
    invalidCPushPlanJson: '模型未返回有效的 C-Push 结构化规划 JSON',
    cPushPlanNotObject: '模型返回的 C-Push 规划不是 JSON 对象',
    emptyCPushFileRef: 'C-Push 规划包含空的文件引用',
    unauthorizedCPushFile: (fileRef) => `模型尝试将未授权文件（${fileRef}）纳入 C-Push 计划`,
    missingCPushQuestionText: 'C-Push 提问缺少标题或具体说明',
    invalidCPushOptionCount: 'C-Push 提问必须提供 1 至 6 个明确选项',
    invalidCPushOptionFormat: 'C-Push 提问选项格式无效',
    invalidCPushOptionLabelOrDuplicateId: 'C-Push 提问选项缺少标签或存在重复 ID',
    invalidCPushPlanKind: 'C-Push 规划结果缺少有效的 kind（须为 ready 或 question）',
    missingCPushGroups: 'C-Push 规划缺少 groups 数组',
    unconfirmedCPushSkip: '未经用户确认，C-Push 不能擅自跳过已勾选文件',
    duplicateCPushFile: (fileId) => `文件 ${fileId} 在 C-Push 计划中重复出现`,
    invalidCPushGroupFormat: 'C-Push 提交分组格式无效',
    emptyCPushGroupFiles: 'C-Push 每个提交分组至少需包含一个已授权文件',
    duplicateCPushGroupAssignment: (fileId) => `文件 ${fileId} 被分配到了多个提交分组`,
    noValidCPushGroups: 'C-Push 计划未包含任何有效分组',
    missingSelectedCPushFile: (path) => `已选文件 ${path} 未被纳入 C-Push 分组计划`,
    defaultSingleCommitPlanSummary: '已生成单笔提交计划',
    defaultSplitCommitPlanSummary: (groupCount) => `已规划拆分为 ${groupCount} 个独立提交`,
    invalidConflictJson: '模型未返回有效的冲突合并结构化 JSON',
    conflictResultNotObject: '模型返回的冲突合并结果不是 JSON 对象',
    missingResolvedFiles: '模型未返回冲突文件的合并内容（缺少 resolvedFiles）',
    invalidConflictFileEntry: '冲突合并文件项格式无效',
    unauthorizedConflictFile: (targetName) => `模型尝试修改未授权的非冲突文件（${targetName}），已被拦截`,
    duplicateConflictFile: (path) => `冲突文件 ${path} 在合并结果中重复出现`,
    missingMergedContent: (path) => `冲突文件 ${path} 缺少合并后的文本内容`,
    leftoverConflictMarkers: (path) => `冲突文件 ${path} 的合并结果仍残留冲突标记，已拒绝写入`,
    emptyMergedContent: (path) => `冲突文件 ${path} 返回了空内容，为防止意外清空代码已中止合并`,
    missingConflictFiles: '模型未完整返回所有当前冲突文件的合并结果',
    defaultConflictMergeSummary: (count) => `已完成 ${count} 个冲突文件的合并`,
    noResponseMessage: '模型未返回响应消息',
    providerReturnedError: '模型服务返回错误',
    lengthTruncatedJson: '模型输出达到长度上限被截断，未能生成完整的结构化结果；请缩小单次勾选文件范围后重试',
    thinkingTokensExhausted: '模型思考内容耗尽了输出长度上限，未生成最终正文；请缩小所选变更范围或改用非深度思考模型重试',
    defaultCustomActionSummary: '自定义动作已执行完成。',
    invalidCustomActionDraftJson: 'Picket 未返回有效的按钮草稿 JSON。',
    missingCustomActionDraftFields: '生成的按钮草稿缺少名称或指令内容。',
    defaultCustomActionDraftSummary: '已根据仓库上下文生成按钮草稿，你可以微调后保存。',
  },
};

export function resolveSidecarLocale(locale?: unknown): AppLocale {
  return normalizeAppLocale(locale, 'en');
}

export function getSidecarMessages(locale?: unknown): SidecarMessages {
  return SIDECAR_MESSAGES[resolveSidecarLocale(locale)];
}
