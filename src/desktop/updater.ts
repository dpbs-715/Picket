import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { version as PACKAGE_VERSION } from '../../package.json';
import type { AppLocale } from '../../shared/language-protocol';
import {
  decodeUpdateStateSnapshot,
  PICKET_RELEASES_URL,
  UPDATE_PROTOCOL_VERSION,
  type UpdateStateSnapshot,
} from '../../shared/update-protocol';
import { getMessages } from '../i18n/messages';

export const UPDATE_STATE_EVENT = 'picket:update-state';

export function initialBrowserUpdateSnapshot(currentVersion = PACKAGE_VERSION): UpdateStateSnapshot {
  return {
    protocolVersion: UPDATE_PROTOCOL_VERSION,
    currentVersion,
    phase: 'idle',
    releasePageUrl: PICKET_RELEASES_URL,
    installBlocker: 'devBuild',
    canInstall: false,
  };
}

export async function getUpdateStatus(): Promise<UpdateStateSnapshot> {
  if (!isTauri()) {
    return initialBrowserUpdateSnapshot();
  }
  const reply = await invoke<unknown>('get_update_status');
  return decodeUpdateStateSnapshot(reply);
}

export async function checkForUpdate(manual: boolean, locale?: AppLocale): Promise<UpdateStateSnapshot> {
  if (!isTauri()) {
    throw new Error(getMessages(locale).desktop.disconnectedUpdater);
  }
  const reply = await invoke<unknown>('check_for_update', {
    request: {
      manual,
      ...(locale ? { locale } : {}),
    },
  });
  return decodeUpdateStateSnapshot(reply);
}

export async function installUpdate(version: string, locale?: AppLocale): Promise<UpdateStateSnapshot> {
  if (!isTauri()) {
    throw new Error(getMessages(locale).desktop.disconnectedUpdater);
  }
  const reply = await invoke<unknown>('install_update', {
    request: {
      version,
      ...(locale ? { locale } : {}),
    },
  });
  return decodeUpdateStateSnapshot(reply);
}

export async function cancelUpdate(locale?: AppLocale): Promise<UpdateStateSnapshot> {
  if (!isTauri()) {
    return initialBrowserUpdateSnapshot();
  }
  const reply = await invoke<unknown>('cancel_update', {
    request: {
      ...(locale ? { locale } : {}),
    },
  });
  return decodeUpdateStateSnapshot(reply);
}

export async function openReleasePage(locale?: AppLocale): Promise<void> {
  if (!isTauri()) {
    if (typeof window !== 'undefined') {
      window.open(PICKET_RELEASES_URL, '_blank', 'noopener,noreferrer');
    }
    return;
  }
  await invoke<void>('open_release_page', {
    request: {
      ...(locale ? { locale } : {}),
    },
  });
}

export function subscribeUpdateState(receive: (snapshot: UpdateStateSnapshot) => void): () => void {
  if (!isTauri()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | undefined;
  void listen<unknown>(UPDATE_STATE_EVENT, (event) => {
    if (disposed) return;
    try {
      receive(decodeUpdateStateSnapshot(event.payload));
    } catch {
      // Ignore malformed update events.
    }
  }).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  }).catch(() => {});
  return () => {
    disposed = true;
    unlisten?.();
  };
}
