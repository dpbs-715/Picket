import { useEffect, useState, type MutableRefObject } from 'react';
import type { SkillsStateSnapshot } from '../../../../shared/skills-protocol';
import type { UpdateStateSnapshot } from '../../../../shared/update-protocol';
import { initialBrowserSkillsSnapshot } from '../../../desktop/skills';
import { initialBrowserUpdateSnapshot } from '../../../desktop/updater';
import { getMessages } from '../../../i18n';
import type { WorkbenchBackend } from '../commands';
import type { WorkbenchState } from '../state/workbench';

interface UseSettingsServicesParams {
  backend: WorkbenchBackend;
  hydrated: boolean;
  initialState: WorkbenchState | undefined;
  settingsOpen: boolean;
  latestState: MutableRefObject<WorkbenchState>;
}

export function useSettingsServices({
  backend,
  hydrated,
  initialState,
  settingsOpen,
  latestState,
}: UseSettingsServicesParams) {
  const [updateState, setUpdateState] = useState<UpdateStateSnapshot>(() => initialBrowserUpdateSnapshot());
  const [skillsState, setSkillsState] = useState<SkillsStateSnapshot>(() => initialBrowserSkillsSnapshot());
  const [skillsBusy, setSkillsBusy] = useState<'sync' | 'toggle' | 'reset' | null>(null);
  const [skillsError, setSkillsError] = useState('');
  const [skillsNotice, setSkillsNotice] = useState('');

  const hasUpdate = updateState.phase === 'available'
    || updateState.phase === 'downloading'
    || updateState.phase === 'installing';

  useEffect(() => {
    let mounted = true;
    void backend.getUpdateStatus?.().then((snapshot) => {
      if (mounted) setUpdateState(snapshot);
    }).catch(() => {});
    const unsubscribe = backend.subscribeUpdateState?.((snapshot) => {
      if (mounted) setUpdateState(snapshot);
    });
    return () => {
      mounted = false;
      unsubscribe?.();
    };
  }, [backend]);

  useEffect(() => {
    if (!backend.getSkillsState) return;
    let mounted = true;
    void backend.getSkillsState(latestState.current.locale).then((snapshot) => {
      if (mounted) setSkillsState(snapshot);
    }).catch(() => {});
    return () => {
      mounted = false;
    };
  }, [backend, latestState, settingsOpen]);

  useEffect(() => {
    const isSmoke = import.meta.env.VITE_PICKET_SMOKE === '1'
      && Boolean((globalThis as { __PICKET_UI_SMOKE__?: boolean }).__PICKET_UI_SMOKE__);
    if (!hydrated || initialState || !backend.repositoryReadsAvailable || !backend.checkForUpdate || isSmoke) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void backend.checkForUpdate?.(false, latestState.current.locale).then((snapshot) => {
        if (!cancelled) setUpdateState(snapshot);
      }).catch(() => {});
    }, 4_000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [backend, hydrated, initialState, latestState]);

  async function handleCheckUpdate() {
    if (!backend.checkForUpdate) return;
    setUpdateState((previous) => ({ ...previous, phase: 'checking', error: undefined }));
    try {
      const next = await backend.checkForUpdate(true, latestState.current.locale);
      setUpdateState(next);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateState((previous) => ({ ...previous, phase: 'error', error: message, canInstall: false }));
    }
  }

  async function handleInstallUpdate(version: string) {
    if (!backend.installUpdate) return;
    try {
      const next = await backend.installUpdate(version, latestState.current.locale);
      setUpdateState(next);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateState((previous) => ({ ...previous, phase: 'error', error: message, canInstall: false }));
    }
  }

  async function handleCancelUpdate() {
    if (!backend.cancelUpdate) return;
    try {
      const next = await backend.cancelUpdate(latestState.current.locale);
      setUpdateState(next);
    } catch {
      // Keep current snapshot if cancel failed.
    }
  }

  function handleOpenReleasePage() {
    void backend.openReleasePage?.(latestState.current.locale).catch((error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateState((previous) => ({ ...previous, phase: 'error', error: message }));
    });
  }

  async function handleSyncSkills(sourceRepo?: string): Promise<boolean> {
    if (!backend.syncSkills || skillsBusy) return false;
    setSkillsBusy('sync');
    setSkillsError('');
    setSkillsNotice('');
    try {
      const next = await backend.syncSkills(sourceRepo, latestState.current.locale);
      setSkillsState(next);
      const enabledCount = next.skills.filter((item) => item.enabled).length;
      const shortSha = (next.sourceCommit ?? '').slice(0, 7) || 'HEAD';
      setSkillsNotice(getMessages(latestState.current.locale).settings.skills.syncSuccessNotice(enabledCount, shortSha));
      return true;
    } catch (error) {
      const message = error instanceof Error
        ? error.message
        : getMessages(latestState.current.locale).settings.skills.syncErrorFallback;
      setSkillsError(message);
      return false;
    } finally {
      setSkillsBusy(null);
    }
  }

  async function handleToggleSkill(name: string, enabled: boolean) {
    if (!backend.setEnabledSkills || skillsBusy) return;
    const currentEnabled = new Set(skillsState.skills.filter((item) => item.enabled).map((item) => item.name));
    if (enabled) currentEnabled.add(name);
    else currentEnabled.delete(name);
    setSkillsBusy('toggle');
    setSkillsError('');
    setSkillsNotice('');
    try {
      const next = await backend.setEnabledSkills([...currentEnabled], latestState.current.locale);
      setSkillsState(next);
    } catch (error) {
      const message = error instanceof Error
        ? error.message
        : getMessages(latestState.current.locale).settings.skills.toggleErrorFallback;
      setSkillsError(message);
    } finally {
      setSkillsBusy(null);
    }
  }

  async function handleResetSkills() {
    if (!backend.resetSkills || skillsBusy) return;
    setSkillsBusy('reset');
    setSkillsError('');
    setSkillsNotice('');
    try {
      const next = await backend.resetSkills(latestState.current.locale);
      setSkillsState(next);
      setSkillsNotice(getMessages(latestState.current.locale).settings.skills.resetSuccessNotice);
    } catch (error) {
      const message = error instanceof Error
        ? error.message
        : getMessages(latestState.current.locale).settings.skills.resetErrorFallback;
      setSkillsError(message);
    } finally {
      setSkillsBusy(null);
    }
  }

  return {
    updateState,
    hasUpdate,
    handleCheckUpdate,
    handleInstallUpdate,
    handleCancelUpdate,
    handleOpenReleasePage,
    skillsState,
    skillsBusy,
    skillsError,
    skillsNotice,
    handleSyncSkills,
    handleToggleSkill,
    handleResetSkills,
  };
}
