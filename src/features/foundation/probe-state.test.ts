import { describe, expect, it } from 'vitest';
import type { ProbeEvent } from '../../../shared/pi-protocol';
import { initialProbeState, probeReducer } from './probe-state';

const context = (id: string) => ({ repositoryId: 'foundation', worktreeId: 'temporary', sessionId: id, scopeVersion: 'foundation-v1' });
const event = (id: string, delta: string): ProbeEvent => ({ type: 'textDelta', requestId: id, context: context(id), delta });

describe('probe projection', () => {
  it('ignores late events and results from an earlier request', () => {
    const state = probeReducer(initialProbeState, { type: 'start', requestId: 'new' });
    expect(probeReducer(state, { type: 'event', requestId: 'old', event: event('old', 'stale') })).toBe(state);
    expect(probeReducer(state, { type: 'failed', requestId: 'old', error: 'stale failure' })).toBe(state);
  });
  it('rejects a different worktree or range even with the same request ID', () => {
    const state = probeReducer(initialProbeState, { type: 'start', requestId: 'current' });
    const wrong = { ...event('current', 'wrong'), context: { ...context('current'), worktreeId: 'another-worktree' } };
    expect(probeReducer(state, { type: 'event', requestId: 'current', event: wrong })).toBe(state);
  });
  it('waits for the native child exit before presenting completion', () => {
    let state = probeReducer(initialProbeState, { type: 'start', requestId: 'current' });
    state = probeReducer(state, { type: 'event', requestId: 'current', event: event('current', 'stream') });
    state = probeReducer(state, { type: 'event', requestId: 'current', event: { type: 'completed', requestId: 'current', context: context('current'), text: 'final' } });
    expect(state.status).toBe('running');
    state = probeReducer(state, { type: 'settled', report: { requestId: 'current', status: 'completed', childPid: 123,
      nodeVersion: '24.17.0', piCoreVersion: '1.6.0', deltaCount: 1, childExitCode: 0 } });
    expect(state.status).toBe('completed');
    expect(state.text).toBe('final');
    expect(probeReducer(state, { type: 'event', requestId: 'current', event: event('current', 'late') })).toBe(state);
  });
});
