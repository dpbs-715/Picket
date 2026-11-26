import { Agent, type AgentTool } from '@earendil-works/pi-agent-core';
import { normalizeAppLocale } from '../../../shared/language-protocol';
import { MODEL_GLOBAL_DEFAULTS } from '../../../shared/model-defaults';
import type {
  GenerateCustomActionCommand,
  ListModelsRequest,
  PiCPushPlan,
  PiTaskRequest,
  PiToolResultCommand,
  PlanCPushCommand,
  ProbeEvent,
  ResolveConflictCommand,
  ReviewScopeCommand,
  RunCustomActionCommand,
  TestConnectionRequest,
} from '../../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import { createProbe } from '../probe';
import { createIsolatedModelRuntime, fetchProviderModels, sanitizeProviderError } from '../providers';
import { SkillRegistry } from '../skills';
import {
  ToolBridge,
  createAskUserQuestionTool,
  createCheckCliEnvTool,
  createCleanupSubmitDuplicatesTool,
  createCommitExactScopeTool,
  createEditRepoFileTool,
  createInstallPlatformCliTool,
  createManageSubmitWorktreeTool,
  createReadRepoFileTool,
  createReadScopeDiffTool,
  createReadSkillDocTool,
  createRecordActionCheckpointTool,
  createRunGitTool,
  createRunPlatformCliTool,
  createRunRepoCommandTool,
  createSearchRepoTool,
  createSyncAndPushTool,
  createWriteRepoFileTool,
} from '../tools';
import { buildResolveConflictPrompts, parseConflictResolution } from './conflict';
import { buildCPushPlanPrompts, parseCPushPlan } from './cpush';
import {
  buildCustomActionPrompts,
  buildGenerateCustomActionPrompts,
  parseCustomActionDraft,
  parseCustomActionResult,
} from './custom-actions';
import { buildReviewPrompts, parseReviewFindings } from './review';
import { RecoverablePlanValidationError } from './shared';

const TEST_CONNECTION_HTTP_TIMEOUT_MS = 45_000;
const TASK_HTTP_CEILING_TIMEOUT_MS = 600_000;
const ACTIVITY_HEARTBEAT_INTERVAL_MS = 1_000;
const MAX_REVIEW_TOOL_CALLS = 24;
const MAX_CPUSH_PLAN_TOOL_CALLS = 8;
const MAX_CUSTOM_ACTION_TOOL_CALLS = 80;
const MAX_GENERATE_ACTION_TOOL_CALLS = 16;

type AgentTaskCommand =
  | TestConnectionRequest
  | ReviewScopeCommand
  | PlanCPushCommand
  | ResolveConflictCommand
  | RunCustomActionCommand
  | GenerateCustomActionCommand;

export interface TaskRunnerHandle {
  cancel: () => void;
  deliverToolResult?: (result: PiToolResultCommand) => void;
  run: () => Promise<void>;
}

async function prepareAgentTaskConfig(
  command: AgentTaskCommand,
  bridge: ToolBridge,
  registry: SkillRegistry,
): Promise<{
  prompts: { systemPrompt: string; userPrompt: string };
  tools: AgentTool<any>[];
  toolBudget: number;
}> {
  if (command.type === 'testConnection') {
    return {
      prompts: { systemPrompt: 'Reply with OK only.', userPrompt: 'Connection check.' },
      tools: [],
      toolBudget: 0,
    };
  }
  if (command.type === 'reviewScope') {
    const dcrDoc = await registry.readDoc('dcr', 'SKILL.md');
    const prompts = buildReviewPrompts(command, dcrDoc.content);
    if (!command.enableTools) {
      return { prompts, tools: [], toolBudget: 0 };
    }
    return {
      prompts,
      tools: [
        createReadScopeDiffTool(bridge),
        createReadRepoFileTool(bridge),
        createSearchRepoTool(bridge),
        createReadSkillDocTool(registry),
      ],
      toolBudget: MAX_REVIEW_TOOL_CALLS,
    };
  }
  if (command.type === 'planCPush') {
    const [cpushDoc, commitDoc] = await Promise.all([
      registry.readDoc('cpush', 'SKILL.md'),
      registry.readDoc('commit', 'SKILL.md'),
    ]);
    const prompts = buildCPushPlanPrompts(command, {
      cpush: cpushDoc.content,
      commit: commitDoc.content,
    });
    if (!command.enableTools) {
      return { prompts, tools: [], toolBudget: 0 };
    }
    return {
      prompts,
      tools: [
        createReadScopeDiffTool(bridge),
        createReadRepoFileTool(bridge),
        createSearchRepoTool(bridge),
      ],
      toolBudget: MAX_CPUSH_PLAN_TOOL_CALLS,
    };
  }
  if (command.type === 'runCustomAction') {
    return {
      prompts: buildCustomActionPrompts(command, registry),
      tools: [
        createReadSkillDocTool(registry),
        createCheckCliEnvTool(bridge),
        createInstallPlatformCliTool(bridge),
        createRunGitTool(bridge, false),
        createRunPlatformCliTool(bridge),
        createEditRepoFileTool(bridge),
        createWriteRepoFileTool(bridge),
        createRunRepoCommandTool(bridge),
        createCommitExactScopeTool(bridge),
        createSyncAndPushTool(bridge),
        createManageSubmitWorktreeTool(bridge),
        createCleanupSubmitDuplicatesTool(bridge),
        createRecordActionCheckpointTool(bridge),
        createReadRepoFileTool(bridge),
        createSearchRepoTool(bridge),
        createAskUserQuestionTool(bridge),
      ],
      toolBudget: MAX_CUSTOM_ACTION_TOOL_CALLS,
    };
  }
  if (command.type === 'generateCustomAction') {
    return {
      prompts: buildGenerateCustomActionPrompts(command, registry),
      tools: [
        createReadSkillDocTool(registry),
        createCheckCliEnvTool(bridge),
        createRunGitTool(bridge, true),
        createReadRepoFileTool(bridge),
        createSearchRepoTool(bridge),
        createAskUserQuestionTool(bridge),
      ],
      toolBudget: MAX_GENERATE_ACTION_TOOL_CALLS,
    };
  }
  return {
    prompts: buildResolveConflictPrompts(command),
    tools: [],
    toolBudget: 0,
  };
}

function runAgentTask(
  command: AgentTaskCommand,
  emit: (event: ProbeEvent) => void,
): TaskRunnerHandle {
  const envelope = { requestId: command.requestId, context: command.context };
  const locale = normalizeAppLocale(command.locale, 'en');
  const msg = getSidecarMessages(locale);
  const bridge = new ToolBridge(envelope, emit);
  let cancelled = false;
  let activeAgent: Agent | undefined;
  let lastActivitySentAt = 0;

  const emitActivity = (force = false) => {
    const now = Date.now();
    if (force || now - lastActivitySentAt >= ACTIVITY_HEARTBEAT_INTERVAL_MS) {
      lastActivitySentAt = now;
      emit({ ...envelope, type: 'activity' });
    }
  };

  return {
    cancel: () => {
      cancelled = true;
      bridge.abortAll('Task cancelled');
      activeAgent?.abort();
    },
    deliverToolResult: (result) => {
      bridge.deliverToolResult(result);
    },
    async run() {
      emit({ ...envelope, type: 'started' });
      let unsubscribe: (() => void) | undefined;
      try {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        const { models, model } = createIsolatedModelRuntime(command.model, locale);
        const isTest = command.type === 'testConnection';

        const overrideCatalog = command.type === 'reviewScope'
          || command.type === 'planCPush'
          || command.type === 'runCustomAction'
          || command.type === 'generateCustomAction'
          ? command.skillOverrides
          : undefined;
        const registry = new SkillRegistry(overrideCatalog, async (skill, path) => {
          const response = await bridge.invokeHostTool('read_skill_override_doc', { skill, path });
          return response.ok && !response.isError ? response.content : null;
        });

        const { prompts, tools, toolBudget } = await prepareAgentTaskConfig(command, bridge, registry);
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }

        let toolCallsExecuted = 0;
        const agent = new Agent({
          sessionId: command.context.sessionId,
          identity: { attribution: null },
          initialState: {
            model,
            tools,
            thinkingLevel: 'off',
            systemPrompt: prompts.systemPrompt,
          },
          beforeToolCall: toolBudget > 0
            ? async () => {
                if (toolCallsExecuted >= toolBudget) {
                  return {
                    block: true,
                    reason: `Tool call budget (${toolBudget} calls) reached. Do not call more tools; return your final JSON response now using the evidence already collected.`,
                  };
                }
                toolCallsExecuted += 1;
                return undefined;
              }
            : undefined,
          streamFn: (targetModel, context, options) => models.streamSimple(targetModel, context, {
            ...options,
            maxTokens: isTest
              ? Math.min(MODEL_GLOBAL_DEFAULTS.testConnectionMaxOutputTokens, targetModel.maxTokens)
              : targetModel.maxTokens,
            ...(targetModel.api === 'openai-responses'
              && /(?:kimi|k2|k3|\bo1\b|\bo3\b|\bo4\b|gpt-5|reasoner|codex)/i.test(targetModel.id)
              ? { samplingParams: { reasoning: { effort: 'low' } } }
              : {}),
            temperature: isTest ? 0.2 : 0.1,
            maxRetries: isTest ? 0 : 1,
            timeoutMs: isTest ? TEST_CONNECTION_HTTP_TIMEOUT_MS : TASK_HTTP_CEILING_TIMEOUT_MS,
            cacheRetention: 'none',
            disableContinuation: true,
            onResponse: () => {
              emitActivity(true);
            },
            onProviderStreamEvent: () => {
              emitActivity(false);
            },
          }),
        });
        activeAgent = agent;
        unsubscribe = agent.subscribe((event) => {
          if (event.type === 'message_update') {
            if (event.assistantMessageEvent.type === 'text_delta') {
              lastActivitySentAt = Date.now();
              emit({ ...envelope, type: 'textDelta', delta: event.assistantMessageEvent.delta });
            } else {
              emitActivity(false);
            }
          } else if (
            event.type === 'tool_execution_start'
            || event.type === 'tool_execution_update'
            || event.type === 'tool_execution_end'
          ) {
            emitActivity(true);
          }
        });

        const runPromptTurn = async (promptText: string) => {
          if (cancelled) return { status: 'cancelled' as const };
          await agent.prompt(promptText);
          await agent.waitForIdle();
          if (cancelled) return { status: 'cancelled' as const };
          const last = agent.state.messages.findLast((message) => message.role === 'assistant');
          if (!last || last.role !== 'assistant') {
            throw new LocalizedSidecarError(msg.noResponseMessage);
          }
          if (last.stopReason === 'aborted') {
            return { status: 'cancelled' as const };
          }
          if (last.stopReason === 'error') {
            throw new Error(last.errorMessage ?? msg.providerReturnedError);
          }
          const rawText = last.content.flatMap((block) => (block.type === 'text' ? [block.text] : [])).join('');
          const hasThinkingOnly = !rawText.trim() && last.content.some((block) => block.type === 'thinking');
          if (hasThinkingOnly && last.stopReason === 'length') {
            throw new LocalizedSidecarError(msg.thinkingTokensExhausted);
          }
          return { status: 'ok' as const, stopReason: last.stopReason, rawText };
        };

        const wrapLengthTruncation = <T>(stopReason: string | undefined, parseFn: () => T): T => {
          try {
            return parseFn();
          } catch (err) {
            if (stopReason === 'length') {
              throw new LocalizedSidecarError(msg.lengthTruncatedJson);
            }
            throw err;
          }
        };

        const firstTurn = await runPromptTurn(prompts.userPrompt);
        if (firstTurn.status === 'cancelled') {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }

        if (command.type === 'testConnection') {
          emit({ ...envelope, type: 'completed', text: firstTurn.rawText.trim() || 'OK' });
        } else if (command.type === 'reviewScope') {
          const findings = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseReviewFindings(firstTurn.rawText, command.files, locale),
          );
          emit({ ...envelope, type: 'reviewCompleted', findings });
        } else if (command.type === 'planCPush') {
          let plan: PiCPushPlan;
          try {
            plan = wrapLengthTruncation(firstTurn.stopReason, () =>
              parseCPushPlan(firstTurn.rawText, command, locale),
            );
          } catch (err) {
            if (!(err instanceof RecoverablePlanValidationError) || firstTurn.stopReason === 'length') {
              throw err;
            }
            emitActivity(true);
            const repairTurn = await runPromptTurn(err.repairPrompt);
            if (repairTurn.status === 'cancelled') {
              emit({ ...envelope, type: 'cancelled' });
              return;
            }
            plan = wrapLengthTruncation(repairTurn.stopReason, () =>
              parseCPushPlan(repairTurn.rawText, command, locale),
            );
          }
          emit({ ...envelope, type: 'cPushPlanCompleted', plan });
        } else if (command.type === 'runCustomAction') {
          const result = parseCustomActionResult(firstTurn.rawText, locale);
          emit({ ...envelope, type: 'customActionCompleted', summary: result.summary, links: result.links });
        } else if (command.type === 'generateCustomAction') {
          const draft = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseCustomActionDraft(firstTurn.rawText, locale),
          );
          emit({ ...envelope, type: 'customActionDrafted', draft });
        } else {
          const resolution = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseConflictResolution(firstTurn.rawText, command, locale),
          );
          emit({ ...envelope, type: 'conflictResolved', resolution });
        }
      } catch (error) {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'failed', message: sanitizeProviderError(error, command.model, locale) });
      } finally {
        unsubscribe?.();
      }
    },
  };
}

function runListModelsTask(
  command: ListModelsRequest,
  emit: (event: ProbeEvent) => void,
): TaskRunnerHandle {
  const envelope = { requestId: command.requestId, context: command.context };
  const locale = normalizeAppLocale(command.locale, 'en');
  const controller = new AbortController();
  let cancelled = false;

  return {
    cancel: () => {
      cancelled = true;
      controller.abort();
    },
    async run() {
      emit({ ...envelope, type: 'started' });
      const timer = setTimeout(() => controller.abort(), 30_000);
      try {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        const models = await fetchProviderModels(command.model, controller.signal, locale);
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'modelsListed', models });
      } catch (error) {
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }
        emit({ ...envelope, type: 'failed', message: sanitizeProviderError(error, command.model, locale) });
      } finally {
        clearTimeout(timer);
      }
    },
  };
}

export function createTaskRunner(command: PiTaskRequest, emit: (event: ProbeEvent) => void): TaskRunnerHandle {
  if (command.type === 'probe') {
    return createProbe(command, emit);
  }
  if (command.type === 'listModels') {
    return runListModelsTask(command, emit);
  }
  return runAgentTask(command, emit);
}
