import type {
  ActionApprovalMode,
  ActionCommandReply,
  CustomActionConfig,
  GenerateCustomActionProgressEvent,
  GenerateCustomActionReply,
  GenerateCustomActionRequest,
  RespondActionQuestionRequest,
  SessionRecoveryAction,
} from '../../../shared/custom-action-protocol';
import type { GitBranchReply } from '../../../shared/git-branch-protocol';
import type { GitOpenFileReply } from '../../../shared/ide-protocol';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import type {
  CreateProviderConnectionInput,
  ListModelsInput,
  ModelFileSelection,
  ModelMutateReply,
  ModelSaveReply,
  ProviderRevealKeyReply,
  ProviderSaveReply,
  ProviderUpdateReply,
  RemoteModelOption,
  SaveModelInput,
  TestConnectionInput,
  UpdateProviderConnectionInput,
} from '../../../shared/model-protocol';
import type { GitLocalKeepReply, GitReadReply } from '../../../shared/git-read-protocol';
import type {
  DiscoverRunCommandsProgressEvent,
  DiscoverRunCommandsReply,
  DiscoverRunCommandsRequest,
  OpenRunLocalUrlRequest,
  OpenRunLogsWindowRequest,
  RestartRunCommandRequest,
  RunProcessSnapshot,
  StartRunCommandRequest,
  StopRunCommandRequest,
  UserEnvSummary,
} from '../../../shared/run-protocol';
import { PICKET_RELEASES_URL, type UpdateStateSnapshot } from '../../../shared/update-protocol';
import type { SkillsStateSnapshot } from '../../../shared/skills-protocol';
import type { WorkbenchRestoreReply, WorkbenchSaveRequest } from '../../../shared/workbench-persistence-protocol';
import { initialBrowserSkillsSnapshot } from '../../desktop/skills';
import { initialBrowserUpdateSnapshot } from '../../desktop/updater';
import { getMessages } from '../../i18n/messages';
import type { EditorId, Target, WorkbenchEvent } from './state/workbench';

export type WorkbenchCommand =
  | { kind: 'openRepository'; requestId: string; locale?: AppLocale }
  | { kind: 'refresh'; target: Target; requestId: string; locale?: AppLocale }
  | { kind: 'setLocalKeep'; target: Target; requestId: string; paths: string[]; keep: boolean; locale?: AppLocale }
  | { kind: 'switchBranch' | 'checkoutRemoteBranch'; target: Target; requestId: string; branch: string; branchToken: string; locale?: AppLocale }
  | { kind: 'openFile'; target: Target; requestId: string; fileId?: string; path?: string; editor: EditorId; line?: number; locale?: AppLocale }
  | {
      kind: 'startAction';
      profile: 'cpush' | 'customAction';
      target: Target;
      requestId: string;
      action?: CustomActionConfig;
      fileIds: string[];
      selections?: ModelFileSelection[];
      modelId: string;
      extraPrompt: string;
      push?: boolean;
      followUpPrompt?: string;
      resumeFromRunId?: string;
      locale?: AppLocale;
      commitLanguage?: CommitLanguage;
      approvalMode?: ActionApprovalMode;
    }
  | {
      kind: 'runRecovery';
      target: Target;
      requestId: string;
      runId?: string;
      messageId?: string;
      action: SessionRecoveryAction;
      modelId?: string;
      locale?: AppLocale;
    }
  | { kind: 'selectModel' | 'deleteModel'; modelId: string; locale?: AppLocale }
  | { kind: 'deleteProviderConnection'; connectionId: string; locale?: AppLocale };

export type WorkbenchCommandReply =
  | GitReadReply
  | GitLocalKeepReply
  | GitBranchReply
  | GitOpenFileReply
  | ModelMutateReply
  | ActionCommandReply
  | void;

export interface WorkbenchBackend {
  repositoryReadsAvailable?: boolean;
  execute(command: WorkbenchCommand): Promise<WorkbenchCommandReply>;
  respondActionQuestion?(request: RespondActionQuestionRequest): Promise<boolean>;
  generateCustomAction?(
    request: GenerateCustomActionRequest,
    onProgress?: (event: GenerateCustomActionProgressEvent) => void,
  ): Promise<GenerateCustomActionReply>;
  cancelAction?(requestId: string): Promise<void>;
  reconcileSession?(target: Target, locale?: AppLocale, expectedRunId?: string): Promise<void>;
  dismissSessionDone?(target: Target, runId: string | undefined, messageId: string, locale?: AppLocale): Promise<void>;
  cancelRequest?(requestId: string): Promise<void>;
  loadWorkbenchState?(): Promise<WorkbenchRestoreReply>;
  saveWorkbenchState?(request: WorkbenchSaveRequest): Promise<void>;
  testModel(input: TestConnectionInput): Promise<string>;
  listProviderModels?(input: ListModelsInput): Promise<RemoteModelOption[]>;
  createProviderConnection(input: CreateProviderConnectionInput): Promise<ProviderSaveReply>;
  updateProviderConnection(input: UpdateProviderConnectionInput): Promise<ProviderUpdateReply>;
  saveModel(input: SaveModelInput): Promise<ModelSaveReply>;
  revealProviderApiKey?(connectionId: string, locale?: AppLocale): Promise<ProviderRevealKeyReply>;
  getUpdateStatus?(): Promise<UpdateStateSnapshot>;
  checkForUpdate?(manual: boolean, locale?: AppLocale): Promise<UpdateStateSnapshot>;
  installUpdate?(version: string, locale?: AppLocale): Promise<UpdateStateSnapshot>;
  cancelUpdate?(locale?: AppLocale): Promise<UpdateStateSnapshot>;
  openReleasePage?(locale?: AppLocale): Promise<void>;
  subscribeUpdateState?(receive: (snapshot: UpdateStateSnapshot) => void): () => void;
  getSkillsState?(locale?: AppLocale): Promise<SkillsStateSnapshot>;
  syncSkills?(sourceRepo: string | undefined, locale?: AppLocale): Promise<SkillsStateSnapshot>;
  setEnabledSkills?(enabledSkills: string[], locale?: AppLocale): Promise<SkillsStateSnapshot>;
  resetSkills?(locale?: AppLocale): Promise<SkillsStateSnapshot>;
  listRunProcesses?(): Promise<RunProcessSnapshot[]>;
  startRunCommand?(request: StartRunCommandRequest): Promise<RunProcessSnapshot>;
  stopRunCommand?(request: StopRunCommandRequest): Promise<RunProcessSnapshot>;
  restartRunCommand?(request: RestartRunCommandRequest): Promise<RunProcessSnapshot>;
  openRunLogsWindow?(request?: OpenRunLogsWindowRequest): Promise<void>;
  openRunLocalUrl?(request: OpenRunLocalUrlRequest): Promise<void>;
  refreshUserShellEnv?(locale?: AppLocale): Promise<UserEnvSummary>;
  subscribeRunState?(receive: (snapshots: RunProcessSnapshot[]) => void): () => void;
  discoverRunCommands?(
    request: DiscoverRunCommandsRequest,
    onProgress?: (event: DiscoverRunCommandsProgressEvent) => void,
  ): Promise<DiscoverRunCommandsReply>;
  cancelDiscoverRunCommands?(requestId: string): Promise<void>;
  subscribe?(receive: (event: WorkbenchEvent) => void): () => void;
}

// Browser/development fallback. Unimplemented actions must never fabricate Git
// results, saved credentials, or successful model connections.
export const disconnectedBackend: WorkbenchBackend = {
  async execute(command) {
    const t = getMessages(command.locale).desktop;
    if (['selectModel', 'deleteModel', 'deleteProviderConnection', 'startAction'].includes(command.kind)
      || (command.kind === 'runRecovery' && command.action.kind !== 'pullRebase')) {
      throw new Error(t.disconnectedModelOrCPush);
    }
    throw new Error(t.disconnectedGit);
  },
  async testModel(input) {
    throw new Error(getMessages(input.locale).desktop.disconnectedTestModel);
  },
  async listProviderModels(input) {
    throw new Error(getMessages(input.locale).desktop.disconnectedTestModel);
  },
  async createProviderConnection(input) {
    throw new Error(getMessages(input.locale).desktop.disconnectedSaveModel);
  },
  async updateProviderConnection(input) {
    throw new Error(getMessages(input.locale).desktop.disconnectedSaveModel);
  },
  async saveModel(input) {
    throw new Error(getMessages(input.locale).desktop.disconnectedSaveModel);
  },
  async revealProviderApiKey(_connectionId, locale) {
    throw new Error(getMessages(locale).desktop.disconnectedRevealModelKey);
  },
  async getUpdateStatus() {
    return initialBrowserUpdateSnapshot();
  },
  async checkForUpdate(_manual, locale) {
    throw new Error(getMessages(locale).desktop.disconnectedUpdater);
  },
  async installUpdate(_version, locale) {
    throw new Error(getMessages(locale).desktop.disconnectedUpdater);
  },
  async cancelUpdate() {
    return initialBrowserUpdateSnapshot();
  },
  async openReleasePage() {
    if (typeof window !== 'undefined') {
      window.open(PICKET_RELEASES_URL, '_blank', 'noopener,noreferrer');
    }
  },
  async getSkillsState() {
    return initialBrowserSkillsSnapshot();
  },
  async syncSkills(_sourceRepo, locale) {
    throw new Error(getMessages(locale).desktop.disconnectedSkills);
  },
  async setEnabledSkills(_enabledSkills, locale) {
    throw new Error(getMessages(locale).desktop.disconnectedSkills);
  },
  async resetSkills(locale) {
    throw new Error(getMessages(locale).desktop.disconnectedSkills);
  },
};
