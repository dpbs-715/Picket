import { useEffect, useLayoutEffect, useReducer, useRef, useState } from 'react';
import { readWindowPinned, setWindowPinned } from '../../desktop/window';
import { Icon, IconButton } from '../../ui/controls';
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
import { serializeWorkbenchPersistence } from './state/repositories';
import { editors, emptyWorkbench, isActiveSession, selectedFiles, splitPath, targetOf, workbenchReducer } from './state/workbench';
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
  const cancelledRequests = useRef(new Set<string>());
  const [pinned, setPinned] = useState(false);
  const [pinBusy, setPinBusy] = useState(false);
  const pinInFlight = useRef(false);
  const [hydrated, setHydrated] = useState(() => !!initialState || !backend.loadWorkbenchState);
  const saveRevision = useRef(0);
  const lastPersistedKey = useRef<string | null>(null);
  const lastStructuralKey = useRef<string | null>(null);
  const latestState = useRef(state);
  latestState.current = state;
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
  const pullBusy = !!worktree && pending.includes(`${worktree.repositoryId}:pull`);
  const busy = refreshing || branchBusy || pullBusy || pending.some((key) => key.startsWith(`${viewKey}:`) && !key.endsWith(':generate') && !key.endsWith(':review'));

  useEffect(() => backend.subscribe?.((event) => dispatch({ type: 'event', event })), [backend]);
  useEffect(() => {
    if (initialState || !backend.loadWorkbenchState) return;
    let mounted = true;
    void backend.loadWorkbenchState().then((restored) => {
      if (!mounted) return;
      dispatch({ type: 'workbenchHydrated', restored });
      if (restored.warning) {
        const key = restored.activeWorktreeId ?? restored.snapshots[0]?.activeWorktreeId ?? 'empty';
        setFeedback((previous) => ({ ...previous, [key]: restored.warning! }));
      }
      setHydrated(true);
    }).catch((error: unknown) => {
      if (!mounted) return;
      const message = error instanceof Error ? error.message : '无法读取本地工作台保存记录。';
      setFeedback((previous) => ({ ...previous, empty: message }));
      setHydrated(true);
    });
    return () => { mounted = false; };
  }, [backend, initialState]);
  const persistenceSnapshot = serializeWorkbenchPersistence(state, 0);
  const persistenceKey = JSON.stringify(persistenceSnapshot);
  const structuralKey = JSON.stringify([
    state.editor,
    state.activeWorktreeId,
    state.emptyEditing,
    state.worktrees.map((tree) => [tree.worktreeId, tree.selectedIds, tree.scopeVersion, tree.tab, tree.draft.editing, tree.draft.outdated]),
  ]);
  useEffect(() => {
    if (!hydrated || !backend.saveWorkbenchState) return;
    if (lastPersistedKey.current === null) {
      lastStructuralKey.current = structuralKey;
      if (!state.editorModified && state.navigationRevision === 0 && !state.emptyEditing && !state.emptyDraft) {
        lastPersistedKey.current = persistenceKey;
        return;
      }
    }
    if (persistenceKey === lastPersistedKey.current) {
      lastStructuralKey.current = structuralKey;
      return;
    }
    const immediate = structuralKey !== lastStructuralKey.current;
    lastStructuralKey.current = structuralKey;
    const persistNow = () => {
      const current = serializeWorkbenchPersistence(latestState.current, 0);
      const currentKey = JSON.stringify(current);
      if (currentKey === lastPersistedKey.current) return;
      lastPersistedKey.current = currentKey;
      const revision = ++saveRevision.current;
      void backend.saveWorkbenchState!({ ...current, revision }).catch((error: unknown) => {
        const message = error instanceof Error ? error.message : '无法保存本地草稿状态；当前窗口输入仍已保留。';
        setFeedback((previous) => ({ ...previous, [latestState.current.activeWorktreeId ?? 'empty']: message }));
      });
    };
    if (immediate) {
      persistNow();
      return;
    }
    const timer = setTimeout(persistNow, 80);
    return () => clearTimeout(timer);
  }, [backend, hydrated, persistenceKey, structuralKey]);
  useEffect(() => {
    if (!hydrated || !backend.saveWorkbenchState) return;
    const flush = () => {
      const current = serializeWorkbenchPersistence(latestState.current, 0);
      const currentKey = JSON.stringify(current);
      if (currentKey === lastPersistedKey.current) return;
      lastPersistedKey.current = currentKey;
      const revision = ++saveRevision.current;
      void backend.saveWorkbenchState!({ ...current, revision }).catch(() => {});
    };
    window.addEventListener('pagehide', flush);
    return () => window.removeEventListener('pagehide', flush);
  }, [backend, hydrated]);
  useEffect(() => {
    let mounted = true;
    void readWindowPinned().then((pinned) => { if (mounted) setPinned(pinned); }).catch(() => {
      if (mounted) setFeedback((previous) => ({ ...previous, [initialState?.activeWorktreeId ?? 'empty']: '无法读取窗口置顶状态，请重试窗口置顶操作。' }));
    });
    return () => { mounted = false; };
  }, []);
  useEffect(() => {
    if (!backend.repositoryReadsAvailable || !worktree) return;
    const refresh = () => {
      if (inFlight.current.has('global:openRepository')) return;
      void execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID() }, true);
    };
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
    const branching = command.kind === 'switchBranch' ? command : undefined;
    const committing = command.kind === 'commit' ? command : undefined;
    const pulling = command.kind === 'pull' ? command : undefined;
    const key = branching || committing ? `${(branching ?? committing)!.target.repositoryId}:write` : pulling ? `${pulling.target.repositoryId}:pull` : command.kind === 'openRepository' ? 'global:openRepository' : command.kind === 'refresh' ? `${command.target.repositoryId}:refresh` : `${targetKey}:${command.kind}`;
    if (inFlight.current.has(key)) return false;
    inFlight.current.add(key); setPending([...inFlight.current]);
    const reading = command.kind === 'openRepository' || command.kind === 'refresh' ? command : undefined;
    const readKey = reading?.kind === 'openRepository' ? 'open' : reading?.target.repositoryId;
    if (reading && readKey) dispatch({ type: 'repositoryReadStarted', key: readKey, requestId: reading.requestId });
    if (branching) dispatch({ type: 'repositoryBranchStarted', repositoryId: branching.target.repositoryId, requestId: branching.requestId });
    if (committing) dispatch({ type: 'repositoryCommitStarted', repositoryId: committing.target.repositoryId, worktreeId: committing.target.worktreeId, requestId: committing.requestId, fileIds: committing.fileIds });
    try {
      const reply = await backend.execute(command);
      if (command.kind === 'selectModel' || command.kind === 'deleteModel') {
        if (reply && typeof reply === 'object' && 'models' in reply) {
          dispatch({ type: 'modelsSynced', models: reply.models, activeModelId: reply.activeModelId });
        }
        return true;
      }
      if (command.kind === 'generate') {
        if (cancelledRequests.current.delete(command.requestId)) return false;
        if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'generate') {
          dispatch({ type: 'event', event: { type: 'generated', target: reply.target, requestId: reply.requestId, text: reply.text } });
        }
        return true;
      }
      if (command.kind === 'review') {
        if (cancelledRequests.current.delete(command.requestId)) return false;
        if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'review') {
          dispatch({
            type: 'event',
            event: {
              type: 'reviewed',
              target: reply.target,
              requestId: reply.requestId,
              findings: reply.findings,
              skippedPaths: reply.skippedPaths,
              reviewedFileIds: reply.reviewedFileIds,
            },
          });
          const targetTree = latestState.current.worktrees.find((item) => item.worktreeId === reply.target.worktreeId);
          if (targetTree && isActiveSession(targetTree)) {
            const high = reply.findings.filter((item) => item.severity === 'high').length;
            const sug = reply.findings.length - high;
            const summary = reply.findings.length === 0 ? '未发现风险项' : `${high ? `${high} 项高危` : ''}${high && sug ? '、' : ''}${sug ? `${sug} 项建议` : ''}`;
            dispatch({
              type: 'event',
              event: {
                type: 'message',
                target: reply.target,
                message: { id: 'review-notice', kind: 'review', title: `审查完成：${summary}`, description: '已更新审查意见。' },
              },
            });
          }
        }
        return true;
      }
      if (command.kind === 'cPush' || command.kind === 'respond' || command.kind === 'pull') {
        if (reply && typeof reply === 'object' && 'kind' in reply && reply.kind === 'cPush') {
          const expandDock = command.kind === 'pull'
            && document.activeElement !== textareaRef.current
            && reply.session.sessionStatus !== 'idle'
            && !reply.session.autoCollapse;
          dispatch({ type: 'cPushSettled', reply, expandDock });
          if (command.kind === 'pull') {
            if (reply.session.sessionStatus === 'idle') {
              const syncedRemote = reply.repositorySnapshot?.worktrees.find((item) => item.worktreeId === command.target.worktreeId)?.remote;
              notify(syncedRemote?.kind === 'tracked' ? `已完成拉取并变基（${syncedRemote.branch}），工作区改动已保全。` : '已完成拉取并变基。', targetKey);
            } else {
              notify('', targetKey);
            }
          }
        }
        return true;
      }
      if (command.kind === 'openFile') {
        if (!reply || !('kind' in reply) || reply.kind !== 'openFile' || reply.requestId !== command.requestId) {
          throw new Error('桌面层未返回匹配的编辑器联动结果。');
        }
        const shortName = editors.find((item) => item.id === reply.editor)?.short ?? reply.editor;
        const fileName = splitPath(reply.filePath).name;
        const detail = reply.action === 'goto' ? `定位到 ${fileName}:${reply.line}`
          : reply.action === 'merge' ? `打开冲突合并：${fileName}`
          : reply.action === 'stagedDiff' ? `打开已暂存 Diff：${fileName}`
          : reply.action === 'open' ? `打开文件：${fileName}`
          : `打开 Diff：${fileName}`;
        notify(`已在 ${shortName} ${detail}`, targetKey);
      }
      if (committing) {
        if (committing.push) {
          if (!reply || !('kind' in reply) || reply.kind !== 'cPush' || reply.requestId !== committing.requestId) {
            throw new Error('Git 未返回匹配的提交与推送结果，请刷新核对实际历史。');
          }
          const expandDock = reply.session.sessionStatus !== 'idle'
            && !reply.session.autoCollapse;
          dispatch({ type: 'cPushSettled', reply, expandDock });
          if (reply.session.sessionStatus === 'done') {
            notify(reply.session.message?.title ?? '已提交并推送至远端。', targetKey);
          } else {
            notify('', targetKey);
          }
        } else {
          if (!reply || !('kind' in reply) || reply.kind !== 'commit' || reply.requestId !== committing.requestId) throw new Error('Git 未返回匹配的提交结果，请刷新核对实际历史。');
          dispatch({ type: 'repositoryCommitFinished', repositoryId: committing.target.repositoryId, reply });
          notify(`已在本地提交 ${reply.commit.shortOid}：${reply.commit.title}（尚未推送）`, targetKey);
        }
      }
      if (branching) {
        if (!reply || !('kind' in reply) || reply.kind !== 'branch' || reply.requestId !== branching.requestId) throw new Error('Git 未返回匹配的分支结果，请刷新核对实际状态。');
        dispatch({ type: 'repositoryBranchFinished', repositoryId: branching.target.repositoryId, reply });
        notify(`已切换到 ${branching.branch}，草稿和选择已保留。`, reply.snapshot.activeWorktreeId);
      }
      if (reading && readKey) {
        if (!reply || ('kind' in reply && reply.kind !== undefined) || !('requestId' in reply) || reply.requestId !== reading.requestId) throw new Error('Git 未返回匹配的读取结果，当前草稿与选择已保留。');
        const previousTree = latestState.current.worktrees.find((item) => item.worktreeId === targetKey);
        const previousReadError = previousTree?.readError;
        dispatch({ type: 'repositoryReadFinished', key: readKey, reply });
        if (reading.kind === 'openRepository') {
          if (reply.snapshot && !preserveFeedback) notify('', targetKey);
        } else {
          let recoveredFromReadError = false;
          if (previousReadError) {
            const refreshedTree = reply.snapshot?.worktrees.find((item) => item.worktreeId === targetKey);
            recoveredFromReadError = !!refreshedTree && !refreshedTree.readError;
          }
          if (!preserveFeedback || recoveredFromReadError) notify('', targetKey);
          const sessionStatus = previousTree?.sessionStatus;
          if (sessionStatus && ['waitingManual', 'conflict', 'pushFailed'].includes(sessionStatus)) {
            void backend.reconcileSession?.(reading.target).catch(() => {});
          }
        }
      }
      return true;
    } catch (error) {
      if ('requestId' in command && cancelledRequests.current.delete(command.requestId)) {
        return false;
      }
      const message = error instanceof Error ? error.message : typeof error === 'string' ? error : '操作未完成，当前草稿与选择已保留。';
      if (committing) {
        dispatch({ type: 'repositoryCommitFailed', repositoryId: committing.target.repositoryId, requestId: committing.requestId });
        if (backend.repositoryReadsAvailable) void execute({ kind: 'refresh', target: committing.target, requestId: crypto.randomUUID() }, true);
      }
      if (command.kind === 'cPush') {
        dispatch({ type: 'cPushStartFailed', worktreeId: command.target.worktreeId, runId: command.requestId });
      }
      if (pulling && backend.repositoryReadsAvailable) {
        void execute({ kind: 'refresh', target: pulling.target, requestId: crypto.randomUUID() }, true);
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
    const targetFiles = selected.filter((file) => !changedOnly || worktree.review.changedPaths.includes(file.path));
    const fileIds = targetFiles.map((file) => file.id);
    if (!fileIds.length) { notify('当前所选范围内没有待复审的变动文件。'); return; }
    const selections = targetFiles.every((file) => !!file.commitToken)
      ? targetFiles.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    dispatch({ type: 'request', target, kind, requestId, fileIds });
    const accepted = await execute({ kind, target, requestId, fileIds, selections, modelId: model.id, draft: draft.text,
      ...(changedOnly ? { changedPaths: worktree.review.changedPaths } : {}) });
    if (!accepted) dispatch({ type: 'requestFailed', target, kind, requestId });
  }
  function cancelModel(kind: 'generate' | 'review') {
    const active = worktree?.requests[kind];
    if (!worktree || !active) return;
    cancelledRequests.current.add(active.id);
    dispatch({ type: 'requestFailed', target: targetOf(worktree), kind, requestId: active.id });
    void backend.cancelRequest?.(active.id).catch(() => {});
    notify(kind === 'generate' ? '已取消生成提交说明；草稿与选择已保留。' : '已取消代码审查；草稿与选择已保留。');
  }
  function cancelActiveModelRequests(tree: typeof worktree) {
    if (!tree) return;
    for (const kind of ['generate', 'review'] as const) {
      const active = tree.requests[kind];
      if (active) {
        cancelledRequests.current.add(active.id);
        void backend.cancelRequest?.(active.id).catch(() => {});
      }
    }
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
    if (inFlight.current.has(`${worktree.worktreeId}:cPush`)) return;
    if (worktree.detached) { notify('当前处于分离 HEAD，请先在顶部切换已有分支，或在 IDE / 终端切好分支。草稿和所选范围已保留。'); return; }
    if (model && selected.length && !worktree.readError) {
      const requestId = crypto.randomUUID();
      const target = targetOf(worktree);
      const fileIds = selected.map((file) => file.id);
      const selections = selected.every((file) => !!file.commitToken)
        ? selected.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
        : undefined;
      const expandDock = document.activeElement !== textareaRef.current;
      dispatch({ type: 'cPushStarted', worktreeId: worktree.worktreeId, runId: requestId, frozenTarget: target, frozenFileIds: fileIds, modelId: model.id, expandDock });
      void execute({ kind: 'cPush', target, requestId, fileIds, selections, modelId: model.id, draft: draft.text });
    }
  }
  async function commit(confirmed: Change[], push: boolean) {
    if (!worktree || worktree.readError || !confirmed.length || worktree.detached || !draft.text.trim()) return false;
    const selections = confirmed.every((file) => !!file.commitToken)
      ? confirmed.map((file) => ({ fileId: file.id, commitToken: file.commitToken! }))
      : undefined;
    return execute({ kind: 'commit', target: targetOf(worktree), requestId: crypto.randomUUID(), fileIds: confirmed.map((file) => file.id), selections, draft: draft.text, push, ...(model ? { modelId: model.id } : {}) });
  }
  async function respond(message: Message, answer: { optionId?: string; text?: string }) {
    if (!worktree) return;
    const id = worktree.worktreeId;
    const target = worktree.cPush?.frozenTarget ?? targetOf(worktree);
    const runId = worktree.cPush?.runId;
    if (await execute({ kind: 'respond', target, requestId: crypto.randomUUID(), ...(runId ? { runId } : {}), ...(model ? { modelId: model.id } : {}), messageId: message.id, ...answer })) {
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
  const noFilesReviewed = worktree?.review.reviewedFileIds !== undefined && worktree.review.reviewedFileIds.length === 0 && (worktree.review.skippedPaths.length ?? 0) > 0;
  const reviewLabel = worktree?.review.status === 'running' ? '审查中' : worktree?.review.status === 'done'
    ? worktree.review.changedPaths.length || selected.some((file) => !worktree.review.scopeFileIds?.includes(file.id)) ? '已变化' : highCount ? `${highCount} 高危${suggestionCount ? ` · ${suggestionCount} 建议` : ''}` : suggestionCount ? `${suggestionCount} 建议` : noFilesReviewed ? '未覆盖' : '通过' : '';

  return <main className="workbench" aria-label="Picket 工作台">
    <div className="workbench-content" inert={settingsOpen || addModelOpen}>
      <ContextHeader key={`header:${viewKey}`} state={state} worktree={worktree} repository={repository} opening={opening} branchBusy={branchBusy} pullBusy={pullBusy} pinned={pinned} pinBusy={pinBusy} onPin={() => void pin()} onSettings={() => setSettingsOpen(true)}
        onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID() })} onSwitch={switchWorktree}
        onBranch={async (kind, branch) => worktree ? execute({ kind, branch, branchToken: worktree.branchToken ?? '', target: targetOf(worktree), requestId: crypto.randomUUID() }) : false}
        onPull={() => { if (worktree) void execute({ kind: 'pull', target: targetOf(worktree), requestId: crypto.randomUUID(), ...(model ? { modelId: model.id } : {}) }); }} />
      {feedback[viewKey] && <div className="operation-notice" role="alert"><span className="operation-notice-body"><Icon name="alert" className="operation-notice-icon" /><span>{feedback[viewKey]}</span></span><IconButton icon="close" label="关闭操作提示" onClick={() => notify('')} /></div>}
      <ChangesPanel worktree={worktree} editorShort={editor.short} opening={opening} repositoryReadsAvailable={!!backend.repositoryReadsAvailable} onOpenRepository={() => void execute({ kind: 'openRepository', requestId: crypto.randomUUID() })}
        onSelect={(fileId) => { if (worktree) { cancelActiveModelRequests(worktree); dispatch({ type: 'select', worktreeId: worktree.worktreeId, fileId }); } }} onOpenFile={openFile} onReview={() => setTab('review')}
        refreshing={refreshing} onRefresh={() => { if (worktree) void execute({ kind: 'refresh', target: targetOf(worktree), requestId: crypto.randomUUID() }); }} />
      <section className="lower-workspace" aria-label="说明、审查与消息">
        <div className="tab-workspace" inert={!!worktree?.dockExpanded}>
          <div className="tab-heading"><div className="tab-list" role="tablist" aria-label="工作区">
            <button type="button" id="commit-tab" role="tab" aria-selected={tab === 'commit'} aria-controls="commit-panel" tabIndex={tab === 'commit' ? 0 : -1} onKeyDown={(event) => { if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { setTab('review'); document.getElementById('review-tab')?.focus(); } }} onClick={() => setTab('commit')}>提交说明{draft.outdated && <span className="status-dot warning" title="变更已更新" />}</button>
            <button type="button" id="review-tab" role="tab" aria-selected={tab === 'review'} aria-controls="review-panel" tabIndex={tab === 'review' ? 0 : -1} onKeyDown={(event) => { if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { setTab('commit'); document.getElementById('commit-tab')?.focus(); } }} onClick={() => setTab('review')}>审查意见{reviewLabel && <span className={`small ${highCount ? 'danger' : 'muted'}`}>{reviewLabel}</span>}</button>
          </div><div className="row">
            {tab === 'commit' && (draft.editing || draft.text) && <><IconButton icon="refresh" label="重新生成提交说明" disabled={!canUseModel || !!worktree?.requests.generate} onClick={() => void runModel('generate')} /><IconButton icon="close" label="清空提交说明" onClick={() => { if (worktree?.requests.generate) { cancelledRequests.current.add(worktree.requests.generate.id); void backend.cancelRequest?.(worktree.requests.generate.id).catch(() => {}); } dispatch({ type: 'clear', worktreeId: worktree?.worktreeId ?? null }); }} /></>}
            {tab === 'review' && worktree?.review.status === 'done' && <IconButton icon="refresh" label="重新审查已选范围" disabled={!canUseModel} onClick={() => void runModel('review')} />}
          </div></div>
          <div role="tabpanel" id={tab === 'commit' ? 'commit-panel' : 'review-panel'} aria-labelledby={tab === 'commit' ? 'commit-tab' : 'review-tab'} className="tab-panel">
            {tab === 'commit' ? <CommitPanel text={draft.text} editing={draft.editing} outdated={draft.outdated} hasWorktree={!!worktree} textareaRef={textareaRef} onChange={(text) => dispatch({ type: 'draft', worktreeId: worktree?.worktreeId ?? null, text })} onEdit={editDraft} hasModel={!!model} canGenerate={canUseModel} generating={!!worktree?.requests.generate} onGenerate={() => void runModel('generate')} onCancelGenerate={() => cancelModel('generate')} onAddModel={() => setAddModelOpen(true)} />
              : <ReviewPanel worktree={worktree} hasModel={!!model} canReview={canUseModel} onReview={(changedOnly) => void runModel('review', changedOnly)} onCancel={() => cancelModel('review')} onAddModel={() => setAddModelOpen(true)} onOpenFile={openFile} />}
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
