import type { Dispatch, SetStateAction } from 'react';
import type { CustomActionConfig, CustomActionSyncResult } from '../../../../shared/custom-action-protocol';
import type { CommitLanguage } from '../../../../shared/language-protocol';
import type { SkillsStateSnapshot } from '../../../../shared/skills-protocol';
import type { UpdateStateSnapshot } from '../../../../shared/update-protocol';
import { getMessages } from '../../../i18n';
import { MotionPresence } from '../../../ui/motion';
import { CustomActionModal } from '../../actions/CustomActionModal';
import { CustomActionsManagerModal } from '../../actions/CustomActionsManagerModal';
import { ModelEntryForm } from '../../settings/ModelEntryForm';
import type { ModelConfiguration } from '../../settings/modelProviders';
import { ProviderConnectionForm } from '../../settings/ProviderConnectionForm';
import { SettingsPanel } from '../../settings/SettingsPanel';
import type { WorkbenchBackend } from '../commands';
import {
  targetOf,
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
  worktree: Worktree | undefined;
  repository: Repository | undefined;
  viewKey: string;
  model: ModelConfiguration | undefined;
  activeEditor: EditorId;
  activeCommitLanguage: CommitLanguage;
  activeRepoCustomActions: CustomActionConfig[];
  activeCustomAction: CustomActionConfig | undefined;
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
  worktree,
  repository,
  viewKey,
  model,
  activeEditor,
  activeCommitLanguage,
  activeRepoCustomActions,
  activeCustomAction,
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

  return (
    <>
      <MotionPresence>
        {presentation.showSettings && (
          <SettingsPanel
            key={presentation.surface === 'standaloneSettings' ? 'settings-standalone' : 'settings-sheet'}
            overlayVariant={presentation.surface === 'standaloneSettings' ? 'standalone' : 'sheet'}
            locale={state.locale}
            repositoryName={repository?.name}
            editor={activeEditor}
            commitLanguage={activeCommitLanguage}
            installedEditors={state.installedEditors}
            connections={state.connections}
            models={state.models}
            activeModelId={state.activeModelId}
            onLocale={(locale) => dispatch({ type: 'locale', locale })}
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
              otherRepoCount={otherRepoCount}
              hasModel={!!model}
              canGenerateWithPi={Boolean(model && backend.generateCustomAction)}
              onBack={hasParent ? () => setActionStack((prev) => prev.slice(0, -1)) : undefined}
              backLabel={hasParent ? t.actions.manageModalTitle : undefined}
              onClose={() => setActionStack([])}
              onConfigureModel={() => setModelOverlay({ kind: 'createProvider', keepActiveModel: false })}
              onSave={(savedAction, selectAsActive) => {
                dispatch({
                  type: 'saveCustomAction',
                  repositoryId: worktree.repositoryId,
                  action: savedAction,
                  selectAsActive,
                });
                setActionStack((prev) => (prev.length > 1 && !selectAsActive ? prev.slice(0, -1) : []));
                notify(t.actions.actionSavedNotice(savedAction.label), viewKey, { tone: 'success' });
              }}
              onDelete={(actionId) => {
                const target = activeRepoCustomActions.find((a) => a.id === actionId);
                dispatch({ type: 'deleteCustomAction', repositoryId: worktree.repositoryId, actionId });
                setActionStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : []));
                if (target) notify(t.actions.actionDeletedNotice(target.label), viewKey, { tone: 'info' });
              }}
              onSyncToAllRepos={(action, overwrite) => onSyncActionToAllRepos(action, overwrite)}
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
              onRespondGenerateQuestion={backend.respondCustomAction
                ? (runId, callId, response) => backend.respondCustomAction!({
                    requestId: crypto.randomUUID(),
                    runId,
                    callId,
                    response,
                    locale: state.locale,
                  })
                : undefined}
              onCancelGenerate={(runId) => {
                void backend.cancelCustomAction?.(runId).catch(() => {});
              }}
            />
          );
        })}
      </MotionPresence>
      <MotionPresence>
        {presentation.surface !== 'compact' && modelOverlay.kind === 'createProvider' && (
          <ProviderConnectionForm
            key="create-provider"
            depth={settingsOpen ? 1 : actionStack.length}
            overlayVariant={settingsOpen || actionOverlayOpen ? 'drill' : 'sheet'}
            mode="create"
            keepActiveModel={modelOverlay.keepActiveModel}
            onBack={settingsOpen || actionOverlayOpen ? () => setModelOverlay({ kind: 'closed' }) : undefined}
            backLabel={settingsOpen ? t.settings.tabs.models : actionOverlayOpen ? t.actions.newModalTitle : undefined}
            closeDisabled={settingsOpen && pinBusy && settingsSession?.returnTo === 'compact'}
            onClose={() => {
              setModelOverlay({ kind: 'closed' });
              if (settingsOpen) void onCloseSettings();
              else if (actionOverlayOpen) setActionStack([]);
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
