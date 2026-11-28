import type { RefObject } from 'react';
import type {
  CPushQaItem,
  CustomActionLink,
  WorkflowTimelineItem,
} from '../../../../shared/custom-action-protocol';
import { useI18n } from '../../../i18n';
import { Icon } from '../../../ui/controls';
import type { Message } from '../state/workbench';
import { MessageBlocks } from './MessageBlocks';

interface MessageDockBodyProps {
  primary: Message;
  secondary: Message[];
  scrollRef: RefObject<HTMLDivElement | null>;
  editorShort: string;
  hasCardBlocks: boolean;
  isQuestionnaire: boolean;
  currentQuestionText?: string;
  effectiveLinks?: CustomActionLink[];
  timeline: WorkflowTimelineItem[];
  qaLog: CPushQaItem[];
  hasTraceDetails: boolean;
  collapseTraceByDefault: boolean;
  detailsOpen: boolean;
  onToggleDetails(): void;
  onScrollStickChange(atBottom: boolean): void;
  onOpenFile(fileId: string): void;
  onOpenRepoPath(path: string, line?: number): void;
}

function TraceDetails({
  timeline,
  qaLog,
}: {
  timeline: WorkflowTimelineItem[];
  qaLog: CPushQaItem[];
}) {
  const { t } = useI18n();
  return (
    <>
      {timeline.length > 0 && (
        <div className="session-timeline" aria-label={t.dock.timelineLabel}>
          {timeline.map((step, index) => (
            <div className={`timeline-step timeline-${step.state}`} key={`${index}:${step.label}`}>
              <span className="status-dot" />
              <span className="truncate">{step.label}</span>
            </div>
          ))}
        </div>
      )}
      {qaLog.length > 0 && (
        <div className="session-qa-log" aria-label={t.dock.qaLogLabel}>
          {qaLog.map((qa) => (
            <div className="qa-entry small muted" key={qa.questionId}>
              <span className="truncate">{t.dock.qaEntry(qa.question, qa.answer)}</span>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

export function MessageDockBody({
  primary,
  secondary,
  scrollRef,
  editorShort,
  hasCardBlocks,
  isQuestionnaire,
  currentQuestionText,
  effectiveLinks,
  timeline,
  qaLog,
  hasTraceDetails,
  collapseTraceByDefault,
  detailsOpen,
  onToggleDetails,
  onScrollStickChange,
  onOpenFile,
  onOpenRepoPath,
}: MessageDockBodyProps) {
  const { t } = useI18n();

  return (
    <div
      ref={scrollRef}
      className="d2-scroll scroll-area"
      onScroll={(event) => {
        const el = event.currentTarget;
        onScrollStickChange(el.scrollHeight - el.scrollTop - el.clientHeight <= 24);
      }}
    >
      {hasCardBlocks && primary.card?.blocks && (
        <MessageBlocks
          blocks={primary.card.blocks}
          editorShort={editorShort}
          onOpenRepoPath={onOpenRepoPath}
        />
      )}

      {!hasCardBlocks
        && !isQuestionnaire
        && primary.description
        && primary.description.trim() !== (primary.progress?.label ?? '').trim() && (
        <p className="d2-fallback-desc">{primary.description}</p>
      )}

      {!hasCardBlocks && isQuestionnaire && primary.description && primary.description !== currentQuestionText && (
        <p className="d2-fallback-desc">{primary.description}</p>
      )}

      {!!primary.files?.length && (
        <div className="d2-pill-row">
          {primary.files.map((file) => (
            <button
              type="button"
              key={file.id}
              className="d2-pill"
              onClick={() => onOpenFile(file.id)}
            >
              <Icon name="fileCode" />
              <span className="mono truncate">{file.label}</span>
              {file.badge && <span className="d2-pill-badge warning">{file.badge}</span>}
              <Icon name="arrowUpRight" />
            </button>
          ))}
        </div>
      )}

      {!!primary.commits?.length && (
        <div className="d2-commits">
          {primary.commits.map((commit) => (
            <div className="d2-commit-row mono" key={commit.hash}>
              <Icon name="commit" className="accent" />
              <span className="accent d2-commit-hash">{commit.hash}</span>
              <span className="truncate" title={commit.title}>{commit.title}</span>
            </div>
          ))}
        </div>
      )}

      {!!effectiveLinks?.length && (
        <div className="d2-pill-row">
          {effectiveLinks.map((link) => (
            <a
              key={`${link.label}:${link.url}`}
              className="d2-pill d2-link-pill"
              href={link.url}
              target="_blank"
              rel="noreferrer"
            >
              <span className="truncate">{link.label}</span>
              <Icon name="arrowUpRight" />
            </a>
          ))}
        </div>
      )}

      {hasTraceDetails && (
        collapseTraceByDefault ? (
          <div className="d2-trace-disclosure">
            <button
              type="button"
              className="d2-skipped-toggle"
              aria-expanded={detailsOpen}
              onClick={onToggleDetails}
            >
              <Icon name={detailsOpen ? 'down' : 'right'} />
              <span>{t.dock.detailsToggle}</span>
            </button>
            {detailsOpen && <TraceDetails timeline={timeline} qaLog={qaLog} />}
          </div>
        ) : (
          <TraceDetails timeline={timeline} qaLog={qaLog} />
        )
      )}

      {secondary.map((message) => (
        <div className="secondary-message" key={message.id}>
          <Icon name="history" />
          <span className="truncate">{message.title}</span>
        </div>
      ))}
    </div>
  );
}
