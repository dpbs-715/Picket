import { describe, expect, it } from 'vitest';
import {
  CUSTOM_ACTION_GUARD_DEFAULTS,
  createToolLoopGuard,
  stableToolCallSignature,
} from '../src/tasks/tool-guard';

function makeCall(name: string, args: unknown) {
  return {
    toolCall: { type: 'toolCall' as const, id: `call-${name}`, name, arguments: (args as Record<string, unknown>) ?? {} },
    args,
  };
}

describe('Pi Agent tool loop guard', () => {
  it('allows multi-stage custom actions to exceed 80 varied calls and excludes overhead tools from runaway ceiling', async () => {
    const guard = createToolLoopGuard({
      kind: 'adaptive',
      ...CUSTOM_ACTION_GUARD_DEFAULTS,
    })!;

    // 25 skill doc & checkpoint overhead calls + 120 distinct Git/CLI calls should all pass cleanly:
    for (let i = 0; i < 25; i += 1) {
      expect(await guard.beforeToolCall(makeCall('read_skill_doc', { skill: 'submit', path: `doc-${i}.md` }))).toBeUndefined();
      await guard.afterToolCall({ toolCall: makeCall('read_skill_doc', {}).toolCall, isError: false });
      expect(await guard.beforeToolCall(makeCall('record_action_checkpoint', { step: i }))).toBeUndefined();
      await guard.afterToolCall({ toolCall: makeCall('record_action_checkpoint', {}).toolCall, isError: false });
    }
    for (let i = 0; i < 120; i += 1) {
      const ctx = makeCall('run_git', { args: ['log', '-n', String(i + 1)] });
      expect(await guard.beforeToolCall(ctx)).toBeUndefined();
      await guard.afterToolCall({ toolCall: ctx.toolCall, isError: false });
    }
  });

  it('blocks the 4th consecutive identical tool call and immediately recovers when arguments change', async () => {
    const guard = createToolLoopGuard({
      kind: 'adaptive',
      maxIdenticalConsecutive: 3,
      maxConsecutiveErrors: 8,
      runawayCeiling: 300,
    })!;

    const sameA = makeCall('run_platform_cli', { cli: 'glab', args: ['issue', 'list'] });
    const sameB = makeCall('run_platform_cli', { args: ['issue', 'list'], cli: 'glab' });
    expect(stableToolCallSignature('run_platform_cli', sameA.args)).toBe(
      stableToolCallSignature('run_platform_cli', sameB.args),
    );

    expect(await guard.beforeToolCall(sameA)).toBeUndefined();
    expect(await guard.beforeToolCall(sameB)).toBeUndefined();
    expect(await guard.beforeToolCall(sameA)).toBeUndefined();

    const blocked = await guard.beforeToolCall(sameA);
    expect(blocked?.block).toBe(true);
    expect(blocked?.reason).toContain('Loop detected');

    // Pivoting to a different command breaks the streak and is allowed again:
    const pivot = makeCall('run_platform_cli', { cli: 'glab', args: ['issue', 'create'] });
    expect(await guard.beforeToolCall(pivot)).toBeUndefined();
  });

  it('stops thrashing after 8 consecutive failing tool calls while still allowing message_user and resetting on success', async () => {
    const guard = createToolLoopGuard({
      kind: 'adaptive',
      maxIdenticalConsecutive: 3,
      maxConsecutiveErrors: 8,
      runawayCeiling: 300,
    })!;

    for (let i = 0; i < 8; i += 1) {
      const ctx = makeCall('run_platform_cli', { cli: 'glab', args: ['issue', 'create', `--try-${i}`] });
      expect(await guard.beforeToolCall(ctx)).toBeUndefined();
      await guard.afterToolCall({ toolCall: ctx.toolCall, isError: true });
    }

    const blocked = await guard.beforeToolCall(makeCall('run_git', { args: ['status'] }));
    expect(blocked?.block).toBe(true);
    expect(blocked?.reason).toContain('8 consecutive failing');

    // message_user stays unblocked so the agent can ask the user for help; successful message_user resets error streak:
    const askCtx = makeCall('message_user', { title: 'Need help', questions: [] });
    expect(await guard.beforeToolCall(askCtx)).toBeUndefined();
    await guard.afterToolCall({ toolCall: askCtx.toolCall, isError: false });

    expect(await guard.beforeToolCall(makeCall('run_git', { args: ['status'] }))).toBeUndefined();
  });

  it('preserves fixed budgets for bounded tasks like planCPush and generateCustomAction', async () => {
    const fixed = createToolLoopGuard({ kind: 'fixed', maxCalls: 2 })!;
    expect(await fixed.beforeToolCall(makeCall('read_repo_file', { path: 'a.ts' }))).toBeUndefined();
    expect(await fixed.beforeToolCall(makeCall('read_repo_file', { path: 'b.ts' }))).toBeUndefined();
    const blocked = await fixed.beforeToolCall(makeCall('read_repo_file', { path: 'c.ts' }));
    expect(blocked?.block).toBe(true);
    expect(blocked?.reason).toContain('Tool call budget (2 calls)');
  });
});
