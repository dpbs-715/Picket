import type {
  AppError as DesktopErrorPayload,
  AppErrorCode as DesktopErrorCode,
  ErrorCategory as DesktopErrorCategory,
} from './generated';

export type { DesktopErrorCategory, DesktopErrorCode, DesktopErrorPayload };

const VALID_CATEGORIES = new Set<string>([
  'model',
  'repository',
  'git',
  'action',
  'auth',
  'storage',
  'updater',
  'skills',
  'window',
  'internal',
]);

export function isDesktopErrorPayload(value: unknown): value is DesktopErrorPayload {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const obj = value as Record<string, unknown>;
  return (
    typeof obj.code === 'string'
    && obj.code.length > 0
    && typeof obj.category === 'string'
    && VALID_CATEGORIES.has(obj.category)
    && typeof obj.message === 'string'
    && (obj.retryable === undefined || typeof obj.retryable === 'boolean')
  );
}

export function parseDesktopErrorPayload(raw: unknown): DesktopErrorPayload | null {
  if (isDesktopErrorPayload(raw)) {
    return {
      code: raw.code,
      category: raw.category,
      message: raw.message,
      ...(raw.retryable !== undefined ? { retryable: raw.retryable } : {}),
    };
  }
  if (typeof raw === 'string' && raw.startsWith('{') && raw.endsWith('}')) {
    try {
      const parsed: unknown = JSON.parse(raw);
      if (isDesktopErrorPayload(parsed)) {
        return {
          code: parsed.code,
          category: parsed.category,
          message: parsed.message,
          ...(parsed.retryable !== undefined ? { retryable: parsed.retryable } : {}),
        };
      }
    } catch {
      return null;
    }
  }
  return null;
}
