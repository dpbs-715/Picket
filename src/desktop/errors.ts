import { invoke } from '@tauri-apps/api/core';
import {
  parseDesktopErrorPayload,
  type DesktopErrorCategory,
  type DesktopErrorCode,
  type DesktopErrorPayload,
} from '../../shared/error-protocol';

export class DesktopCommandError extends Error {
  readonly code: DesktopErrorCode;
  readonly category: DesktopErrorCategory;
  readonly retryable?: boolean;

  constructor(payload: DesktopErrorPayload) {
    super(payload.message);
    this.name = 'DesktopCommandError';
    this.code = payload.code;
    this.category = payload.category;
    if (payload.retryable !== undefined) {
      this.retryable = payload.retryable;
    }
  }
}

export function isDesktopCommandError(error: unknown): error is DesktopCommandError {
  return error instanceof DesktopCommandError;
}

export function isModelCommandError(error: unknown): boolean {
  if (error instanceof DesktopCommandError) {
    return error.category === 'model';
  }
  const parsed = parseDesktopErrorPayload(error);
  return parsed?.category === 'model';
}

export function toDesktopCommandError(
  raw: unknown,
  fallbackMessage = 'Operation failed.',
): DesktopCommandError {
  if (raw instanceof DesktopCommandError) {
    return raw;
  }
  const parsed = parseDesktopErrorPayload(raw);
  if (parsed) {
    return new DesktopCommandError(parsed);
  }
  if (raw instanceof Error) {
    const nested = parseDesktopErrorPayload(raw.message);
    if (nested) {
      return new DesktopCommandError(nested);
    }
    return new DesktopCommandError({
      code: 'internal',
      category: 'internal',
      message: raw.message || fallbackMessage,
    });
  }
  if (typeof raw === 'string' && raw.trim()) {
    return new DesktopCommandError({
      code: 'internal',
      category: 'internal',
      message: raw,
    });
  }
  return new DesktopCommandError({
    code: 'internal',
    category: 'internal',
    message: fallbackMessage,
  });
}

export async function invokeDesktop<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toDesktopCommandError(error);
  }
}
