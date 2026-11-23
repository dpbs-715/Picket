import type { CPushCommandReply } from '../../../shared/cpush-protocol';
import type { GitBranchReply } from '../../../shared/git-branch-protocol';
import type { GitCommitReply, GitCommitSelection } from '../../../shared/git-commit-protocol';
import type { GitOpenFileReply } from '../../../shared/ide-protocol';
import type { GenerateCommitReply, ModelFileSelection, ModelMutateReply, ReviewScopeReply } from '../../../shared/model-protocol';
import type { GitReadReply } from '../../../shared/git-read-protocol';
import type { WorkbenchRestoreReply, WorkbenchSaveRequest } from '../../../shared/workbench-persistence-protocol';
import type { ModelConfiguration, ModelInput } from '../settings/modelProviders';
import type { EditorId, Target, WorkbenchEvent } from './state/workbench';

export type WorkbenchCommand =
  | { kind: 'openRepository'; requestId: string }
  | { kind: 'refresh'; target: Target; requestId: string }
  | { kind: 'pull'; target: Target; requestId: string; modelId?: string }
  | { kind: 'switchBranch' | 'checkoutRemoteBranch'; target: Target; requestId: string; branch: string; branchToken: string }
  | { kind: 'openFile'; target: Target; requestId: string; fileId: string; editor: EditorId; line?: number }
  | { kind: 'generate' | 'review' | 'cPush'; target: Target; requestId: string; fileIds: string[]; selections?: ModelFileSelection[]; modelId: string; draft: string; changedPaths?: string[] }
  | { kind: 'commit'; target: Target; requestId: string; fileIds: string[]; selections?: GitCommitSelection[]; draft: string; push: boolean; modelId?: string }
  | { kind: 'respond'; target: Target; requestId: string; runId?: string; messageId: string; optionId?: string; text?: string; modelId?: string }
  | { kind: 'selectModel' | 'deleteModel'; modelId: string };

export type WorkbenchCommandReply =
  | GitReadReply
  | GitBranchReply
  | GitCommitReply
  | GitOpenFileReply
  | GenerateCommitReply
  | ReviewScopeReply
  | ModelMutateReply
  | CPushCommandReply
  | void;

export interface WorkbenchBackend {
  repositoryReadsAvailable?: boolean;
  execute(command: WorkbenchCommand): Promise<WorkbenchCommandReply>;
  reconcileSession?(target: Target): Promise<void>;
  cancelRequest?(requestId: string): Promise<void>;
  loadWorkbenchState?(): Promise<WorkbenchRestoreReply>;
  saveWorkbenchState?(request: WorkbenchSaveRequest): Promise<void>;
  testModel(input: ModelInput): Promise<string>;
  saveModel(input: ModelInput): Promise<ModelConfiguration>;
  subscribe?(receive: (event: WorkbenchEvent) => void): () => void;
}

// Browser/development fallback. Unimplemented actions must never fabricate Git
// results, saved credentials, or successful model connections.
export const disconnectedBackend: WorkbenchBackend = {
  async execute(command) {
    if (['generate', 'review', 'selectModel', 'deleteModel', 'cPush', 'respond'].includes(command.kind)) {
      throw new Error('当前环境未连接桌面模型或 C-Push 服务，此操作尚未接入浏览器预览；草稿和选择已保留。');
    }
    throw new Error('当前环境未连接桌面 Git 服务，此操作尚未接入浏览器预览；不会提交、推送或修改仓库，草稿和选择已保留。');
  },
  async testModel() { throw new Error('当前环境未连接桌面模型服务，未发送连接请求；输入已保留。'); },
  async saveModel() { throw new Error('当前环境未连接系统钥匙串，没有保存模型或密钥；输入已保留。'); },
};
