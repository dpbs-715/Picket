import type { AgentTool } from '@earendil-works/pi-agent-core';
import type { CustomActionRunMode } from '../../../shared/pi-protocol';
import type { SkillRegistry } from '../skills';
import {
  ToolBridge,
  createCheckCliEnvTool,
  createCleanupSubmitDuplicatesTool,
  createCommitExactScopeTool,
  createEditRepoFileTool,
  createHandoffToRecoveryTool,
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
import { CUSTOM_ACTION_GUARD_DEFAULTS, type ToolLoopGuardPolicy } from './tool-guard';

export type AgentExecutionProfileId =
  | CustomActionRunMode
  | 'generateAction'
  | 'discoverRunCommands'
  | 'testConnection';

export interface AgentExecutionProfileConfig {
  id: AgentExecutionProfileId;
  guardPolicy: ToolLoopGuardPolicy;
  buildTools: (bridge: ToolBridge, registry: SkillRegistry) => AgentTool<any>[];
}

const MAX_GENERATE_ACTION_TOOL_CALLS = 16;
const MAX_DISCOVER_RUN_TOOL_CALLS = 12;

const AGENT_EXECUTION_PROFILES: Record<AgentExecutionProfileId, AgentExecutionProfileConfig> = {
  testConnection: {
    id: 'testConnection',
    guardPolicy: { kind: 'none' },
    buildTools: () => [],
  },
  cpush: {
    id: 'cpush',
    guardPolicy: {
      kind: 'adaptive',
      maxIdenticalConsecutive: 3,
      maxConsecutiveErrors: 6,
      runawayCeiling: 32,
    },
    buildTools: (bridge) => [
      createCommitExactScopeTool(bridge, { cpushMode: true }),
      createMessageUserTool(bridge),
      createReadScopeDiffTool(bridge),
      createReadRepoFileTool(bridge),
      createSearchRepoTool(bridge),
    ],
  },
  resolveConflict: {
    id: 'resolveConflict',
    guardPolicy: {
      kind: 'adaptive',
      maxIdenticalConsecutive: 3,
      maxConsecutiveErrors: 6,
      runawayCeiling: 64,
    },
    buildTools: (bridge) => [
      createReadConflictFileTool(bridge),
      createWriteConflictResolutionTool(bridge),
      createReadRepoFileTool(bridge),
      createSearchRepoTool(bridge),
    ],
  },
  customAction: {
    id: 'customAction',
    guardPolicy: {
      kind: 'adaptive',
      ...CUSTOM_ACTION_GUARD_DEFAULTS,
    },
    buildTools: (bridge, registry) => [
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
      createHandoffToRecoveryTool(bridge),
      createManageSubmitWorktreeTool(bridge),
      createCleanupSubmitDuplicatesTool(bridge),
      createRecordActionCheckpointTool(bridge),
      createReadScopeDiffTool(bridge),
      createReadRepoFileTool(bridge),
      createSearchRepoTool(bridge),
      createMessageUserTool(bridge),
    ],
  },
  generateAction: {
    id: 'generateAction',
    guardPolicy: { kind: 'fixed', maxCalls: MAX_GENERATE_ACTION_TOOL_CALLS },
    buildTools: (bridge, registry) => [
      createReadSkillDocTool(registry),
      createCheckCliEnvTool(bridge),
      createRunGitTool(bridge, true),
      createReadRepoFileTool(bridge),
      createSearchRepoTool(bridge),
      createMessageUserTool(bridge),
    ],
  },
  discoverRunCommands: {
    id: 'discoverRunCommands',
    guardPolicy: { kind: 'fixed', maxCalls: MAX_DISCOVER_RUN_TOOL_CALLS },
    buildTools: (bridge) => [
      createReadRepoFileTool(bridge),
      createSearchRepoTool(bridge),
    ],
  },
};

export function resolveAgentExecutionProfile(
  id: AgentExecutionProfileId,
): AgentExecutionProfileConfig {
  return AGENT_EXECUTION_PROFILES[id];
}
