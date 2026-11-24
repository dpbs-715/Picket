import { useReducer, useState } from 'react';
import { cancelPiProbe, inDesktop, runGitProbe, runPiProbe, type GitProbeReport } from '../../desktop/foundation';
import { useI18n } from '../../i18n';
import { initialProbeState, probeReducer } from './probe-state';
import './foundation.css';

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export function Foundation() {
  const { t } = useI18n();
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
    <header><strong>Picket</strong><span>{t.foundation.phaseLabel}</span></header>
    <div className="intro"><h1>{t.foundation.heading}</h1><p>{t.foundation.intro}</p></div>
    {!desktop && <p className="notice">{t.foundation.browserPreviewNoticePrefix}<code>pnpm desktop dev</code>{t.foundation.browserPreviewNoticeSuffix}</p>}
    <section aria-labelledby="pi-heading">
      <div className="section-heading"><h2 id="pi-heading">{t.foundation.piSectionHeading}</h2><span role="status">{t.foundation.piStatus[pi.status]}</span></div>
      <p>{t.foundation.piDescription}</p>
      <div className="actions"><button onClick={() => void startProbe()} disabled={!desktop || pi.status === 'running'}>{t.foundation.verifyStreamButton}</button>
        <button className="secondary" onClick={() => void cancelProbe()} disabled={!desktop || pi.status !== 'running'}>{t.foundation.cancelTaskButton}</button></div>
      <pre aria-label={t.foundation.streamOutputLabel}>{pi.text || t.foundation.waitingPlaceholder}</pre>
      {pi.report && <p className="result">Pi {pi.report.piCoreVersion} · Node {pi.report.nodeVersion}<br />
        {t.foundation.piReportStats(pi.report.childPid, pi.report.deltaCount, pi.report.childExitCode)}</p>}
      {pi.error && <p className="error" role="alert">{pi.error}</p>}
      {notice && <p className="notice" role="alert">{notice}</p>}
    </section>
    <section aria-labelledby="git-heading">
      <div className="section-heading"><h2 id="git-heading">{t.foundation.gitSectionHeading}</h2><span role="status">{t.foundation.gitStatus[git.status]}</span></div>
      <p>{t.foundation.gitDescription}</p>
      <button onClick={() => void checkGit()} disabled={!desktop || git.status === 'running'}>{t.foundation.verifyTempGitButton}</button>
      {git.report && <div className="result"><p>{t.foundation.gitCasesPassed(git.report.gitVersion, git.report.cases.length)}</p>
        <p>{t.foundation.gitCommitsSummary(git.report.commits.length, git.report.temporaryRepositoryRemoved)}</p></div>}
      {git.error && <p className="error" role="alert">{git.error}</p>}
    </section>
    <footer>{t.foundation.footerLine1}<br />{t.foundation.footerLine2}</footer>
  </main>;
}
