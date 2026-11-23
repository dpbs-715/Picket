import { useReducer, useState } from 'react';
import { cancelPiProbe, inDesktop, runGitProbe, runPiProbe, type GitProbeReport } from '../../desktop/foundation';
import { initialProbeState, probeReducer } from './probe-state';
import './foundation.css';

const statusLabels = { idle: '未验证', running: '验证中', completed: '进程已正常退出', cancelled: '已取消并退出', failed: '验证失败' };
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export function Foundation() {
  const [pi, dispatch] = useReducer(probeReducer, initialProbeState);
  const [git, setGit] = useState<{ status: 'idle' | 'running' | 'done' | 'failed'; report?: GitProbeReport; error?: string }>({ status: 'idle' });
  const [notice, setNotice] = useState('');
  const desktop = inDesktop();

  async function startProbe() {
    const requestId = crypto.randomUUID();
    setNotice('');
    dispatch({ type: 'start', requestId });
    try {
      const report = await runPiProbe(requestId, (event) => dispatch({ type: 'event', requestId, event }));
      dispatch({ type: 'settled', report });
    } catch (error) { dispatch({ type: 'failed', requestId, error: errorText(error) }); }
  }
  async function cancelProbe() {
    if (!pi.requestId) return;
    try { await cancelPiProbe(pi.requestId); }
    catch (error) { setNotice(errorText(error)); }
  }
  async function checkGit() {
    setGit({ status: 'running' });
    try { setGit({ status: 'done', report: await runGitProbe() }); }
    catch (error) { setGit({ status: 'failed', error: errorText(error) }); }
  }

  return <main className="foundation">
    <header><strong>Picket</strong><span>M0 · 基础验证</span></header>
    <div className="intro"><h1>先把底层跑通</h1><p>这不是定稿工作台。当前只验证桌面、Pi 进程和安全提交方案，不操作你的项目仓库。</p></div>
    {!desktop && <p className="notice">当前是浏览器预览。请运行 <code>pnpm desktop dev</code> 使用桌面能力。</p>}
    <section aria-labelledby="pi-heading">
      <div className="section-heading"><h2 id="pi-heading">Pi Core 独立进程</h2><span role="status">{statusLabels[pi.status]}</span></div>
      <p>随包 Node + Pi SDK；脚本化响应，不调用真实模型，不读取凭据，也没有文件或终端工具。</p>
      <div className="actions"><button onClick={() => void startProbe()} disabled={!desktop || pi.status === 'running'}>验证流式通信</button>
        <button className="secondary" onClick={() => void cancelProbe()} disabled={!desktop || pi.status !== 'running'}>取消任务</button></div>
      <pre aria-label="Pi 流式输出">{pi.text || '等待验证…'}</pre>
      {pi.report && <p className="result">Pi {pi.report.piCoreVersion} · Node {pi.report.nodeVersion}<br />
        PID {pi.report.childPid} · {pi.report.deltaCount} 个文本片段 · 退出码 {pi.report.childExitCode}</p>}
      {pi.error && <p className="error" role="alert">{pi.error}</p>}
      {notice && <p className="notice" role="alert">{notice}</p>}
    </section>
    <section aria-labelledby="git-heading">
      <div className="section-heading"><h2 id="git-heading">Git 范围与草稿保全</h2><span role="status">{git.status === 'running' ? '验证中' : git.status === 'done' ? '验证完成' : git.status === 'failed' ? '验证失败' : '未验证'}</span></div>
      <p>创建并删除临时仓库，检查部分暂存、未选已暂存文件、拆分提交与重复执行。</p>
      <button onClick={() => void checkGit()} disabled={!desktop || git.status === 'running'}>验证临时 Git 仓库</button>
      {git.report && <div className="result"><p>{git.report.gitVersion} · {git.report.cases.length} 项通过</p>
        <p>产生 {git.report.commits.length} 个测试提交；临时仓库{git.report.temporaryRepositoryRemoved ? '已删除' : '未删除'}。</p></div>}
      {git.error && <p className="error" role="alert">{git.error}</p>}
    </section>
    <footer>正式 UI 以 docs/design/picket-mvp.tsx 为准。<br />模型、IDE、推送与发布验收尚未接入。</footer>
  </main>;
}
