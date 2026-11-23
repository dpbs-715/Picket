import { useEffect, useLayoutEffect, useReducer, useRef, useState } from 'react';
import { readWindowPinned, setWindowPinned } from '../../desktop/window';
import { IconButton } from '../../ui/controls';
import { AddModelForm } from '../settings/AddModelForm';
import { SettingsPanel } from '../settings/SettingsPanel';
import { disconnectedBackend } from './commands';
import type { WorkbenchBackend, WorkbenchCommand } from './commands';
import { ActionFooter } from './components/ActionFooter';
import { ChangesPanel } from './components/ChangesPanel';
import { CommitPanel } from './components/CommitPanel';
import { ContextHeader } from './components/ContextHeader';
import { MessageDock } from './components/MessageDock';
import { ReviewPanel } from './components/ReviewPanel';
import { editors, emptyWorkbench, isActiveSession, isProtected, selectedFiles, targetOf, workbenchReducer } from './state/workbench';
import type { Change, Message, WorkbenchState } from './state/workbench';
import './workbench.css';

interface Props { initialState?: WorkbenchState; backend?: WorkbenchBackend }

export function Workbench({ initialState, backend = disconnectedBackend }: Props) {
  const [state, dispatch] = useReducer(workbenchReducer, initialState, (initial) => initial ?? emptyWorkbench());
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [addModelOpen, setAddModelOpen] = useState(false);
  const [emptyTab, setEmptyTab] = useState<'commit' | 'review'>('commit');
  const [feedback, setFeedback] = useState<Record<string, string>>({});
  const [pending, setPending] = useState<string[]>([]);
  const inFlight = useRef(new Set<string>());
  const [pinned, setPinned] = useState(false);
  const [pinBusy, setPinBusy] = useState(false);
  const pinInFlight = useRef(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const focusRequested = useRef(false);
  const worktree = state.worktrees.find((tree) => tree.worktreeId === state.activeWorktreeId);
  const repository = state.repositories.find((repository) => repository.id === worktree?.repositoryId);
  const viewKey = worktree?.worktreeId ?? 'empty';
  const tab = worktree?.tab ?? emptyTab;
  const draft = worktree?.draft ?? { text: state.emptyDraft, editing: state.emptyEditing, outdated: false };
  const model = state.models.find((model) => model.id === state.activeModelId);
  const editor = editors.find((editor) => editor.id === state.editor)!;
  const selected = worktree ? selectedFiles(worktree) : [];
  const canUseModel = !!model && selected.length > 0 && !worktree?.readError;
  const refreshing = !!worktree && pending.includes(`${worktree.repositoryId}:refresh`);
  const opening = pending.includes('global:openRepository');
  const branchBusy = !!worktree && pending.includes(`${worktree.repositoryId}:write`);
  const busy = refreshing || branchBusy || pending.some((key) => key.startsWith(`${viewKey}:`));

  useEffect(() => backend.subscribe?.((event) => dispatch({ type: 'event', event })), [backend]);
  useEffect(() => {
    let mounted = true;
    void readWindowPinned().then((pinned) => { if (mounted) setPinned(pinned); }).catch(() => {
      if (mounted) setFeedback((previous) => ({ ...previous, [initialState?.activeWorktreeId ?? 'empty']: '无法读取窗口置顶状态，请重试窗口置顶操作。' }));
    });
    return () => { mounted = false; };
  }, []);
  useEffect(() => {
    if (!backend.repositoryReadsAvailable || !worktree) return;
    const refresh = () => { void execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID() }); };
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [backend, worktree?.worktreeId, worktree?.scopeVersion]);
  useLayoutEffect(() => {
    if (focusRequested.current && draft.editing && tab === 'commit') {
      textareaRef.current?.focus(); focusRequested.current = false;
    }
  }, [draft.editing, tab, viewKey]);

  function notify(text: string, key = viewKey) { setFeedback((previous) => ({ ...previous, [key]: text })); }
  async function execute(command: WorkbenchCommand, preserveFeedback = false): Promise<boolean> {
    const targetKey = 'target' in command ? command.target.worktreeId : viewKey;
    const branching = command.kind === 'createBranch' || command.kind === 'createWorktree' || command.kind === 'switchBranch' ? command : undefined;
    const committing = command.kind === 'commit' ? command : undefined;
    const key = branching || committing ? `${(branching ?? committing)!.target.repositoryId}:write` : command.kind === 'openRepository' ? 'global:openRepository' : command.kind === 'refresh' ? `${command.target.repositoryId}:refresh` : `${targetKey}:${command.kind}`;
    if (inFlight.current.has(key)) return false;
    inFlight.current.add(key); setPending([...inFlight.current]);
    const reading = command.kind === 'openRepository' || command.kind === 'refresh' ? command : undefined;
    const readKey = reading?.kind === 'openRepository' ? 'open' : reading?.target.repositoryId;
    if (reading && readKey) dispatch({ type: 'repositoryReadStarted', key: readKey, requestId: reading.requestId });
    if (branching) dispatch({ type: 'repositoryBranchStarted', repositoryId: branching.target.repositoryId, requestId: branching.requestId });
    if (committing) dispatch({ type: 'repositoryCommitStarted', repositoryId: committing.target.repositoryId, worktreeId: committing.target.worktreeId, requestId: committing.requestId, fileIds: committing.fileIds });
    try {
      const reply = await backend.execute(command);
      if (committing) {
        if (!reply || !('kind' in reply) || reply.kind !== 'commit' || reply.requestId !== committing.requestId) throw new Error('Git 未返回匹配的提交结果，请刷新核对实际历史。');
        dispatch({ type: 'repositoryCommitFinished', repositoryId: committing.target.repositoryId, reply });
        notify(`已在本地提交 ${reply.commit.shortOid}：${reply.commit.title}（尚未推送）`, targetKey);
      }
      if (branching) {
        if (!reply || !('kind' in reply) || reply.kind !== 'branch' || reply.requestId !== branching.requestId) throw new Error('Git 未返回匹配的分支结果，请刷新核对实际状态。');
        dispatch({ type: 'repositoryBranchFinished', repositoryId: branching.target.repositoryId, reply });
        if (reply.outcome === 'cancelled') { notify('已取消创建 Worktree，分支名、草稿与选择已保留。', targetKey); return false; }
        notify(branching.kind === 'createWorktree' ? 'Worktree 已创建；原工作树的改动留在原处。' : `已切换到 ${branching.branch}，草稿和选择已保留。`, reply.snapshot!.activeWorktreeId);
      }
      if (reading && readKey) {
        if (!reply || reply.requestId !== reading.requestId) throw new Error('Git 未返回匹配的读取结果，当前草稿与选择已保留。');
        dispatch({ type: 'repositoryReadFinished', key: readKey, reply });
        if (!preserveFeedback) notify('', targetKey);
      }
      return true;
    } catch (error) {
      const message = error instanceof Error ? error.message : typeof error === 'string' ? error : '操作未完成，当前草稿与选择已保留。';
      if (committing) {
        dispatch({ type: 'repositoryCommitFailed', repositoryId: committing.target.repositoryId, requestId: committing.requestId });
        if (backend.repositoryReadsAvailable && !committing.push) void execute({ kind: 'refresh', target: committing.target, requestId: crypto.randomUUID() }, true);
      }
      if (branching) {
        dispatch({ type: 'repositoryBranchFailed', repositoryId: branching.target.repositoryId, requestId: branching.requestId });
        if (backend.repositoryReadsAvailable) void execute({ kind: 'refresh', target: branching.target, requestId: crypto.randomUUID() }, true);
      }
      if (reading && readKey) dispatch({ type: 'repositoryReadFailed', key: readKey, requestId: reading.requestId, ...(reading.kind === 'refresh' ? { worktreeId: reading.target.worktreeId, error: message } : {}) });
      notify(message, targetKey); return false;
    } finally { inFlight.current.delete(key); setPending([...inFlight.current]); }
  }
  function setTab(next: 'commit' | 'review') {
    if (worktree) dispatch({ type: 'tab', worktreeId: worktree.worktreeId, tab: next });
    else setEmptyTab(next);
  }
  function editDraft() {
    focusRequested.current = true;
    dispatch({ type: 'edit', worktreeId: worktree?.worktreeId ?? null });
  }
  async function runModel(kind: 'generate' | 'review', changedOnly = false) {
    if (!worktree || !model || !canUseModel || worktree.requests[kind]) return;
    const target = targetOf(worktree), requestId = crypto.randomUUID();
    const fileIds = selected.filter((file) => !changedOnly || worktree.review.changedPaths.includes(file.path)).map((file) => file.id);
    if (!fileIds.length) { notify('当前所选范围内没有待复审的变动文件。'); return; }
    dispatch({ type: 'request', target, kind, requestId, fileIds });
    const accepted = await execute({ kind, target, requestId, fileIds, modelId: model.id, draft: draft.text,
      ...(changedOnly ? { changedPaths: worktree.review.changedPaths } : {}) });
    if (!accepted) dispatch({ type: 'requestFailed', target, kind, requestId });
  }
  function switchWorktree(worktreeId: string, expand?: boolean) {
    dispatch({ type: 'switch', worktreeId, expand });
    const next = state.worktrees.find((tree) => tree.worktreeId === worktreeId);
    if (backend.repositoryReadsAvailable && next) void execute({ kind: 'refresh', target: targetOf(next), requestId: crypto.randomUUID() });
  }
  function openFile(fileId: string, line?: number) {
    if (worktree) void execute({ kind: 'openFile', target: targetOf(worktree), requestId: crypto.randomUUID(), fileId, editor: state.editor, line });
  }
  function primary() {
    if (!worktree) return;
    if (isActiveSession(worktree)) { dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded: true }); return; }
    if (worktree.detached) { notify('当前处于分离 HEAD，请在顶部新建或切换开发分支。草稿和所选范围已保留。'); return; }
    if (isProtected(worktree.branch)) { notify('当前为保护分支，请在顶部新建或切换开发分支。草稿和所选范围已保留。'); return; }
    if (model && selected.length && !worktree.readError) void execute({ kind: 'cPush', target: targetOf(worktree), requestId: crypto.randomUUID(), fileIds: selected.map((file) => file.id), modelId: model.id, draft: draft.text });
  }
  async function commit(confirmed: Change[], push: boolean) {
    if (!worktree || worktree.readError || !confirmed.length || worktree.detached || isProtected(worktree.branch) || !draft.text.trim()) return false;
    const selections = confirmed.every((file) => !!file.commitToken)
      ? confirmed.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    return execute({ kind: 'commit', target: targetOf(worktree), requestId: crypto.randomUUID(), fileIds: confirmed.map((file) => file.id), selections, draft: draft.text, push });
  }
  async function respond(message: Message, answer: { optionId?: string; text?: string }) {
    if (!worktree) return;
    const id = worktree.worktreeId;
    if (await execute({ kind: 'respond', target: targetOf(worktree), requestId: crypto.randomUUID(), messageId: message.id, ...answer })) {
      dispatch({ type: 'reply', worktreeId: id, messageId: message.id, text: '' });
    }
  }
  async function selectModel(id: string) {
    if (await execute({ kind: 'selectModel', modelId: id })) dispatch({ type: 'modelSelected', modelId: id });
  }
  async function deleteModel(id: string) {
    if (await execute({ kind: 'deleteModel', modelId: id })) dispatch({ type: 'modelDeleted', modelId: id });
  }
  async function pin() {
    if (pinInFlight.current) return;
    pinInFlight.current = true; setPinBusy(true);
    try { setPinned(await setWindowPinned(!pinned)); }
    catch (error) { notify(error instanceof Error ? error.message : '无法设置窗口置顶'); }
    finally { pinInFlight.current = false; setPinBusy(false); }
  }
  const highCount = worktree?.review.findings.filter((finding) => finding.severity === 'high').length ?? 0;
  const suggestionCount = (worktree?.review.findings.length ?? 0) - highCount;
  const reviewLabel = worktree?.review.status === 'running' ? '审查中' : worktree?.review.status === 'done'
    ? worktree.review.changedPaths.length || selected.some((file) => !worktree.review.scopeFileIds?.includes(file.id)) ? '已变化' : highCount ? `${highCount} 高危${suggestionCount ? ` · ${suggestionCount} 建议` : ''}` : suggestionCount ? `${suggestionCount} 建议` : '通过' : '';

  return <main className="workbench" aria-label="Picket 工作台">
    <div className="workbench-content" inert={settingsOpen || addModelOpen}>
      <ContextHeader key={`header:${viewKey}`} state={state} worktree={worktree} repository={repository} opening={opening} branchBusy={branchBusy} pinned={pinned} pinBusy={pinBusy} onPin={() => void pin()} onSettings={() => setSettingsOpen(true)}
        onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID() })} onSwitch={switchWorktree}
        onBranch={async (kind, branch) => worktree ? execute({ kind, branch, branchToken: worktree.branchToken ?? '', target: targetOf(worktree), requestId: crypto.randomUUID() }) : false}
        onPull={() => { if (worktree) void execute({ kind: 'pull', target: targetOf(worktree), requestId: crypto.randomUUID() }); }} />
      {feedback[viewKey] && <div className="operation-notice" role="alert"><span>{feedback[viewKey]}</span><IconButton icon="close" label="关闭操作提示" onClick={() => notify('')} /></div>}
      <ChangesPanel worktree={worktree} editorShort={editor.short} opening={opening} repositoryReadsAvailable={!!backend.repositoryReadsAvailable} onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID() })}
        onSelect={(fileId) => { if (worktree) dispatch({ type: 'select', worktreeId: worktree.worktreeId, fileId }); }} onOpenFile={openFile} onReview={() => setTab('review')}
        refreshing={refreshing} onRefresh={() => { if (worktree) void execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID() }); }} />
      <section className="lower-workspace" aria-label="说明、审查与消息">
        <div className="tab-workspace" inert={!!worktree?.dockExpanded}>
          <div className="tab-heading"><div className="tab-list" role="tablist" aria-label="工作区">
            <button type="button" id="commit-tab" role="tab" aria-selected={tab === 'commit'} aria-controls="commit-panel" tabIndex={tab === 'commit' ? 0 : -1} onKeyDown={(event) => { if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { setTab('review'); document.getElementById('review-tab')?.focus(); } }} onClick={() => setTab('commit')}>提交说明{draft.outdated && <span className="status-dot warning" title="变更已更新" />}</button>
            <button type="button" id="review-tab" role="tab" aria-selected={tab === 'review'} aria-controls="review-panel" tabIndex={tab === 'review' ? 0 : -1} onKeyDown={(event) => { if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { setTab('commit'); document.getElementById('commit-tab')?.focus(); } }} onClick={() => setTab('review')}>审查意见{reviewLabel && <span className={`small ${highCount ? 'danger' : 'muted'}`}>{reviewLabel}</span>}</button>
          </div><div className="row">
            {tab === 'commit' && (draft.editing || draft.text) && <><IconButton icon="refresh" label="重新生成提交说明" disabled={!canUseModel || !!worktree?.requests.generate} onClick={() => void runModel('generate')} /><IconButton icon="close" label="清空提交说明" onClick={() => dispatch({ type: 'clear', worktreeId: worktree?.worktreeId ?? null })} /></>}
            {tab === 'review' && worktree?.review.status === 'done' && <IconButton icon="refresh" label="重新审查已选范围" disabled={!canUseModel} onClick={() => void runModel('review')} />}
          </div></div>
          <div role="tabpanel" id={tab === 'commit' ? 'commit-panel' : 'review-panel'} aria-labelledby={tab === 'commit' ? 'commit-tab' : 'review-tab'} className="tab-panel">
            {tab === 'commit' ? <CommitPanel text={draft.text} editing={draft.editing} outdated={draft.outdated} textareaRef={textareaRef} onChange={(text) => dispatch({ type: 'draft', worktreeId: worktree?.worktreeId ?? null, text })} onEdit={editDraft} hasModel={!!model} canGenerate={canUseModel} generating={!!worktree?.requests.generate} onGenerate={() => void runModel('generate')} onAddModel={() => setAddModelOpen(true)} />
              : <ReviewPanel worktree={worktree} hasModel={!!model} canReview={canUseModel} onReview={(changedOnly) => void runModel('review', changedOnly)} onAddModel={() => setAddModelOpen(true)} onOpenFile={openFile} />}
          </div>
        </div>
        {worktree && <MessageDock worktree={worktree} busy={busy} editorShort={editor.short} onExpanded={(expanded) => dispatch({ type: 'dock', worktreeId: worktree.worktreeId, expanded })} onReply={(messageId, text) => dispatch({ type: 'reply', worktreeId: worktree.worktreeId, messageId, text })} onRespond={(message, answer) => void respond(message, answer)} onOpenFile={openFile} onReview={() => setTab('review')} />}
      </section>
      <ActionFooter key={`footer:${viewKey}`} worktree={worktree} models={state.models} activeModelId={state.activeModelId} busy={busy} onAddModel={() => setAddModelOpen(true)} onSelectModel={(id) => void selectModel(id)} onPrimary={primary} onCommit={commit} />
    </div>
    {settingsOpen && <SettingsPanel editor={state.editor} models={state.models} activeModelId={state.activeModelId} onEditor={(editor) => dispatch({ type: 'editor', editor })} onSelectModel={(id) => void selectModel(id)} onDeleteModel={(id) => void deleteModel(id)} covered={addModelOpen} error={feedback[viewKey]} onClose={() => setSettingsOpen(false)} onAddModel={() => setAddModelOpen(true)} />}
    {addModelOpen && <AddModelForm onClose={() => setAddModelOpen(false)} test={(input) => backend.testModel(input)} save={(input) => backend.saveModel(input)} onSaved={(model) => { dispatch({ type: 'modelSaved', model }); setAddModelOpen(false); setSettingsOpen(false); }} />}
  </main>;
}
