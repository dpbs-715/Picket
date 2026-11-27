import type { CPushCommandReply } from '../../../shared/cpush-protocol';
import type {
  CustomActionCommandReply,
  CustomActionConfig,
  GenerateCustomActionProgressEvent,
  GenerateCustomActionReply,
  GenerateCustomActionRequest,
  RespondCustomActionRequest,
} from '../../../shared/custom-action-protocol';
import type { GitBranchReply } from '../../../shared/git-branch-protocol';
import type { GitOpenFileReply } from '../../../shared/ide-protocol';
import type { AppLocale, CommitLanguage } from '../../../shared/language-protocol';
import type { MessageUserResponse } from '../../../shared/message-user-protocol';
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
  | { kind: 'pull'; target: Target; requestId: string; modelId?: string; locale?: AppLocale }
  | { kind: 'switchBranch' | 'checkoutRemoteBranch'; target: Target; requestId: string; branch: string; branchToken: string; locale?: AppLocale }
  | { kind: 'openFile'; target: Target; requestId: string; fileId?: string; path?: string; editor: EditorId; line?: number; locale?: AppLocale }
  | { kind: 'cPush'; target: Target; requestId: string; fileIds: string[]; selections?: ModelFileSelection[]; modelId: string; prompt: string; push?: boolean; locale?: AppLocale; commitLanguage?: CommitLanguage }
  | { kind: 'runCustomAction'; target: Target; requestId: string; action: CustomActionConfig; fileIds: string[]; selections?: ModelFileSelection[]; modelId: string; extraPrompt: string; followUpPrompt?: string; resumeFromRunId?: string; locale?: AppLocale; commitLanguage?: CommitLanguage }
  | { kind: 'respond'; target: Target; requestId: string; runId?: string; messageId: string; optionId?: string; text?: string; questionnaireResponse?: MessageUserResponse; modelId?: string; locale?: AppLocale }
  | { kind: 'selectModel' | 'deleteModel'; modelId: string; locale?: AppLocale }
  | { kind: 'deleteProviderConnection'; connectionId: string; locale?: AppLocale };

export type WorkbenchCommandReply =
  | GitReadReply
  | GitLocalKeepReply
  | GitBranchReply
  | GitOpenFileReply
  | ModelMutateReply
  | CPushCommandReply
  | CustomActionCommandReply
  | void;

export interface WorkbenchBackend {
  repositoryReadsAvailable?: boolean;
  execute(command: WorkbenchCommand): Promise<WorkbenchCommandReply>;
  respondCustomAction?(request: RespondCustomActionRequest): Promise<boolean>;
  generateCustomAction?(
    request: GenerateCustomActionRequest,
    onProgress?: (event: GenerateCustomActionProgressEvent) => void,
  ): Promise<GenerateCustomActionReply>;
  cancelCustomAction?(requestId: string): Promise<void>;
  reconcileSession?(target: Target, locale?: AppLocale): Promise<void>;
  dismissCPushDone?(target: Target, runId: string | undefined, messageId: string, locale?: AppLocale): Promise<void>;
  dismissCustomActionDone?(target: Target, runId: string | undefined, messageId: string, locale?: AppLocale): Promise<void>;
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
  subscribe?(receive: (event: WorkbenchEvent) => void): () => void;
}

// Browser/development fallback. Unimplemented actions must never fabricate Git
// results, saved credentials, or successful model connections.
export const disconnectedBackend: WorkbenchBackend = {
  async execute(command) {
    const t = getMessages(command.locale).desktop;
    if (['selectModel', 'deleteModel', 'deleteProviderConnection', 'cPush', 'runCustomAction', 'respond'].includes(command.kind)) {
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
