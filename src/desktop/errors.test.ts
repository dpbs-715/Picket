import { describe, expect, it } from 'vitest';
import {
  DesktopCommandError,
  isDesktopCommandError,
  isModelCommandError,
  toDesktopCommandError,
} from './errors';

describe('Desktop structured errors', () => {
  it('parses structured Rust AppError JSON objects and exposes model category without string matching', () => {
    const err = toDesktopCommandError({
      code: 'modelAuthFailed',
      category: 'model',
      message: '鉴权失败，请检查配置。',
    });

    expect(err).toBeInstanceOf(Error);
    expect(isDesktopCommandError(err)).toBe(true);
    expect(err.code).toBe('modelAuthFailed');
    expect(err.category).toBe('model');
    expect(err.message).toBe('鉴权失败，请检查配置。');
    expect(isModelCommandError(err)).toBe(true);
  });

  it('parses serialized JSON string errors and preserves retryable flags', () => {
    const raw = JSON.stringify({
      code: 'modelRateLimited',
      category: 'model',
      message: 'Quota exceeded.',
      retryable: true,
    });
    const err = toDesktopCommandError(raw);
    expect(err.code).toBe('modelRateLimited');
    expect(err.retryable).toBe(true);
    expect(isModelCommandError(err)).toBe(true);
  });

  it('does not misclassify repository or git errors as model errors', () => {
    const err = new DesktopCommandError({
      code: 'commitSubmoduleUnsupported',
      category: 'git',
      message: 'Submodule changes are not supported yet.',
      retryable: false,
    });
    expect(isModelCommandError(err)).toBe(false);
    expect(isModelCommandError(new Error('401 inside git commit message'))).toBe(false);
  });

  it('wraps legacy plain strings and native Errors as internal DesktopCommandError', () => {
    const fromString = toDesktopCommandError('Unexpected failure');
    expect(fromString.code).toBe('internal');
    expect(fromString.category).toBe('internal');
    expect(fromString.message).toBe('Unexpected failure');

    const fromErr = toDesktopCommandError(new Error('Disk full'));
    expect(fromErr.message).toBe('Disk full');
    expect(isModelCommandError(fromErr)).toBe(false);
  });
});
