import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import type { CustomActionConfig } from '../../../../shared/custom-action-protocol';
import type { AppLocale } from '../../../../shared/language-protocol';
import { useI18n } from '../../../i18n';
import { IconButton } from '../../../ui/controls';
import { Collapse, useInstantMotion } from '../../../ui/motion';
import type { Change, Worktree } from '../state/workbench';
import type { WorkbenchDispatch } from '../types';
import { CommitPanel } from './CommitPanel';
import { ReviewPanel } from './ReviewPanel';

interface Props {
  locale: AppLocale;
  worktree: Worktree | undefined;
  viewKey: string;
  tab: 'commit' | 'review';
  promptText: string;
  selected: Change[];
  hasModel: boolean;
  canUseModel: boolean;
  canPlan: boolean;
  planOutdated: boolean;
  activeCustomAction: CustomActionConfig | undefined;
  mounted: boolean;
  commitPanelRef: RefObject<HTMLDivElement | null>;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  onSelectTab: (next: 'commit' | 'review') => void;
  onRunPlan: () => void;
  onCancelPlan: () => void;
  onRunReview: (changedOnly?: boolean) => void;
  onCancelReview: () => void;
  onConfigureModel: () => void;
  onOpenModelSettings: () => void;
  onOpenFile: (fileId: string, line?: number) => void;
  onEditAction: (action: CustomActionConfig) => void;
  dispatch: WorkbenchDispatch;
}

export function WorkspaceTabsPanel({
  locale,
  worktree,
  viewKey,
  tab,
  promptText,
  selected,
  hasModel,
  canUseModel,
  canPlan,
  planOutdated,
  activeCustomAction,
  mounted,
  commitPanelRef,
  textareaRef,
  onSelectTab,
  onRunPlan,
  onCancelPlan,
  onRunReview,
  onCancelReview,
  onConfigureModel,
  onOpenModelSettings,
  onOpenFile,
  onEditAction,
  dispatch,
}: Props) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();
  const [promptOpen, setPromptOpen] = useState(false);
  const [tabIndicator, setTabIndicator] = useState<{ left: number; width: number } | null>(null);
  const promptFocusRequestedRef = useRef(false);
  const prevViewKeyRef = useRef(viewKey);
  const prevPromptTextRef = useRef(promptText);
  const tabListRef = useRef<HTMLDivElement>(null);
  const commitTabRef = useRef<HTMLButtonElement>(null);
  const reviewTabRef = useRef<HTMLButtonElement>(null);

  const showPromptBox = promptOpen || promptText.length > 0;
  const highCount = worktree?.review.findings.filter((finding) => finding.severity === 'high').length ?? 0;
  const suggestionCount = (worktree?.review.findings.length ?? 0) - highCount;
  const noFilesReviewed = worktree?.review.reviewedFileIds !== undefined
    && worktree.review.reviewedFileIds.length === 0
    && (worktree.review.skippedPaths.length ?? 0) > 0;
  const reviewLabel = worktree?.review.status === 'running'
    ? t.review.tabStatusRunning
    : worktree?.review.status === 'done'
      ? worktree.review.changedPaths.length || selected.some((file) => !worktree.review.scopeFileIds?.includes(file.id))
        ? t.review.tabStatusChanged
        : highCount || suggestionCount
          ? t.review.tabStatusCounts(highCount, suggestionCount)
          : noFilesReviewed
            ? t.review.tabStatusUncovered
            : t.review.tabStatusPassed
      : '';

  useLayoutEffect(() => {
    if (!promptFocusRequestedRef.current) return;
    promptFocusRequestedRef.current = false;
    textareaRef.current?.focus();
  }, [showPromptBox, textareaRef]);

  useLayoutEffect(() => {
    if (prevViewKeyRef.current !== viewKey) {
      prevViewKeyRef.current = viewKey;
      prevPromptTextRef.current = promptText;
      setPromptOpen(false);
      return;
    }
    if (prevPromptTextRef.current.length > 0 && promptText.length === 0) {
      const focused = typeof document !== 'undefined' && document.activeElement === textareaRef.current;
      if (!focused) {
        setPromptOpen(false);
      }
    }
    prevPromptTextRef.current = promptText;
  }, [promptText, textareaRef, viewKey]);

  const syncTabIndicator = useCallback(() => {
    const activeButton = tab === 'commit' ? commitTabRef.current : reviewTabRef.current;
    if (!activeButton || activeButton.offsetWidth <= 0) {
      setTabIndicator(null);
      return;
    }
    const nextLeft = activeButton.offsetLeft;
    const nextWidth = activeButton.offsetWidth;
    setTabIndicator((previous) =>
      previous && previous.left === nextLeft && previous.width === nextWidth
        ? previous
        : { left: nextLeft, width: nextWidth },
    );
  }, [tab]);

  useLayoutEffect(() => {
    if (!mounted) return;
    syncTabIndicator();
  }, [
    mounted,
    syncTabIndicator,
    locale,
    reviewLabel,
    worktree?.requests.plan,
    worktree?.plan?.groups.length,
    planOutdated,
  ]);

  useEffect(() => {
    if (!mounted || typeof ResizeObserver === 'undefined') return;
    const listEl = tabListRef.current;
    if (!listEl) return;
    const observer = new ResizeObserver(() => syncTabIndicator());
    observer.observe(listEl);
    if (commitTabRef.current) observer.observe(commitTabRef.current);
    if (reviewTabRef.current) observer.observe(reviewTabRef.current);
    return () => observer.disconnect();
  }, [mounted, syncTabIndicator]);

  return (
    <div className="tab-workspace" inert={!!worktree?.dockExpanded}>
      <div className="tab-heading">
        <div ref={tabListRef} className={`tab-list ${tabIndicator ? '' : 'no-indicator'}`.trim()} role="tablist" aria-label={t.workbench.workspaceTabsLabel}>
          {tabIndicator && (
            <span
              className={`tab-active-indicator ${instantMotion ? 'is-instant' : ''}`.trim()}
              aria-hidden="true"
              style={{
                transform: `translateX(${tabIndicator.left}px)`,
                width: `${tabIndicator.width}px`,
              }}
            />
          )}
          <button
            ref={commitTabRef}
            type="button"
            id="commit-tab"
            role="tab"
            aria-selected={tab === 'commit'}
            aria-controls="commit-panel"
            tabIndex={tab === 'commit' ? 0 : -1}
            onKeyDown={(event) => {
              if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
                onSelectTab('review');
                document.getElementById('review-tab')?.focus();
              }
            }}
            onClick={() => onSelectTab('commit')}
          >
            {t.commit.tabLabel}
            {worktree?.requests.plan
              ? <span className="small muted">{t.commit.planningBadge}</span>
              : worktree?.plan
                ? <span className={`small ${planOutdated ? 'warning' : 'muted'}`}>{planOutdated ? t.commit.planOutdatedBadge : t.commit.planCountBadge(worktree.plan.groups.length)}</span>
                : null}
          </button>
          <button
            ref={reviewTabRef}
            type="button"
            id="review-tab"
            role="tab"
            aria-selected={tab === 'review'}
            aria-controls="review-panel"
            tabIndex={tab === 'review' ? 0 : -1}
            onKeyDown={(event) => {
              if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
                onSelectTab('commit');
                document.getElementById('commit-tab')?.focus();
              }
            }}
            onClick={() => onSelectTab('review')}
          >
            {t.review.tabLabel}
            {reviewLabel && <span className={`small ${highCount ? 'danger' : 'muted'}`}>{reviewLabel}</span>}
          </button>
        </div>
        <div className="row">
          {tab === 'review' && worktree?.review.status === 'done' && (
            <IconButton icon="refresh" label={t.review.reReviewLabel} disabled={!canUseModel} onClick={() => onRunReview()} />
          )}
          <button
            type="button"
            className={`prompt-toggle ${showPromptBox ? 'is-open' : ''} ${promptText ? 'has-text' : ''}`}
            aria-expanded={showPromptBox}
            onClick={() => {
              if (showPromptBox && !promptText) {
                setPromptOpen(false);
              } else {
                promptFocusRequestedRef.current = true;
                setPromptOpen(true);
                textareaRef.current?.focus();
              }
            }}
          >
            {promptText ? (
              <>
                <span className="prompt-dot" aria-hidden="true" />
                {t.commit.promptActiveToggle}
              </>
            ) : (
              t.commit.promptToggle
            )}
          </button>
        </div>
      </div>
      <Collapse key={`prompt:${viewKey}`} open={showPromptBox}>
        <div className="prompt-box">
          <textarea
            ref={textareaRef}
            className="prompt-textarea"
            aria-label={t.commit.promptLabel}
            rows={2}
            value={promptText}
            onChange={(event) => dispatch({ type: 'prompt', worktreeId: worktree?.worktreeId ?? null, text: event.target.value })}
            placeholder={t.commit.promptPlaceholder}
            spellCheck={false}
          />
          <IconButton
            icon="close"
            label={promptText ? t.commit.clearPromptLabel : t.commit.hidePromptLabel}
            onClick={() => {
              if (promptText) {
                dispatch({ type: 'clearPrompt', worktreeId: worktree?.worktreeId ?? null });
              }
              setPromptOpen(false);
            }}
          />
        </div>
      </Collapse>
      <div
        role="tabpanel"
        id={tab === 'commit' ? 'commit-panel' : 'review-panel'}
        aria-labelledby={tab === 'commit' ? 'commit-tab' : 'review-tab'}
        className="tab-panel"
      >
        {tab === 'commit' ? (
          <CommitPanel
            files={worktree?.files ?? []}
            plan={worktree?.plan}
            planOutdated={planOutdated}
            activeAction={activeCustomAction}
            hasWorktree={!!worktree}
            hasModel={hasModel}
            canPlan={canPlan}
            planning={!!worktree?.requests.plan}
            panelRef={commitPanelRef}
            onPlan={onRunPlan}
            onCancelPlan={onCancelPlan}
            onEditPlanGroup={(groupId, message) => { if (worktree) dispatch({ type: 'planGroupEdited', worktreeId: worktree.worktreeId, groupId, message }); }}
            onClearPlan={() => { if (worktree) dispatch({ type: 'planCleared', worktreeId: worktree.worktreeId }); }}
            onConfigureModel={onConfigureModel}
            onEditAction={onEditAction}
            onSwitchBackCPush={() => { if (worktree) dispatch({ type: 'selectActiveAction', repositoryId: worktree.repositoryId, actionId: null }); }}
          />
        ) : (
          <ReviewPanel
            worktree={worktree}
            hasModel={hasModel}
            canReview={canUseModel}
            onReview={(changedOnly) => onRunReview(changedOnly)}
            onCancel={onCancelReview}
            onAddModel={onConfigureModel}
            onOpenModelSettings={onOpenModelSettings}
            onOpenFile={onOpenFile}
          />
        )}
      </div>
    </div>
  );
}
