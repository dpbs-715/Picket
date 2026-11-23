import type { PiEvent } from '../../../shared/pi-protocol';
import type { ProbeReport } from '../../desktop/foundation';

export interface ProbeState {
  status: 'idle' | 'running' | 'completed' | 'cancelled' | 'failed';
  requestId?: string;
  text: string;
  report?: ProbeReport;
  error?: string;
}
export const initialProbeState: ProbeState = { status: 'idle', text: '' };
export type ProbeAction = { type: 'start'; requestId: string }
  | { type: 'event'; requestId: string; event: PiEvent }
  | { type: 'settled'; report: ProbeReport }
  | { type: 'failed'; requestId: string; error: string };

export function probeReducer(state: ProbeState, action: ProbeAction): ProbeState {
  if (action.type === 'start') return { status: 'running', requestId: action.requestId, text: '' };
  const requestId = action.type === 'settled' ? action.report.requestId : action.requestId;
  if (state.status !== 'running' || requestId !== state.requestId) return state;
  if (action.type === 'failed') return { ...state, status: 'failed', error: action.error };
  if (action.type === 'settled') return { ...state, status: action.report.status, report: action.report };
  const event = action.event;
  if (!('requestId' in event) || event.requestId !== state.requestId || event.context.sessionId !== state.requestId
    || event.context.repositoryId !== 'foundation' || event.context.worktreeId !== 'temporary'
    || event.context.scopeVersion !== 'foundation-v1') return state;
  if (event.type === 'textDelta') return { ...state, text: state.text + event.delta };
  if (event.type === 'completed') return { ...state, text: event.text };
  return state;
}
