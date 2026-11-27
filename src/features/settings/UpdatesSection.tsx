import type { UpdateStateSnapshot } from '../../../shared/update-protocol';
import { useI18n } from '../../i18n';
import { Icon } from '../../ui/controls';
import { MOTION_DURATION, MOTION_EASE, m, useInstantMotion } from '../../ui/motion';

function formatMegabytes(bytes: number): string {
  return (bytes / (1024 * 1024)).toFixed(1);
}

interface UpdatesSectionProps {
  updateState: UpdateStateSnapshot;
  gitBusyForUpdate?: boolean;
  onCheckUpdate?(): void;
  onInstallUpdate?(version: string): void;
  onCancelUpdate?(): void;
  onOpenReleasePage?(): void;
}

export function UpdatesSection({
  updateState,
  gitBusyForUpdate,
  onCheckUpdate,
  onInstallUpdate,
  onCancelUpdate,
  onOpenReleasePage,
}: UpdatesSectionProps) {
  const { t } = useI18n();
  const instantMotion = useInstantMotion();

  return (
    <div id="settings-updates-section" className="settings-updates-block" data-update-phase={updateState.phase}>
      <div className="settings-row-item settings-update-row">
        <div className="row min-width-zero">
          <span className="settings-row-label">{t.settings.updates.heading}</span>
          <span className="model-count mono">{t.settings.updates.currentVersion(updateState.currentVersion)}</span>
          {updateState.phase === 'upToDate' && (
            <span className="settings-uptodate-inline" role="status">
              <Icon name="check" className="success" />
              <span>{t.settings.updates.upToDate}</span>
            </span>
          )}
        </div>
        {onCheckUpdate && (
          <button
            type="button"
            className="secondary compact"
            disabled={
              updateState.phase === 'checking'
              || updateState.phase === 'downloading'
              || updateState.phase === 'installing'
            }
            onClick={onCheckUpdate}
          >
            {updateState.phase === 'checking' && <Icon name="refresh" className="spinning" />}
            {updateState.phase === 'checking' ? t.settings.updates.checking : t.settings.updates.checkButton}
          </button>
        )}
      </div>

      {updateState.phase !== 'idle'
        && updateState.phase !== 'checking'
        && updateState.phase !== 'upToDate' && (
        <m.div
          key={`update-card:${updateState.phase}:${updateState.availableVersion ?? ''}`}
          initial={instantMotion ? false : { opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: MOTION_DURATION.fast, ease: MOTION_EASE }}
        >
          {updateState.phase === 'available' && updateState.availableVersion && (
            <div className="update-card" role="region" aria-label={t.settings.updates.availableTitle(updateState.availableVersion)}>
              <div className="row update-card-title">
                <strong>{t.settings.updates.availableTitle(updateState.availableVersion)}</strong>
              </div>
              {updateState.releaseNotes && (
                <p className="muted update-notes">{updateState.releaseNotes}</p>
              )}
              {updateState.installBlocker !== 'none' && (
                <p className="help muted">
                  {t.settings.updates.blockers[updateState.installBlocker]}
                </p>
              )}
              {updateState.installBlocker === 'none' && gitBusyForUpdate && (
                <p className="help warning">{t.settings.updates.gitBusyNote}</p>
              )}
              {updateState.error && (
                <p className="help danger" role="alert">{updateState.error}</p>
              )}
              <div className="row update-actions">
                {updateState.canInstall && onInstallUpdate && (
                  <button
                    type="button"
                    className="primary compact"
                    disabled={gitBusyForUpdate}
                    onClick={() => onInstallUpdate?.(updateState.availableVersion!)}
                  >
                    {t.settings.updates.updateAndRestart}
                  </button>
                )}
                {onOpenReleasePage && (
                  <button type="button" className="secondary compact" onClick={onOpenReleasePage}>
                    {t.settings.updates.downloadFromGithub}
                  </button>
                )}
              </div>
            </div>
          )}

          {updateState.phase === 'downloading' && (
            <div className="update-card" role="status">
              {(() => {
                const downloaded = updateState.downloadedBytes ?? 0;
                const total = updateState.totalBytes;
                const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
                return (
                  <>
                    <div className="row update-progress-row">
                      <span className="small">
                        {percent !== null
                          ? t.settings.updates.downloading(percent, formatMegabytes(downloaded), formatMegabytes(total!))
                          : t.settings.updates.downloadingUnknown(formatMegabytes(downloaded))}
                      </span>
                      {onCancelUpdate && (
                        <button type="button" className="compact" onClick={onCancelUpdate}>
                          {t.common.cancel}
                        </button>
                      )}
                    </div>
                    <div className="update-progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? undefined}>
                      <div className="update-progress-fill" style={{ width: `${percent ?? 15}%` }} />
                    </div>
                  </>
                );
              })()}
            </div>
          )}

          {updateState.phase === 'installing' && (
            <div className="update-card update-card-installing" role="status">
              <Icon name="refresh" className="spinning" />
              <span>{t.settings.updates.installing}</span>
            </div>
          )}

          {updateState.phase === 'error' && (
            <div className="update-card update-card-error" role="alert">
              {updateState.error && <p className="help danger">{updateState.error}</p>}
              <div className="row update-actions">
                {onCheckUpdate && (
                  <button type="button" className="secondary compact" onClick={onCheckUpdate}>
                    {t.settings.updates.retry}
                  </button>
                )}
                {onOpenReleasePage && (
                  <button type="button" className="secondary compact" onClick={onOpenReleasePage}>
                    {t.settings.updates.downloadFromGithub}
                  </button>
                )}
              </div>
            </div>
          )}
        </m.div>
      )}
    </div>
  );
}
