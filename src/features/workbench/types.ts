import type { Dispatch } from 'react';
import type { CustomActionConfig } from '../../../shared/custom-action-protocol';
import type { WindowPosition, WindowSize } from '../../../shared/workbench-persistence-protocol';
import type { ModelConfiguration, ProviderConnection } from '../settings/modelProviders';
import type { CompactExpandTarget, SettingsReturnTarget, workbenchReducer } from './state/workbench';

export type WorkbenchDispatch = Dispatch<Parameters<typeof workbenchReducer>[1]>;

export type NoticeTone = 'info' | 'success' | 'warning' | 'error';

export interface NoticeItem {
  text: string;
  tone?: NoticeTone;
  showModelSettings?: boolean;
  seq?: number;
}

export type NotifyOptions = boolean | { showModelSettings?: boolean; tone?: NoticeTone };

export type NotifyFn = (text: string, key?: string, options?: NotifyOptions) => void;

export type PendingModeFocus = { kind: CompactExpandTarget | 'addModel' };

export interface CompactReturnSnapshot {
  position: WindowPosition;
  size: WindowSize;
}

export interface SettingsSession {
  returnTo: SettingsReturnTarget;
  focusModels: boolean;
  /** Captured immediately before expanding from compact mode so closing Settings restores exact compact size & position. */
  compactSnapshot?: CompactReturnSnapshot;
}

export type ModelOverlayState =
  | { kind: 'closed' }
  | { kind: 'createProvider'; keepActiveModel?: boolean }
  | { kind: 'editProvider'; connection: ProviderConnection }
  | { kind: 'addModel'; connection: ProviderConnection }
  | { kind: 'editModel'; connection: ProviderConnection; model: ModelConfiguration };

export type ActionOverlayItem =
  | { kind: 'manage' }
  | { kind: 'create'; mode: 'manual' | 'create' }
  | { kind: 'edit'; action: CustomActionConfig };
