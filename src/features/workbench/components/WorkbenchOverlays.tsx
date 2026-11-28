import type { Dispatch, SetStateAction } from 'react';
import type { CustomActionConfig, CustomActionSyncResult } from '../../../../shared/custom-action-protocol';
import type { CommitLanguage } from '../../../../shared/language-protocol';
import {
  isRunProcessActive,
  RUN_COMMAND_LIMITS,
  sanitizeRunCommandConfig,
  type RunCommandConfig,
  type RunEnvEntry,
  type RunProcessSnapshot,
} from '../../../../shared/run-protocol';
import type { SkillsStateSnapshot } from '../../../../shared/skills-protocol';
import type { UpdateStateSnapshot } from '../../../../shared/update-protocol';
import { getMessages } from '../../../i18n';
import { MotionPresence } from '../../../ui/motion';
import { CustomActionModal } from '../../actions/CustomActionModal';
import { CustomActionsManagerModal } from '../../actions/CustomActionsManagerModal';
import { DiscoverRunCommandsModal } from '../../runner/DiscoverRunCommandsModal';
import { RunCommandModal } from '../../runner/RunCommandModal';
import { RunSharedEnvModal } from '../../runner/RunSharedEnvModal';
import { ModelEntryForm } from '../../settings/ModelEntryForm';
import type { ModelConfiguration } from '../../settings/modelProviders';
import { ProviderConnectionForm } from '../../settings/ProviderConnectionForm';
import { SettingsPanel } from '../../settings/SettingsPanel';
import type { WorkbenchBackend } from '../commands';
import {
  computeSyncCustomActionAcrossRepos,
  targetOf,
  type CustomActionSyncRepoTarget,
  type EditorId,
  type Repository,
  type WorkbenchPresentation,
  type WorkbenchState,
  type Worktree,
} from '../state/workbench';
import type {
  ActionOverlayItem,
  ModelOverlayState,
  NoticeItem,
  NotifyFn,
  RunOverlayState,
  SettingsSession,
  WorkbenchDispatch,
} from '../types';

interface Props {
  state: WorkbenchState;
  backend: WorkbenchBackend;
  presentation: WorkbenchPresentation;
  settingsSession: SettingsSession | null;
  settingsOpen: boolean;
  pinBusy: boolean;
  modelOverlay: ModelOverlayState;
  setModelOverlay: Dispatch<SetStateAction<ModelOverlayState>>;
  actionStack: ActionOverlayItem[];
  setActionStack: Dispatch<SetStateAction<ActionOverlayItem[]>>;
  runOverlay: RunOverlayState;
  setRunOverlay: Dispatch<SetStateAction<RunOverlayState>>;
  activeRepoRunCommands: RunCommandConfig[];
  activeRepoRunSharedEnv: RunEnvEntry[];
  runProcesses: RunProcessSnapshot[];
  onStopAllForRunCommand: (repositoryId: string, commandId: string) => Promise<boolean>;
  worktree: Worktree | undefined;
  repository: Repository | undefined;
  viewKey: string;
  model: ModelConfiguration | undefined;
  activeEditor: EditorId;
  activeCommitLanguage: CommitLanguage;
  activeRepoCustomActions: CustomActionConfig[];
  activeCustomAction: CustomActionConfig | undefined;
  otherRepositories: CustomActionSyncRepoTarget[];
  otherRepoCount: number;
  activeNotice: NoticeItem | undefined;
  skillsState: SkillsStateSnapshot;
  skillsBusy: 'sync' | 'toggle' | 'reset' | null;
  skillsError: string;
  skillsNotice: string;
  onSyncSkills: (sourceRepo?: string) => Promise<boolean>;
  onToggleSkill: (name: string, enabled: boolean) => Promise<void>;
  onResetSkills: () => Promise<void>;
  updateState: UpdateStateSnapshot;
  gitBusyForUpdate: boolean;
  onCheckUpdate: () => Promise<void>;
  onInstallUpdate: (version: string) => Promise<void>;
  onCancelUpdate: () => Promise<void>;
  onOpenReleasePage: () => void;
  onSelectModel: (id: string) => Promise<void>;
  onDeleteModel: (id: string) => Promise<void>;
  onDeleteProviderConnection: (connectionId: string) => Promise<void>;
  onSyncActionToAllRepos: (action: CustomActionConfig, overwrite: boolean) => CustomActionSyncResult;
  onCloseSettings: () => Promise<void>;
  dispatch: WorkbenchDispatch;
  notify: NotifyFn;
}

export function WorkbenchOverlays({
  state,
  backend,
  presentation,
  settingsSession,
  settingsOpen,
  pinBusy,
  modelOverlay,
  setModelOverlay,
  actionStack,
  setActionStack,
  runOverlay,
  setRunOverlay,
  activeRepoRunCommands,
  activeRepoRunSharedEnv,
  runProcesses,
  onStopAllForRunCommand,
  worktree,
  repository,
  viewKey,
  model,
  activeEditor,
  activeCommitLanguage,
  activeRepoCustomActions,
  activeCustomAction,
  otherRepositories,
  otherRepoCount,
  activeNotice,
  skillsState,
  skillsBusy,
  skillsError,
  skillsNotice,
  onSyncSkills,
  onToggleSkill,
  onResetSkills,
  updateState,
  gitBusyForUpdate,
  onCheckUpdate,
  onInstallUpdate,
  onCancelUpdate,
  onOpenReleasePage,
  onSelectModel,
  onDeleteModel,
  onDeleteProviderConnection,
  onSyncActionToAllRepos,
  onCloseSettings,
  dispatch,
  notify,
}: Props) {
  const t = getMessages(state.locale);
  const modelOverlayOpen = modelOverlay.kind !== 'closed';
  const actionOverlayOpen = actionStack.length > 0;
  const runOverlayOpen = runOverlay.kind !== 'closed';

  return (
    <>
      <MotionPresence>
        {presentation.showSettings && (
          <SettingsPanel
            key={presentation.surface === 'standaloneSettings' ? 'settings-standalone' : 'settings-sheet'}
            overlayVariant={presentation.surface === 'standaloneSettings' ? 'standalone' : 'sheet'}
            locale={state.locale}
            theme={state.theme}
            actionApprovalMode={state.actionApprovalMode}
            repositoryName={repository?.name}
            editor={activeEditor}
            commitLanguage={activeCommitLanguage}
            installedEditors={state.installedEditors}
            connections={state.connections}
            models={state.models}
            activeModelId={state.activeModelId}
            onLocale={(locale) => dispatch({ type: 'locale', locale })}
            onTheme={(theme) => dispatch({ type: 'theme', theme })}
            onActionApprovalMode={(mode) => dispatch({ type: 'actionApprovalMode', mode })}
            onEditor={(nextEditor) => dispatch({ type: 'editor', editor: nextEditor, repositoryId: worktree?.repositoryId ?? null })}
            onCommitLanguage={(commitLanguage) => dispatch({ type: 'commitLanguage', commitLanguage, repositoryId: worktree?.repositoryId ?? null })}
            onSelectModel={(id) => void onSelectModel(id)}
            onAddProvider={() => setModelOverlay({ kind: 'createProvider', keepActiveModel: state.models.length > 0 })}
            onEditProvider={(connection) => setModelOverlay({ kind: 'editProvider', connection })}
            onDeleteProvider={(connectionId) => void onDeleteProviderConnection(connectionId)}
            onAddModel={(connection) => setModelOverlay({ kind: 'addModel', connection })}
            onEditModel={(connection, target) => setModelOverlay({ kind: 'editModel', connection, model: target })}
            onDeleteModel={(id) => void onDeleteModel(id)}
            skillsState={skillsState}
            skillsBusy={skillsBusy}
            skillsError={skillsError}
            skillsNotice={skillsNotice}
            onSyncSkills={onSyncSkills}
            onToggleSkill={(name, enabled) => void onToggleSkill(name, enabled)}
            onResetSkills={() => void onResetSkills()}
            updateState={updateState}
            gitBusyForUpdate={gitBusyForUpdate}
            onCheckUpdate={() => void onCheckUpdate()}
            onInstallUpdate={(version) => void onInstallUpdate(version)}
            onCancelUpdate={() => void onCancelUpdate()}
            onOpenReleasePage={onOpenReleasePage}
            covered={modelOverlayOpen}
            closeDisabled={pinBusy && settingsSession?.returnTo === 'compact'}
            error={activeNotice?.text}
            focusModels={settingsSession?.focusModels ?? false}
            onClose={() => void onCloseSettings()}
          />
        )}
      </MotionPresence>
      <MotionPresence>
        {presentation.surface !== 'compact' && worktree && repository && actionStack.map((item, idx) => {
          const isCovered = idx < actionStack.length - 1 || modelOverlayOpen;
          const hasParent = idx > 0;
          if (item.kind === 'manage') {
            return (
              <CustomActionsManagerModal
                key={`manage-actions:${repository.id}`}
                depth={idx}
                covered={isCovered}
                repositoryName={repository.name}
                actions={activeRepoCustomActions}
                activeActionId={activeCustomAction?.id ?? null}
                otherRepoCount={otherRepoCount}
                onClose={() => setActionStack([])}
                onSelectActive={(actionId) => {
                  dispatch({ type: 'selectActiveAction', repositoryId: worktree.repositoryId, actionId });
                }}
                onNewAction={(mode) => setActionStack((prev) => [...prev, { kind: 'create', mode }])}
                onEditAction={(action) => setActionStack((prev) => [...prev, { kind: 'edit', action }])}
                onDeleteAction={(actionId) => {
                  const target = activeRepoCustomActions.find((a) => a.id === actionId);
                  dispatch({ type: 'deleteCustomAction', repositoryId: worktree.repositoryId, actionId });
                  if (target) notify(t.actions.actionDeletedNotice(target.label), viewKey, { tone: 'info' });
                }}
                onSyncToAllRepos={(action, overwrite) => onSyncActionToAllRepos(action, overwrite)}
              />
            );
          }
          return (
            <CustomActionModal
              key={item.kind === 'edit' ? `edit-action:${item.action.id}` : `new-action:${item.mode}`}
              depth={idx}
              covered={isCovered}
              repositoryName={repository.name}
              existingActions={activeRepoCustomActions}
              initialAction={item.kind === 'edit' ? item.action : undefined}
              initialMode={item.kind === 'create' ? item.mode : 'manual'}
              otherRepositories={otherRepositories}
              hasModel={!!model}
              canGenerateWithPi={Boolean(model && backend.generateCustomAction)}
              onBack={hasParent ? () => setActionStack((prev) => prev.slice(0, -1)) : undefined}
              backLabel={hasParent ? t.actions.manageModalTitle : undefined}
              onClose={() => setActionStack([])}
              onConfigureModel={() => setModelOverlay({ kind: 'createProvider', keepActiveModel: false })}
              onSave={(savedAction, selectAsActive, syncTargetRepositoryIds) => {
                const syncResult = syncTargetRepositoryIds.length > 0
                  ? computeSyncCustomActionAcrossRepos(
                      state,
                      worktree.repositoryId,
                      savedAction,
                      true,
                      syncTargetRepositoryIds,
                    ).result
                  : null;
                dispatch({
                  type: 'saveCustomAction',
                  repositoryId: worktree.repositoryId,
                  action: savedAction,
                  selectAsActive,
                  ...(syncTargetRepositoryIds.length > 0
                    ? { syncTargetRepositoryIds, syncOverwrite: true }
                    : {}),
                });
                setActionStack((prev) => (prev.length > 1 && !selectAsActive ? prev.slice(0, -1) : []));
                notify(
                  syncResult
                    ? t.actions.actionSavedWithSyncNotice(
                        savedAction.label,
                        syncResult.added,
                        syncResult.updated,
                        syncResult.skipped,
                      )
                    : t.actions.actionSavedNotice(savedAction.label),
                  viewKey,
                  { tone: 'success' },
                );
              }}
              onDelete={(actionId) => {
                const target = activeRepoCustomActions.find((a) => a.id === actionId);
                dispatch({ type: 'deleteCustomAction', repositoryId: worktree.repositoryId, actionId });
                setActionStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : []));
                if (target) notify(t.actions.actionDeletedNotice(target.label), viewKey, { tone: 'info' });
              }}
              onResetWriteTrust={(actionId) => {
                dispatch({ type: 'resetCustomActionWriteTrust', repositoryId: worktree.repositoryId, actionId });
              }}
              onGenerateWithPi={backend.generateCustomAction && model
                ? (requestId, userRequest, onProgress) => backend.generateCustomAction!(
                    {
                      requestId,
                      target: targetOf(worktree),
                      modelId: model.id,
                      userRequest,
                      existingLabels: activeRepoCustomActions.map((a) => a.label),
                      locale: state.locale,
                    },
                    onProgress,
                  )
                : undefined}
              onRespondGenerateQuestion={backend.respondActionQuestion
                ? (runId, callId, response) => backend.respondActionQuestion!({
                    requestId: crypto.randomUUID(),
                    runId,
                    callId,
                    response,
                    locale: state.locale,
                  })
                : undefined}
              onCancelGenerate={(runId) => {
                void backend.cancelAction?.(runId).catch(() => {});
              }}
            />
          );
        })}
      </MotionPresence>
      <MotionPresence>
        {presentation.surface !== 'compact' && worktree && repository && (runOverlay.kind === 'create' || runOverlay.kind === 'edit') && (
          <RunCommandModal
            key={runOverlay.kind === 'edit' ? `edit-run:${runOverlay.command.id}` : `new-run:${repository.id}`}
            repositoryName={repository.name}
            existingCommands={activeRepoRunCommands}
            initialCommand={runOverlay.kind === 'edit' ? runOverlay.command : undefined}
            runningCount={
              runOverlay.kind === 'edit'
                ? runProcesses.filter(
                    (proc) => proc.repositoryId === repository.id
                      && proc.commandId === runOverlay.command.id
                      && isRunProcessActive(proc.status),
                  ).length
                : 0
            }
            covered={modelOverlayOpen}
            onClose={() => setRunOverlay({ kind: 'closed' })}
            onSave={(command) => {
              dispatch({ type: 'saveRunCommand', repositoryId: worktree.repositoryId, command });
              setRunOverlay({ kind: 'closed' });
              notify(t.run.savedNotice(command.label), viewKey, { tone: 'success' });
            }}
            onDelete={async (commandId) => {
              const target = activeRepoRunCommands.find((c) => c.id === commandId);
              const stopped = await onStopAllForRunCommand(worktree.repositoryId, commandId);
              if (!stopped) return;
              dispatch({ type: 'deleteRunCommand', repositoryId: worktree.repositoryId, commandId });
              setRunOverlay({ kind: 'closed' });
              if (target) notify(t.run.deletedNotice(target.label), viewKey, { tone: 'info' });
            }}
          />
        )}
        {presentation.surface !== 'compact' && worktree && repository && runOverlay.kind === 'discover' && (
          <DiscoverRunCommandsModal
            key={`discover-run:${repository.id}`}
            repositoryName={repository.name}
            existingCommands={activeRepoRunCommands}
            hasModel={!!model}
            canDiscover={Boolean(backend.discoverRunCommands && model)}
            covered={modelOverlayOpen}
            onClose={() => setRunOverlay({ kind: 'closed' })}
            onConfigureModel={() => setModelOverlay({ kind: 'createProvider', keepActiveModel: state.models.length > 0 })}
            onAddManual={() => setRunOverlay({ kind: 'create' })}
            onDiscover={(requestId, onProgress) => backend.discoverRunCommands!(
              {
                requestId,
                target: targetOf(worktree),
                modelId: model!.id,
                existingCommands: activeRepoRunCommands.map((c) => c.command),
                locale: state.locale,
              },
              onProgress,
            )}
            onCancelDiscover={(requestId) => {
              void backend.cancelDiscoverRunCommands?.(requestId).catch(() => {});
            }}
            onAddCommands={(commands) => {
              const seenIds = new Set(activeRepoRunCommands.map((item) => item.id));
              const seenKeys = new Set(activeRepoRunCommands.map((item) => `${item.cwd ?? '.'}::${item.command.trim()}`));
              let addedCount = 0;
              for (const raw of commands) {
                if (activeRepoRunCommands.length + addedCount >= RUN_COMMAND_LIMITS.maxPerRepo) break;
                try {
                  const sanitized = sanitizeRunCommandConfig(raw);
                  if (!sanitized.id || !sanitized.label || !sanitized.command) continue;
                  const key = `${sanitized.cwd ?? '.'}::${sanitized.command}`;
                  if (seenIds.has(sanitized.id) || seenKeys.has(key)) continue;
                  seenIds.add(sanitized.id);
                  seenKeys.add(key);
                  addedCount += 1;
                } catch {
                  continue;
                }
              }
              dispatch({ type: 'addRunCommands', repositoryId: worktree.repositoryId, commands });
              if (addedCount > 0) {
                notify(t.run.discoverAddedNotice(addedCount), viewKey, { tone: 'success' });
              }
            }}
          />
        )}
        {presentation.surface !== 'compact' && worktree && repository && runOverlay.kind === 'sharedEnv' && (
          <RunSharedEnvModal
            key={`shared-env:${repository.id}`}
            repositoryName={repository.name}
            initialEnv={activeRepoRunSharedEnv}
            covered={modelOverlayOpen}
            onClose={() => setRunOverlay({ kind: 'closed' })}
            onSave={(env) => {
              dispatch({ type: 'setRunSharedEnv', repositoryId: worktree.repositoryId, env });
              setRunOverlay({ kind: 'closed' });
              notify(t.run.sharedEnvSavedNotice(env.length), viewKey, { tone: 'success' });
            }}
          />
        )}
      </MotionPresence>
      <MotionPresence>
        {presentation.surface !== 'compact' && modelOverlay.kind === 'createProvider' && (
          <ProviderConnectionForm
            key="create-provider"
            depth={settingsOpen ? 1 : actionOverlayOpen ? actionStack.length : runOverlayOpen ? 1 : 0}
            overlayVariant={settingsOpen || actionOverlayOpen || runOverlayOpen ? 'drill' : 'sheet'}
            mode="create"
            keepActiveModel={modelOverlay.keepActiveModel}
            onBack={settingsOpen || actionOverlayOpen || runOverlayOpen ? () => setModelOverlay({ kind: 'closed' }) : undefined}
            backLabel={
              settingsOpen
                ? t.settings.tabs.models
                : actionOverlayOpen
                  ? t.actions.newModalTitle
                  : runOverlayOpen
                    ? t.run.discoverModalTitle
                    : undefined
            }
            closeDisabled={settingsOpen && pinBusy && settingsSession?.returnTo === 'compact'}
            onClose={() => {
              setModelOverlay({ kind: 'closed' });
              if (settingsOpen) void onCloseSettings();
              else if (actionOverlayOpen) setActionStack([]);
              else if (runOverlayOpen) setRunOverlay({ kind: 'closed' });
            }}
            test={(input) => backend.testModel({ ...input, locale: state.locale })}
            listModels={backend.listProviderModels
              ? (input) => backend.listProviderModels!({ ...input, locale: state.locale })
              : undefined}
            create={(input) => backend.createProviderConnection({ ...input, locale: state.locale })}
            onCreated={(reply) => {
              const closeSettingsAfter = !modelOverlay.keepActiveModel;
              dispatch({
                type: 'catalogSynced',
                connections: reply.connections,
                models: reply.models,
                activeModelId: reply.activeModelId,
              });
              setModelOverlay({ kind: 'closed' });
              if (settingsOpen && closeSettingsAfter) {
                void onCloseSettings();
              }
            }}
          />
        )}
        {presentation.surface !== 'compact' && modelOverlay.kind === 'editProvider' && (
          <ProviderConnectionForm
            key={`edit-provider:${modelOverlay.connection.id}`}
            depth={settingsOpen ? 1 : 0}
            overlayVariant={settingsOpen ? 'drill' : 'sheet'}
            mode="edit"
            connection={
              state.connections.find((c) => c.id === modelOverlay.connection.id)
              ?? modelOverlay.connection
            }
            childModels={state.models.filter((m) => m.connectionId === modelOverlay.connection.id)}
            onBack={settingsOpen ? () => setModelOverlay({ kind: 'closed' }) : undefined}
            backLabel={settingsOpen ? t.settings.tabs.models : undefined}
            closeDisabled={settingsOpen && pinBusy && settingsSession?.returnTo === 'compact'}
            onClose={() => {
              setModelOverlay({ kind: 'closed' });
              if (settingsOpen) void onCloseSettings();
            }}
            test={(input) => backend.testModel({ ...input, locale: state.locale })}
            update={(input) => backend.updateProviderConnection({ ...input, locale: state.locale })}
            revealApiKey={
              backend.revealProviderApiKey
                ? (connectionId) => backend.revealProviderApiKey!(connectionId, state.locale)
                : undefined
            }
            onKeyHintResolved={(connectionId, keyHint) =>
              dispatch({ type: 'providerKeyHintSynced', connectionId, keyHint })}
            onUpdated={(reply) => {
              dispatch({
                type: 'catalogSynced',
                connections: reply.connections,
                models: reply.models,
                activeModelId: reply.activeModelId,
              });
              setModelOverlay({ kind: 'closed' });
            }}
          />
        )}
        {presentation.surface !== 'compact' && (modelOverlay.kind === 'addModel' || modelOverlay.kind === 'editModel') && (
          <ModelEntryForm
            key={
              modelOverlay.kind === 'editModel'
                ? `edit-model:${modelOverlay.model.id}`
                : `add-model:${modelOverlay.connection.id}`
            }
            depth={settingsOpen ? 1 : 0}
            overlayVariant={settingsOpen ? 'drill' : 'sheet'}
            connection={
              state.connections.find((c) => c.id === modelOverlay.connection.id)
              ?? modelOverlay.connection
            }
            initialModel={modelOverlay.kind === 'editModel' ? modelOverlay.model : undefined}
            onBack={settingsOpen ? () => setModelOverlay({ kind: 'closed' }) : undefined}
            backLabel={settingsOpen ? t.settings.tabs.models : undefined}
            closeDisabled={settingsOpen && pinBusy && settingsSession?.returnTo === 'compact'}
            onClose={() => {
              setModelOverlay({ kind: 'closed' });
              if (settingsOpen) void onCloseSettings();
            }}
            test={(input) => backend.testModel({ ...input, locale: state.locale })}
            listModels={backend.listProviderModels
              ? (input) => backend.listProviderModels!({ ...input, locale: state.locale })
              : undefined}
            save={(input) => backend.saveModel({ ...input, locale: state.locale })}
            onSaved={(reply) => {
              dispatch({
                type: 'catalogSynced',
                connections: reply.connections,
                models: reply.models,
                activeModelId: reply.activeModelId,
              });
              setModelOverlay({ kind: 'closed' });
            }}
          />
        )}
      </MotionPresence>
    </>
  );
}
