import type { GitBranchReply } from '../../../shared/git-branch-protocol';
import type { GitCommitReply, GitCommitSelection } from '../../../shared/git-commit-protocol';
import type { GitReadReply } from '../../../shared/git-read-protocol';
import type { ModelConfiguration, ModelInput } from '../settings/modelProviders';
import type { EditorId, Target, WorkbenchEvent } from './state/workbench';

export type WorkbenchCommand =
  | { kind: 'openRepository'; requestId: string }
  | { kind: 'refresh' | 'pull'; target: Target; requestId: string }
  | { kind: 'createBranch' | 'createWorktree' | 'switchBranch'; target: Target; requestId: string; branch: string; branchToken: string }
  | { kind: 'openFile'; target: Target; requestId: string; fileId: string; editor: EditorId; line?: number }
  | { kind: 'generate' | 'review' | 'cPush'; target: Target; requestId: string; fileIds: string[]; modelId: string; draft: string; changedPaths?: string[] }
  | { kind: 'commit'; target: Target; requestId: string; fileIds: string[]; selections?: GitCommitSelection[]; draft: string; push: boolean }
  | { kind: 'respond'; target: Target; requestId: string; messageId: string; optionId?: string; text?: string }
  | { kind: 'selectModel' | 'deleteModel'; modelId: string };

export interface WorkbenchBackend {
  repositoryReadsAvailable?: boolean;
  execute(command: WorkbenchCommand): Promise<GitReadReply | GitBranchReply | GitCommitReply | void>;
  testModel(input: ModelInput): Promise<string>;
  saveModel(input: ModelInput): Promise<ModelConfiguration>;
  subscribe?(receive: (event: WorkbenchEvent) => void): () => void;
}

// Browser/development fallback. Unimplemented actions must never fabricate Git
// results, saved credentials, or successful model connections.
export const disconnectedBackend: WorkbenchBackend = {
  async execute(command) {
    const stage = ['generate', 'review', 'selectModel', 'deleteModel'].includes(command.kind) ? 'M3' : ['cPush', 'respond'].includes(command.kind) ? 'M4 / M5' : command.kind === 'pull' || (command.kind === 'commit' && command.push) ? 'M5' : 'M2';
    throw new Error(`此操作尚未接入，将在 ${stage} 实现。该操作未执行，不会提交、推送或修改仓库；草稿和选择已保留。`);
  },
  async testModel() { throw new Error('模型连接将在 M3 接入，尚未发送网络请求。请勿在当前版本填写真实密钥。'); },
  async saveModel() { throw new Error('系统钥匙串与模型保存将在 M3 接入，当前没有保存模型或 API Key。'); },
};
