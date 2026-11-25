import { describe, expect, it } from 'vitest';
import {
  decodeUpdateStateSnapshot,
  PICKET_RELEASES_URL,
  UPDATE_PROTOCOL_VERSION,
} from '../../shared/update-protocol';
import { checkForUpdate, getUpdateStatus, installUpdate } from './updater';

describe('updater protocol decoder and browser-preview boundaries', () => {
  it('decodes valid update snapshots across idle, available, downloading, and error phases', () => {
    const decoded = decodeUpdateStateSnapshot({
      protocolVersion: UPDATE_PROTOCOL_VERSION,
      currentVersion: '0.1.0',
      phase: 'downloading',
      releasePageUrl: PICKET_RELEASES_URL,
      availableVersion: '0.1.1',
      releaseNotes: 'Bug fixes and speedups',
      publishedAt: '2026-11-25T02:00:00Z',
      downloadedBytes: 1_048_576,
      totalBytes: 48_000_000,
      installBlocker: 'none',
      canInstall: false,
      checkedAt: '2026-11-25T02:10:00Z',
    });
    expect(decoded.availableVersion).toBe('0.1.1');
    expect(decoded.downloadedBytes).toBe(1_048_576);
    expect(decoded.totalBytes).toBe(48_000_000);
    expect(decoded.installBlocker).toBe('none');
  });

  it('rejects insecure release URLs, unknown phases, unknown blockers, or wrong protocol versions', () => {
    expect(() => decodeUpdateStateSnapshot({
      protocolVersion: 99,
      currentVersion: '0.1.0',
      phase: 'idle',
      releasePageUrl: PICKET_RELEASES_URL,
      installBlocker: 'none',
      canInstall: false,
    })).toThrow(/protocol version/i);

    expect(() => decodeUpdateStateSnapshot({
      protocolVersion: UPDATE_PROTOCOL_VERSION,
      currentVersion: '0.1.0',
      phase: 'idle',
      releasePageUrl: 'javascript:alert(1)',
      installBlocker: 'none',
      canInstall: false,
    })).toThrow(/https/i);

    expect(() => decodeUpdateStateSnapshot({
      protocolVersion: UPDATE_PROTOCOL_VERSION,
      currentVersion: '0.1.0',
      phase: 'hacked',
      releasePageUrl: PICKET_RELEASES_URL,
      installBlocker: 'none',
      canInstall: false,
    })).toThrow(/phase/i);

    expect(() => decodeUpdateStateSnapshot({
      protocolVersion: UPDATE_PROTOCOL_VERSION,
      currentVersion: '0.1.0',
      phase: 'available',
      releasePageUrl: PICKET_RELEASES_URL,
      installBlocker: 'unknownBlocker',
      canInstall: false,
    })).toThrow(/installBlocker/i);
  });

  it('reports honest unsupported errors instead of faking updates in browser preview', async () => {
    const status = await getUpdateStatus();
    expect(status.phase).toBe('idle');
    expect(status.canInstall).toBe(false);
    await expect(checkForUpdate(true, 'en')).rejects.toThrow(/browser preview/i);
    await expect(installUpdate('0.1.1', 'zh')).rejects.toThrow(/浏览器预览/);
  });
});
