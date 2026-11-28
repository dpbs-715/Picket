import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { decodeActionSessionSnapshot } from './custom-action-protocol';
import { decodeGitReadReply } from './git-read-protocol';
import type { TrayStateSnapshot } from './tray-protocol';
import {
  decodeWorkbenchRestoreReply,
  WORKBENCH_PERSISTENCE_PROTOCOL_VERSION,
  type WorkbenchSaveRequest,
} from './workbench-persistence-protocol';

function readFixture(name: string): unknown {
  return JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8'));
}

describe('Cross-language TS <-> Rust protocol golden contracts', () => {
  it('decodes golden C-Push session fixture including live pendingCallId, card, questionnaire, and links', () => {
    const raw = readFixture('cpush-session.json');
    const decoded = decodeActionSessionSnapshot(raw);
    expect(decoded.runId).toBe('run-contract-cpush-1');
    expect(decoded.profile).toBe('cpush');
    expect(decoded.status).toBe('needsDecision');
    expect(decoded.pendingCallId).toBe('tc-1');
    expect(decoded.message?.card?.tone).toBe('info');
    expect(decoded.message?.card?.blocks).toEqual([
      { type: 'markdown', content: 'Code and release notes are both selected.' },
    ]);
    expect(decoded.message?.questionnaire?.questions).toHaveLength(1);
    expect(decoded.message?.links).toEqual([
      { label: 'PR #101', url: 'https://github.com/acme/orders/pull/101' },
    ]);
  });

  it('decodes golden Custom Action session fixture with commits and links', () => {
    const raw = readFixture('action-session.json');
    const decoded = decodeActionSessionSnapshot(raw);
    expect(decoded.runId).toBe('run-contract-action-1');
    expect(decoded.profile).toBe('customAction');
    expect(decoded.status).toBe('done');
    expect(decoded.links).toHaveLength(2);
    expect(decoded.commits).toHaveLength(1);
  });

  it('decodes golden repository read reply fixture and composes valid WorkbenchRestoreReply', () => {
    const readRaw = readFixture('repository-snapshot.json');
    const readDecoded = decodeGitReadReply(readRaw);
    expect(readDecoded.snapshot?.repository.id).toBe('repo-orders');
    expect(readDecoded.snapshot?.worktrees[0].files[0].range.kind).toBe('partial');

    const saveRaw = readFixture('workbench-save.json') as WorkbenchSaveRequest;
    const cpushRaw = readFixture('cpush-session.json');
    const actionRaw = readFixture('action-session.json');
    const restored = decodeWorkbenchRestoreReply({
      protocolVersion: WORKBENCH_PERSISTENCE_PROTOCOL_VERSION,
      locale: saveRaw.locale,
      theme: saveRaw.theme,
      editor: saveRaw.editor,
      defaultCommitLanguage: saveRaw.defaultCommitLanguage,
      repositorySettings: saveRaw.repositorySettings,
      windowPreferences: saveRaw.windowPreferences,
      installedEditors: ['idea', 'rustrover'],
      activeWorktreeId: saveRaw.activeWorktreeId,
      emptyPrompt: saveRaw.emptyPrompt,
      worktrees: saveRaw.worktrees,
      snapshots: [readDecoded.snapshot],
      actionSessions: [cpushRaw, actionRaw],
      warning: null,
    });
    expect(restored.locale).toBe('zh');
    expect(restored.actionSessions).toHaveLength(2);
    expect(restored.snapshots).toHaveLength(1);
  });

  it('validates golden TrayStateSnapshot fixture shape', () => {
    const tray = readFixture('tray-state.json') as TrayStateSnapshot;
    expect(tray.protocolVersion).toBe(1);
    expect(tray.hasUpdate).toBe(true);
    expect(tray.repositories[0]).toMatchObject({
      repositoryId: 'repo-orders',
      sessionStatus: 'needsDecision',
      localKeepCount: 2,
    });
  });
});
