import { Agent, type AgentTool } from '@earendil-works/pi-agent-core';
import { normalizeAppLocale } from '../../../shared/language-protocol';
import { MODEL_GLOBAL_DEFAULTS } from '../../../shared/model-defaults';
import type {
  GenerateCustomActionCommand,
  ListModelsRequest,
  PiTaskRequest,
  PiToolResultCommand,
  ProbeEvent,
  RunCustomActionCommand,
  TestConnectionRequest,
} from '../../../shared/pi-protocol';
import { LocalizedSidecarError, getSidecarMessages } from '../i18n';
import { createProbe } from '../probe';
import { createIsolatedModelRuntime, fetchProviderModels, sanitizeProviderError } from '../providers';
import { SkillRegistry } from '../skills';
import {
  ToolBridge,
  createCheckCliEnvTool,
  createCleanupSubmitDuplicatesTool,
  createCommitExactScopeTool,
  createEditRepoFileTool,
  createInstallPlatformCliTool,
  createManageSubmitWorktreeTool,
  createMessageUserTool,
  createReadConflictFileTool,
  createReadRepoFileTool,
  createReadScopeDiffTool,
  createReadSkillDocTool,
  createRecordActionCheckpointTool,
  createRunGitTool,
  createRunPlatformCliTool,
  createRunRepoCommandTool,
  createSearchRepoTool,
  createSyncAndPushTool,
  createWriteConflictResolutionTool,
  createWriteRepoFileTool,
} from '../tools';
import {
  buildCPushModePrompts,
  buildCustomActionPrompts,
  buildGenerateCustomActionPrompts,
  buildResolveConflictModePrompts,
  parseCustomActionDraft,
  parseCustomActionResult,
} from './custom-actions';
import {
  CUSTOM_ACTION_GUARD_DEFAULTS,
  createToolLoopGuard,
  type ToolLoopGuardPolicy,
} from './tool-guard';

const TEST_CONNECTION_HTTP_TIMEOUT_MS = 45_000;
const TASK_HTTP_CEILING_TIMEOUT_MS = 600_000;
const ACTIVITY_HEARTBEAT_INTERVAL_MS = 1_000;
const MAX_GENERATE_ACTION_TOOL_CALLS = 16;

type AgentTaskCommand =
  | TestConnectionRequest
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
  guardPolicy: ToolLoopGuardPolicy;
}> {
  if (command.type === 'testConnection') {
    return {
      prompts: { systemPrompt: 'Reply with OK only.', userPrompt: 'Connection check.' },
      tools: [],
      guardPolicy: { kind: 'none' },
    };
  }
  if (command.type === 'runCustomAction') {
    const mode = command.mode ?? 'customAction';
    if (mode === 'cpush') {
      return {
        prompts: await buildCPushModePrompts(command, registry),
        tools: [
          createCommitExactScopeTool(bridge, { cpushMode: true }),
          createMessageUserTool(bridge),
          createReadScopeDiffTool(bridge),
          createReadRepoFileTool(bridge),
          createSearchRepoTool(bridge),
        ],
        guardPolicy: {
          kind: 'adaptive',
          maxIdenticalConsecutive: 3,
          maxConsecutiveErrors: 6,
          runawayCeiling: 32,
        },
      };
    }
    if (mode === 'resolveConflict') {
      return {
        prompts: buildResolveConflictModePrompts(command),
        tools: [
          createReadConflictFileTool(bridge),
          createWriteConflictResolutionTool(bridge),
          createReadRepoFileTool(bridge),
          createSearchRepoTool(bridge),
        ],
        guardPolicy: {
          kind: 'adaptive',
          maxIdenticalConsecutive: 3,
          maxConsecutiveErrors: 6,
          runawayCeiling: 64,
        },
      };
    }
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
        createMessageUserTool(bridge),
      ],
      guardPolicy: {
        kind: 'adaptive',
        ...CUSTOM_ACTION_GUARD_DEFAULTS,
      },
    };
  }
  return {
    prompts: buildGenerateCustomActionPrompts(command, registry),
    tools: [
      createReadSkillDocTool(registry),
      createCheckCliEnvTool(bridge),
      createRunGitTool(bridge, true),
      createReadRepoFileTool(bridge),
      createSearchRepoTool(bridge),
      createMessageUserTool(bridge),
    ],
    guardPolicy: { kind: 'fixed', maxCalls: MAX_GENERATE_ACTION_TOOL_CALLS },
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

        const overrideCatalog = command.type === 'runCustomAction'
          || command.type === 'generateCustomAction'
          ? command.skillOverrides
          : undefined;
        const registry = new SkillRegistry(overrideCatalog, async (skill, path) => {
          const response = await bridge.invokeHostTool('read_skill_override_doc', { skill, path });
          return response.ok && !response.isError ? response.content : null;
        });

        const { prompts, tools, guardPolicy } = await prepareAgentTaskConfig(command, bridge, registry);
        if (cancelled) {
          emit({ ...envelope, type: 'cancelled' });
          return;
        }

        const toolGuard = createToolLoopGuard(guardPolicy);
        const agent = new Agent({
          sessionId: command.context.sessionId,
          identity: { attribution: null },
          initialState: {
            model,
            tools,
            thinkingLevel: 'off',
            systemPrompt: prompts.systemPrompt,
          },
          beforeToolCall: toolGuard ? (ctx) => toolGuard.beforeToolCall(ctx) : undefined,
          afterToolCall: toolGuard ? (ctx) => toolGuard.afterToolCall(ctx) : undefined,
          finishTurn: async () => (bridge.getSettledOutcome() ? { action: 'end' as const } : undefined),
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
          const settled = bridge.getSettledOutcome();
          if (settled) {
            return { status: 'settled' as const, settled };
          }
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
        if (firstTurn.status === 'settled') {
          emit({
            ...envelope,
            type: 'customActionCompleted',
            summary: firstTurn.settled.summary || msg.defaultCustomActionSummary,
            links: firstTurn.settled.links,
          });
          return;
        }

        if (command.type === 'testConnection') {
          emit({ ...envelope, type: 'completed', text: firstTurn.rawText.trim() || 'OK' });
        } else if (command.type === 'runCustomAction') {
          const result = parseCustomActionResult(firstTurn.rawText, locale);
          emit({ ...envelope, type: 'customActionCompleted', summary: result.summary, links: result.links });
        } else {
          const draft = wrapLengthTruncation(firstTurn.stopReason, () =>
            parseCustomActionDraft(firstTurn.rawText, locale),
          );
          emit({ ...envelope, type: 'customActionDrafted', draft });
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
