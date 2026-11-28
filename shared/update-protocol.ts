import type {
  CancelUpdateRequest,
  CheckUpdateRequest,
  InstallUpdateRequest,
  OpenReleasePageRequest,
  UpdateInstallBlocker,
  UpdatePhase,
  UpdateStateSnapshot,
} from './generated';

export const UPDATE_PROTOCOL_VERSION = 1 as const;
export const PICKET_RELEASES_URL = 'https://github.com/dpbs-715/Picket/releases';

export type {
  CancelUpdateRequest,
  CheckUpdateRequest,
  InstallUpdateRequest,
  OpenReleasePageRequest,
  UpdateInstallBlocker,
  UpdatePhase,
  UpdateStateSnapshot,
};

const VALID_PHASES = new Set<UpdatePhase>([
  'idle',
  'checking',
  'upToDate',
  'available',
  'downloading',
  'installing',
  'error',
]);

const VALID_BLOCKERS = new Set<UpdateInstallBlocker>([
  'none',
  'devBuild',
  'readOnlyVolume',
  'appTranslocation',
  'signingKeyUnconfigured',
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function optionalString(value: unknown, field: string): string | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'string') throw new Error(`Invalid update snapshot field: ${field}`);
  return value;
}

function optionalNonNegativeInt(value: unknown, field: string): number | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
    throw new Error(`Invalid update snapshot numeric field: ${field}`);
  }
  return Math.floor(value);
}

export function decodeUpdateStateSnapshot(value: unknown): UpdateStateSnapshot {
  if (!isRecord(value)) {
    throw new Error('Update state snapshot must be an object');
  }
  if (value.protocolVersion !== UPDATE_PROTOCOL_VERSION) {
    throw new Error(`Unsupported update protocol version: ${String(value.protocolVersion)}`);
  }
  if (typeof value.currentVersion !== 'string' || !value.currentVersion.trim()) {
    throw new Error('Update snapshot missing currentVersion');
  }
  if (typeof value.phase !== 'string' || !VALID_PHASES.has(value.phase as UpdatePhase)) {
    throw new Error(`Invalid update phase: ${String(value.phase)}`);
  }
  if (typeof value.releasePageUrl !== 'string' || !value.releasePageUrl.startsWith('https://')) {
    throw new Error('Update snapshot missing valid https releasePageUrl');
  }
  if (
    typeof value.installBlocker !== 'string'
    || !VALID_BLOCKERS.has(value.installBlocker as UpdateInstallBlocker)
  ) {
    throw new Error(`Invalid update installBlocker: ${String(value.installBlocker)}`);
  }
  if (typeof value.canInstall !== 'boolean') {
    throw new Error('Update snapshot missing boolean canInstall');
  }

  const availableVersion = optionalString(value.availableVersion, 'availableVersion');
  const releaseNotes = optionalString(value.releaseNotes, 'releaseNotes');
  const publishedAt = optionalString(value.publishedAt, 'publishedAt');
  const downloadedBytes = optionalNonNegativeInt(value.downloadedBytes, 'downloadedBytes');
  const totalBytes = optionalNonNegativeInt(value.totalBytes, 'totalBytes');
  const error = optionalString(value.error, 'error');
  const checkedAt = optionalString(value.checkedAt, 'checkedAt');

  return {
    protocolVersion: UPDATE_PROTOCOL_VERSION,
    currentVersion: value.currentVersion,
    phase: value.phase as UpdatePhase,
    releasePageUrl: value.releasePageUrl,
    ...(availableVersion !== undefined ? { availableVersion } : {}),
    ...(releaseNotes !== undefined ? { releaseNotes } : {}),
    ...(publishedAt !== undefined ? { publishedAt } : {}),
    ...(downloadedBytes !== undefined ? { downloadedBytes } : {}),
    ...(totalBytes !== undefined ? { totalBytes } : {}),
    installBlocker: value.installBlocker as UpdateInstallBlocker,
    canInstall: value.canInstall,
    ...(error !== undefined ? { error } : {}),
    ...(checkedAt !== undefined ? { checkedAt } : {}),
  };
}
