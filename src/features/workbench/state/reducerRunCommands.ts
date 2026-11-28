import {
  RUN_COMMAND_LIMITS,
  sanitizeRunCommandConfig,
  sanitizeRunEnvEntries,
  type RunCommandConfig,
  type RunEnvEntry,
} from '../../../../shared/run-protocol';
import {
  effectiveCommitLanguageFor,
  effectiveEditorFor,
  localKeepPathsFor,
} from './selectors';
import type { WorkbenchAction, WorkbenchState } from './types';

export type RunCommandWorkbenchAction = Extract<
  WorkbenchAction,
  { type: 'saveRunCommand' | 'addRunCommands' | 'deleteRunCommand' | 'setRunSharedEnv' }
>;

export function isRunCommandWorkbenchAction(
  action: WorkbenchAction,
): action is RunCommandWorkbenchAction {
  return (
    action.type === 'saveRunCommand'
    || action.type === 'addRunCommands'
    || action.type === 'deleteRunCommand'
    || action.type === 'setRunSharedEnv'
  );
}

function withUpdatedRunSettings(
  state: WorkbenchState,
  repositoryId: string,
  nextCommands: RunCommandConfig[],
  nextSharedEnv: RunEnvEntry[],
): WorkbenchState {
  const prev = state.repositorySettings[repositoryId];
  const keepPaths = localKeepPathsFor(state, repositoryId);
  return {
    ...state,
    repositorySettings: {
      ...state.repositorySettings,
      [repositoryId]: {
        editor: effectiveEditorFor(state, repositoryId),
        commitLanguage: effectiveCommitLanguageFor(state, repositoryId),
        ...(keepPaths.length ? { localKeepPaths: keepPaths } : {}),
        ...(prev?.customActions?.length ? { customActions: prev.customActions } : {}),
        ...(prev?.activeActionId ? { activeActionId: prev.activeActionId } : {}),
        ...(nextCommands.length ? { runCommands: nextCommands } : {}),
        ...(nextSharedEnv.length ? { runSharedEnv: nextSharedEnv } : {}),
      },
    },
    settingsModified: true,
  };
}

export function reduceRunCommands(
  state: WorkbenchState,
  action: RunCommandWorkbenchAction,
): WorkbenchState {
  const existing = state.repositorySettings[action.repositoryId]?.runCommands ?? [];
  const existingSharedEnv = state.repositorySettings[action.repositoryId]?.runSharedEnv ?? [];

  if (action.type === 'setRunSharedEnv') {
    const cleanEnv = sanitizeRunEnvEntries(action.env);
    return withUpdatedRunSettings(state, action.repositoryId, existing, cleanEnv);
  }
  if (action.type === 'saveRunCommand') {
    let sanitized: RunCommandConfig;
    try {
      sanitized = sanitizeRunCommandConfig(action.command);
    } catch {
      return state;
    }
    if (!sanitized.id || !sanitized.label || !sanitized.command) return state;
    const idx = existing.findIndex((item) => item.id === sanitized.id);
    if (idx >= 0) {
      const next = existing.map((item, i) => (i === idx ? sanitized : item));
      return withUpdatedRunSettings(state, action.repositoryId, next, existingSharedEnv);
    }
    if (existing.length >= RUN_COMMAND_LIMITS.maxPerRepo) return state;
    return withUpdatedRunSettings(state, action.repositoryId, [...existing, sanitized], existingSharedEnv);
  }
  if (action.type === 'addRunCommands') {
    if (!action.commands.length) return state;
    const next = [...existing];
    const seenIds = new Set(existing.map((item) => item.id));
    const seenKeys = new Set(existing.map((item) => `${item.cwd ?? '.'}::${item.command.trim()}`));
    let added = 0;
    for (const raw of action.commands) {
      if (next.length >= RUN_COMMAND_LIMITS.maxPerRepo) break;
      let sanitized: RunCommandConfig;
      try {
        sanitized = sanitizeRunCommandConfig(raw);
      } catch {
        continue;
      }
      if (!sanitized.id || !sanitized.label || !sanitized.command) continue;
      const key = `${sanitized.cwd ?? '.'}::${sanitized.command}`;
      if (seenIds.has(sanitized.id) || seenKeys.has(key)) continue;
      seenIds.add(sanitized.id);
      seenKeys.add(key);
      next.push(sanitized);
      added += 1;
    }
    if (added === 0) return state;
    return withUpdatedRunSettings(state, action.repositoryId, next, existingSharedEnv);
  }
  const next = existing.filter((item) => item.id !== action.commandId);
  if (next.length === existing.length) return state;
  return withUpdatedRunSettings(state, action.repositoryId, next, existingSharedEnv);
}
