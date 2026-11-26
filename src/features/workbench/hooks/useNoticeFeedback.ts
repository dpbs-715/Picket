import { useEffect, useRef, useState } from 'react';
import type { NoticeItem, NoticeTone, NotifyOptions } from '../types';

export function useNoticeFeedback(viewKey: string) {
  const [feedback, setFeedback] = useState<Record<string, NoticeItem>>({});
  const [noticePaused, setNoticePaused] = useState(false);
  const noticeSeqRef = useRef(0);
  const activeNotice = feedback[viewKey];

  function notify(text: string, key = viewKey, options: NotifyOptions = false) {
    const resolved = typeof options === 'boolean'
      ? { showModelSettings: options, tone: (text ? 'error' : 'info') as NoticeTone }
      : { showModelSettings: Boolean(options.showModelSettings), tone: options.tone ?? (text ? 'error' : 'info') };
    noticeSeqRef.current += 1;
    const seq = noticeSeqRef.current;
    setFeedback((previous) => ({
      ...previous,
      [key]: {
        text,
        ...(text ? { tone: resolved.tone, seq } : {}),
        ...(resolved.showModelSettings && text ? { showModelSettings: true } : {}),
      },
    }));
  }

  useEffect(() => {
    if (!activeNotice?.text || noticePaused) return;
    if (activeNotice.tone !== 'success' && activeNotice.tone !== 'info') return;
    const noticeKey = viewKey;
    const noticeSeq = activeNotice.seq;
    const timer = window.setTimeout(() => {
      setFeedback((previous) => {
        const current = previous[noticeKey];
        if (!current || current.seq !== noticeSeq) return previous;
        return { ...previous, [noticeKey]: { text: '' } };
      });
    }, 3600);
    return () => window.clearTimeout(timer);
  }, [activeNotice?.text, activeNotice?.tone, activeNotice?.seq, noticePaused, viewKey]);

  return {
    feedback,
    setFeedback,
    activeNotice,
    noticePaused,
    setNoticePaused,
    notify,
  };
}
