import { Fragment, useState, type ReactNode } from 'react';
import type {
  MessageBlock,
  MessageChecklistItem,
  MessageFactItem,
  MessageFindingItem,
} from '../../../../shared/message-user-protocol';
import { useI18n } from '../../../i18n';
import { Icon } from '../../../ui/controls';
import { splitPath } from '../state/workbench';

interface Props {
  blocks: MessageBlock[];
  editorShort: string;
  onOpenRepoPath?: (path: string, line?: number) => void;
}

const SAFE_URL_RE = /^https?:\/\/[^\s<>()]+$/i;
const INLINE_TOKEN_RE = /(`[^`\n]+`|\*\*[^*\n]+\*\*|\[[^\]\n]+\]\(https?:\/\/[^\s)]+\)|https?:\/\/[^\s<>()]+)/g;

function renderInlineMarkdown(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let lastIndex = 0;
  let matchIndex = 0;

  for (const match of text.matchAll(INLINE_TOKEN_RE)) {
    const token = match[0];
    const start = match.index ?? 0;
    if (start > lastIndex) {
      nodes.push(text.slice(lastIndex, start));
    }
    const key = `${keyPrefix}:${matchIndex++}`;
    if (token.startsWith('`') && token.endsWith('`') && token.length >= 2) {
      nodes.push(
        <code key={key} className="d2-md-code mono">
          {token.slice(1, -1)}
        </code>,
      );
    } else if (token.startsWith('**') && token.endsWith('**') && token.length >= 4) {
      nodes.push(<strong key={key}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith('[')) {
      const linkMatch = /^\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)$/.exec(token);
      if (linkMatch && SAFE_URL_RE.test(linkMatch[2])) {
        nodes.push(
          <a
            key={key}
            className="d2-md-link"
            href={linkMatch[2]}
            target="_blank"
            rel="noreferrer"
          >
            {linkMatch[1]}
          </a>,
        );
      } else {
        nodes.push(token);
      }
    } else if (SAFE_URL_RE.test(token)) {
      const trimmed = token.replace(/[.,;:!?)]+$/, '');
      const trailing = token.slice(trimmed.length);
      nodes.push(
        <Fragment key={key}>
          <a className="d2-md-link" href={trimmed} target="_blank" rel="noreferrer">
            {trimmed}
          </a>
          {trailing}
        </Fragment>,
      );
    } else {
      nodes.push(token);
    }
    lastIndex = start + token.length;
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }
  return nodes;
}

function SafeMarkdown({ content }: { content: string }) {
  const normalized = content.replace(/\r\n?/g, '\n');
  const lines = normalized.split('\n');
  const elements: ReactNode[] = [];
  let i = 0;
  let keyCounter = 0;

  while (i < lines.length) {
    const rawLine = lines[i];
    const trimmed = rawLine.trim();

    if (!trimmed) {
      i += 1;
      continue;
    }

    if (trimmed.startsWith('```')) {
      const codeLines: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        codeLines.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1;
      elements.push(
        <pre key={`code-${keyCounter++}`} className="d2-pre d2-md-pre mono scroll-area">
          <code>{codeLines.join('\n')}</code>
        </pre>,
      );
      continue;
    }

    const headingMatch = /^(#{1,3})\s+(.+)$/.exec(trimmed);
    if (headingMatch) {
      elements.push(
        <div key={`h-${keyCounter++}`} className="d2-md-h">
          {renderInlineMarkdown(headingMatch[2], `h-${keyCounter}`)}
        </div>,
      );
      i += 1;
      continue;
    }

    if (/^[-*]\s+/.test(trimmed)) {
      const items: string[] = [];
      while (i < lines.length && /^[-*]\s+/.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(/^[-*]\s+/, ''));
        i += 1;
      }
      elements.push(
        <ul key={`ul-${keyCounter++}`} className="d2-md-list">
          {items.map((item, idx) => (
            <li key={idx}>{renderInlineMarkdown(item, `ul-${keyCounter}-${idx}`)}</li>
          ))}
        </ul>,
      );
      continue;
    }

    if (/^\d+\.\s+/.test(trimmed)) {
      const items: string[] = [];
      while (i < lines.length && /^\d+\.\s+/.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(/^\d+\.\s+/, ''));
        i += 1;
      }
      elements.push(
        <ol key={`ol-${keyCounter++}`} className="d2-md-list is-ordered">
          {items.map((item, idx) => (
            <li key={idx}>{renderInlineMarkdown(item, `ol-${keyCounter}-${idx}`)}</li>
          ))}
        </ol>,
      );
      continue;
    }

    const paraLines: string[] = [trimmed];
    i += 1;
    while (
      i < lines.length
      && lines[i].trim() !== ''
      && !lines[i].trim().startsWith('```')
      && !/^(#{1,3})\s+/.test(lines[i].trim())
      && !/^[-*]\s+/.test(lines[i].trim())
      && !/^\d+\.\s+/.test(lines[i].trim())
    ) {
      paraLines.push(lines[i].trim());
      i += 1;
    }
    elements.push(
      <p key={`p-${keyCounter++}`} className="d2-md-p">
        {renderInlineMarkdown(paraLines.join(' '), `p-${keyCounter}`)}
      </p>,
    );
  }

  return <div className="d2-md">{elements}</div>;
}

function factToneClass(tone?: MessageFactItem['tone']): string {
  switch (tone) {
    case 'success':
      return 'is-success';
    case 'warning':
      return 'is-warning';
    case 'danger':
      return 'is-danger';
    default:
      return '';
  }
}

function findingSeverityClass(severity: MessageFindingItem['severity']): string {
  switch (severity) {
    case 'high':
      return 'sev-high';
    case 'medium':
      return 'sev-medium';
    case 'low':
      return 'sev-low';
    default:
      return 'sev-info';
  }
}

function ChecklistStatusIcon({ status }: { status: MessageChecklistItem['status'] }) {
  switch (status) {
    case 'pass':
      return <span className="d2-check-icon is-pass"><Icon name="checkCircle" /></span>;
    case 'fail':
      return <span className="d2-check-icon is-fail"><Icon name="xCircle" /></span>;
    case 'warn':
      return <span className="d2-check-icon is-warn"><Icon name="alertCircle" /></span>;
    case 'running':
      return <span className="d2-check-icon is-running"><Icon name="loader" className="spinning" /></span>;
    default:
      return <span className="d2-check-icon is-skip" aria-hidden="true"><span className="d2-skip-dot" /></span>;
  }
}

export function MessageBlocks({ blocks, editorShort, onOpenRepoPath }: Props) {
  const { t } = useI18n();
  const [codeOpenMap, setCodeOpenMap] = useState<Record<number, boolean>>({});
  const [copiedIdx, setCopiedIdx] = useState<number | null>(null);
  const [skippedOpenMap, setSkippedOpenMap] = useState<Record<number, boolean>>({});

  function handleCopyCode(index: number, content: string) {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(content).catch(() => {});
    }
    setCopiedIdx(index);
    window.setTimeout(() => {
      setCopiedIdx((prev) => (prev === index ? null : prev));
    }, 1500);
  }

  return (
    <div className="d2-blocks">
      {blocks.map((block, idx) => {
        if (block.type === 'markdown') {
          return (
            <div key={idx} className="d2-block-markdown">
              <SafeMarkdown content={block.content} />
            </div>
          );
        }

        if (block.type === 'callout') {
          const tone = block.tone ?? 'info';
          const iconName = tone === 'success'
            ? 'checkCircle'
            : tone === 'danger'
              ? 'xCircle'
              : tone === 'warning'
                ? 'alertCircle'
                : 'info';
          return (
            <div key={idx} className={`d2-callout is-${tone}`}>
              <span className="d2-callout-icon" aria-hidden="true">
                <Icon name={iconName} />
              </span>
              <div className="d2-callout-body min-width-zero">
                {block.title && <div className="d2-callout-title">{block.title}</div>}
                <div className="d2-callout-content">{block.content}</div>
              </div>
            </div>
          );
        }

        if (block.type === 'facts') {
          return (
            <div key={idx} className="d2-block-section">
              {block.heading && <div className="d2-block-heading">{block.heading}</div>}
              <div className={`d2-facts-bar ${block.items.length === 1 ? 'is-single' : ''}`}>
                {block.items.map((item, itemIdx) => (
                  <div key={`${item.label}:${itemIdx}`} className="d2-fact-seg">
                    <div className="d2-fact-label truncate">{item.label}</div>
                    <div
                      className={`d2-fact-value truncate ${item.mono ? 'mono' : ''} ${factToneClass(item.tone)}`}
                      title={item.value}
                    >
                      {item.value}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        }

        if (block.type === 'findings') {
          const skipOpen = Boolean(skippedOpenMap[idx]);
          return (
            <div key={idx} className="d2-block-section">
              {block.heading && <div className="d2-block-heading">{block.heading}</div>}
              <div className="d2-group">
                {block.items.length === 0 ? (
                  <div className="d2-group-row d2-findings-empty">
                    <span className="d2-check-icon is-pass"><Icon name="checkCircle" /></span>
                    <span>{block.emptyText || t.dock.emptyFindingsDefault}</span>
                  </div>
                ) : (
                  block.items.map((finding, fIdx) => {
                    const locName = finding.path ? splitPath(finding.path).name : '';
                    const locLabel = locName ? `${locName}${finding.line ? `:${finding.line}` : ''}` : '';
                    const clickable = Boolean(finding.path && onOpenRepoPath);
                    const tagText = finding.tag || t.dock.severityLabels[finding.severity];
                    const inner = (
                      <>
                        <div className="d2-finding-head">
                          <span className={`d2-sev-tag mono ${findingSeverityClass(finding.severity)}`}>
                            {tagText}
                          </span>
                          <span className="d2-finding-title">{finding.title}</span>
                        </div>
                        <div className="d2-finding-detail">{finding.detail}</div>
                        {locLabel && (
                          <div className="d2-row-foot">
                            <span className="mono truncate" title={finding.path}>{locLabel}</span>
                            {clickable && (
                              <span className="d2-jump-cue">
                                {t.dock.openInEditor(editorShort)}
                                <Icon name="arrowUpRight" />
                              </span>
                            )}
                          </div>
                        )}
                      </>
                    );
                    if (clickable) {
                      return (
                        <button
                          key={fIdx}
                          type="button"
                          className="d2-group-row is-clickable"
                          data-finding-path={finding.path}
                          data-finding-line={finding.line}
                          onClick={() => onOpenRepoPath!(finding.path!, finding.line)}
                        >
                          {inner}
                        </button>
                      );
                    }
                    return (
                      <div key={fIdx} className="d2-group-row">
                        {inner}
                      </div>
                    );
                  })
                )}
                {block.skipped && block.skipped.length > 0 && (
                  <div className="d2-skipped-wrap">
                    <button
                      type="button"
                      className="d2-skipped-toggle"
                      aria-expanded={skipOpen}
                      onClick={() => setSkippedOpenMap((prev) => ({ ...prev, [idx]: !prev[idx] }))}
                    >
                      <Icon name={skipOpen ? 'down' : 'right'} />
                      <span>{t.dock.skippedFilesToggle(block.skipped.length)}</span>
                    </button>
                    {skipOpen && (
                      <div className="d2-skipped-list">
                        {block.skipped.map((entry, sIdx) => (
                          <div key={`${entry.path}:${sIdx}`} className="d2-skipped-item">
                            <span className="mono d2-skipped-path">{entry.path}</span>
                            <span> · {entry.reason}</span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          );
        }

        if (block.type === 'checklist') {
          return (
            <div key={idx} className="d2-block-section">
              {block.heading && <div className="d2-block-heading">{block.heading}</div>}
              <div className="d2-group">
                {block.items.map((item, cIdx) => (
                  <div key={`${item.label}:${cIdx}`} className="d2-group-row d2-checklist-row">
                    <ChecklistStatusIcon status={item.status} />
                    <div className="min-width-zero d2-checklist-body">
                      <div className={`d2-checklist-label ${item.status === 'fail' ? 'is-fail' : ''}`}>
                        {item.label}
                      </div>
                      {item.detail && <div className="d2-checklist-detail">{item.detail}</div>}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        }

        if (block.type === 'code') {
          const lineCount = block.content.split('\n').length;
          const defaultOpen = block.collapsed !== undefined ? !block.collapsed : lineCount <= 8;
          const open = codeOpenMap[idx] !== undefined ? codeOpenMap[idx] : defaultOpen;
          const isCopied = copiedIdx === idx;
          return (
            <div key={idx} className="d2-group d2-code-well">
              <div className="d2-code-bar">
                <button
                  type="button"
                  className="d2-code-title-btn min-width-zero"
                  aria-expanded={open}
                  onClick={() => setCodeOpenMap((prev) => ({ ...prev, [idx]: !open }))}
                >
                  <Icon name="terminal" className="d2-code-term-icon" />
                  <span className="truncate">{block.title || t.dock.defaultCodeTitle}</span>
                  <span className="mono d2-code-lines">{t.dock.codeLines(lineCount)}</span>
                </button>
                <div className="d2-code-actions">
                  <button
                    type="button"
                    className={`d2-ibtn ${isCopied ? 'is-copied' : ''}`}
                    title={isCopied ? t.dock.copiedCode : t.dock.copyCode}
                    aria-label={isCopied ? t.dock.copiedCode : t.dock.copyCode}
                    onClick={() => handleCopyCode(idx, block.content)}
                  >
                    <Icon name={isCopied ? 'check' : 'copy'} />
                  </button>
                  <button
                    type="button"
                    className="d2-ibtn"
                    title={open ? t.dock.collapseCode : t.dock.expandCode}
                    aria-label={open ? t.dock.collapseCode : t.dock.expandCode}
                    onClick={() => setCodeOpenMap((prev) => ({ ...prev, [idx]: !open }))}
                  >
                    <Icon name={open ? 'up' : 'down'} />
                  </button>
                </div>
              </div>
              {open && (
                <pre className="d2-pre mono scroll-area">
                  <code>{block.content}</code>
                </pre>
              )}
            </div>
          );
        }

        return (
          <div key={idx} className="d2-block-section">
            {block.heading && <div className="d2-block-heading">{block.heading}</div>}
            <div className="d2-pill-row">
              {block.items.map((item, fIdx) => {
                const { name } = splitPath(item.path);
                const label = `${name}${item.line ? `:${item.line}` : ''}`;
                return (
                  <button
                    key={`${item.path}:${item.line ?? ''}:${fIdx}`}
                    type="button"
                    className="d2-pill"
                    title={item.path}
                    disabled={!onOpenRepoPath}
                    onClick={() => onOpenRepoPath?.(item.path, item.line)}
                  >
                    <Icon name="fileCode" />
                    <span className="mono truncate">{label}</span>
                    {item.badge && <span className="d2-pill-badge">{item.badge}</span>}
                  </button>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}
