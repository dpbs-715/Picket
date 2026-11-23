import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type { PiEvent } from '../../shared/pi-protocol';

export interface ProbeReport {
  requestId: string;
  status: 'completed' | 'cancelled';
  childPid: number;
  nodeVersion: string;
  piCoreVersion: string;
  deltaCount: number;
  childExitCode: number;
}

export interface GitProbeReport {
  gitVersion: string;
  cases: string[];
  commits: string[];
  temporaryRepositoryRemoved: boolean;
}

export const inDesktop = () => isTauri();

export function runPiProbe(requestId: string, onEvent: (event: PiEvent) => void) {
  const channel = new Channel<PiEvent>();
  channel.onmessage = onEvent;
  return invoke<ProbeReport>('run_pi_probe', { requestId, onEvent: channel });
}

export const cancelPiProbe = (requestId: string) => invoke<void>('cancel_pi_probe', { requestId });
export const runGitProbe = () => invoke<GitProbeReport>('run_git_probe');
