import { useI18n } from '../../../i18n';
import { Icon, IconButton } from '../../../ui/controls';
import { PresenceNotice } from '../../../ui/motion';
import type { NoticeItem } from '../types';

interface Props {
  viewKey: string;
  notice: NoticeItem | undefined;
  onPauseChange: (paused: boolean) => void;
  onOpenModelSettings: () => void;
  onDismiss: () => void;
}

export function OperationNoticeBar({
  viewKey,
  notice,
  onPauseChange,
  onOpenModelSettings,
  onDismiss,
}: Props) {
  const { t } = useI18n();

  return (
    <div className="operation-notice-layer">
      <PresenceNotice show={Boolean(notice?.text)} motionKey={`notice:${viewKey}:${notice?.seq ?? 0}:${notice?.text ?? ''}`}>
        {notice?.text && (() => {
          const tone = notice.tone ?? 'info';
          const noticeIcon = tone === 'success' ? 'check' : tone === 'info' ? 'help' : 'alert';
          return (
            <div
              className={`operation-notice operation-notice-${tone}`}
              data-notice-tone={tone}
              role={tone === 'error' || tone === 'warning' ? 'alert' : 'status'}
              onMouseEnter={() => onPauseChange(true)}
              onMouseLeave={() => onPauseChange(false)}
            >
              <span className="operation-notice-body">
                <Icon name={noticeIcon} className="operation-notice-icon" />
                <span>{notice.text}</span>
              </span>
              <div className="operation-notice-actions">
                {notice.showModelSettings && (
                  <button type="button" className="secondary operation-notice-action" onClick={onOpenModelSettings}>
                    {t.workbench.openModelSettingsAction}
                  </button>
                )}
                <IconButton icon="close" label={t.common.dismissNotice} onClick={onDismiss} />
              </div>
            </div>
          );
        })()}
      </PresenceNotice>
    </div>
  );
}
