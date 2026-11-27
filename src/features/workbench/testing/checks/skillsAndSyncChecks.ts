import { act } from 'react';
import { DEFAULT_SKILLS_SOURCE_REPO, SKILLS_PROTOCOL_VERSION, type SkillsStateSnapshot } from '../../../../../shared/skills-protocol';
import { disconnectedBackend } from '../../commands';
import type { WorkbenchState } from '../../state/workbench';
import { fixture } from '../fixtures';
import { assert, click, clickElement, openSettingsTab, tick, type, type BrowserCheckHarness } from './harness';

export async function runSkillsAndSyncChecks(harness: BrowserCheckHarness) {
  const { checks, mount } = harness;
  // Phase 4 Settings -> Skills: bundled state, custom source edit, error preservation, sync, per-skill toggle, and Restore Built-in:
  {
    const baseBundledSkills: SkillsStateSnapshot = {
      protocolVersion: SKILLS_PROTOCOL_VERSION,
      mode: 'bundled',
      sourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
      defaultSourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
      skills: [
        { name: 'commit', description: 'Create focused git commits.', enabled: true, builtIn: true, source: 'bundled', fileCount: 1, referenceCount: 0 },
        { name: 'dcr', description: 'Review git diffs with findings.', enabled: true, builtIn: true, source: 'bundled', fileCount: 1, referenceCount: 0 },
      ],
    };
    let currentSkills: SkillsStateSnapshot = structuredClone(baseBundledSkills);
    let failNextSync = true;
    const syncedRepos: Array<string | undefined> = [];
    const toggledSets: string[][] = [];
    let resetCount = 0;

    await mount(fixture('typing'), {
      ...disconnectedBackend,
      async getSkillsState() {
        return structuredClone(currentSkills);
      },
      async syncSkills(sourceRepo) {
        syncedRepos.push(sourceRepo);
        if (failNextSync) {
          failNextSync = false;
          throw new Error('Repository not reachable');
        }
        currentSkills = {
          protocolVersion: SKILLS_PROTOCOL_VERSION,
          mode: 'synced',
          sourceRepo: sourceRepo ?? DEFAULT_SKILLS_SOURCE_REPO,
          defaultSourceRepo: DEFAULT_SKILLS_SOURCE_REPO,
          sourceCommit: '1234567890abcdef1234567890abcdef12345678',
          syncedAt: '2026-11-26T06:00:00Z',
          skills: [
            { name: 'commit', description: 'Synced commit skill.', enabled: true, builtIn: true, source: 'synced', fileCount: 1, referenceCount: 0 },
            { name: 'dcr', description: 'Synced diff review skill.', enabled: true, builtIn: true, source: 'synced', fileCount: 1, referenceCount: 0 },
            { name: 'release-tag', description: 'Tag release versions safely.', enabled: false, builtIn: false, source: 'synced', fileCount: 2, referenceCount: 1 },
          ],
        };
        return structuredClone(currentSkills);
      },
      async setEnabledSkills(enabledSkills) {
        toggledSets.push([...enabledSkills]);
        const enabledSet = new Set(enabledSkills);
        currentSkills = {
          ...currentSkills,
          skills: currentSkills.skills.map((item) => ({
            ...item,
            enabled: enabledSet.has(item.name),
          })),
        };
        return structuredClone(currentSkills);
      },
      async resetSkills() {
        resetCount += 1;
        currentSkills = structuredClone(baseBundledSkills);
        return structuredClone(currentSkills);
      },
    });
    await click('Settings');
    await openSettingsTab('skills');
    await tick();
    const skillsSection = () => document.querySelector<HTMLElement>('#settings-skills-section');
    assert(
      skillsSection()?.dataset.skillsMode === 'bundled'
        && (skillsSection()?.textContent ?? '').includes(DEFAULT_SKILLS_SOURCE_REPO)
        && !document.querySelector('[data-skills-reset]'),
      'Settings -> Skills did not render initial bundled state cleanly',
    );
    const bundledCommitCheck = document.querySelector<HTMLInputElement>('[data-skill-name="commit"] input[type="checkbox"]');
    assert(Boolean(bundledCommitCheck?.disabled), 'Bundled skill checkbox should be read-only before syncing');

    await clickElement(document.querySelector<HTMLButtonElement>('[data-skills-edit-source]')!);
    const sourceInput = document.querySelector<HTMLInputElement>('[data-skills-source-input]')!;
    type(sourceInput, 'https://github.com/acme/team-skills');
    await tick();
    await clickElement(document.querySelector<HTMLButtonElement>('[data-skills-sync]')!);
    await tick();
    assert(
      skillsSection()?.dataset.skillsMode === 'bundled'
        && (skillsSection()?.textContent ?? '').includes('Repository not reachable')
        && document.querySelector<HTMLInputElement>('[data-skills-source-input]')?.value === 'https://github.com/acme/team-skills',
      'Failed skill sync did not preserve mode, error alert, and edited source repository URL',
    );

    await clickElement(document.querySelector<HTMLButtonElement>('[data-skills-sync]')!);
    await tick();
    assert(
      skillsSection()?.dataset.skillsMode === 'synced'
        && syncedRepos.at(-1) === 'https://github.com/acme/team-skills'
        && (skillsSection()?.textContent ?? '').includes('1234567')
        && !document.querySelector('[data-skills-source-input]')
        && Boolean(document.querySelector('[data-skills-reset]')),
      'Successful skill sync did not update mode, commit metadata, or reveal Restore Built-in',
    );

    const customSkillCheck = document.querySelector<HTMLInputElement>('[data-skill-name="release-tag"] input[type="checkbox"]')!;
    assert(!customSkillCheck.disabled && !customSkillCheck.checked, 'Synced custom skill should start unchecked and interactive');
    await act(async () => { customSkillCheck.click(); });
    await tick();
    assert(
      toggledSets.length === 1
        && toggledSets[0].includes('release-tag')
        && document.querySelector<HTMLInputElement>('[data-skill-name="release-tag"] input[type="checkbox"]')?.checked === true
        && (document.querySelector('[data-settings-tab="skills"]')?.textContent ?? '').includes('3/3'),
      `Toggling custom skill did not invoke setEnabledSkills or update enabled count (${JSON.stringify(toggledSets)})`,
    );

    await clickElement(document.querySelector<HTMLButtonElement>('[data-skills-reset]')!);
    await tick();
    assert(
      resetCount === 1
        && skillsSection()?.dataset.skillsMode === 'bundled'
        && !document.querySelector('[data-skills-reset]'),
      'Restore Built-in did not reset Skills section back to bundled mode',
    );
    checks.push('Phase 4 Settings -> Skills: bundled view, custom source edit, failed-sync preservation, hot-sync metadata, per-skill toggle, and Restore Built-in');
  }

  // Zero-flash Overlay Navigation Stack (Manage Actions -> Manual/Create -> Back -> Close) & Pinned Overlay Footers:
  {
    await mount(fixture('typing'));
    await click('Switch primary action');
    await click('Manage Actions…');
    const managerOverlay = document.querySelector<HTMLElement>('.overlay[data-overlay-depth="0"]');
    assert(Boolean(managerOverlay) && (managerOverlay?.textContent ?? '').includes('Manage Actions'), 'Manage Actions did not open at stack depth 0');
    await click('Manual');
    const overlaysAfterPush = [...document.querySelectorAll<HTMLElement>('.overlay')];
    assert(
      overlaysAfterPush.length === 2
        && overlaysAfterPush[0] === managerOverlay
        && overlaysAfterPush[0].hasAttribute('inert')
        && overlaysAfterPush[1].dataset.overlayDepth === '1'
        && overlaysAfterPush[1].dataset.overlayVariant === 'drill'
        && Number(getComputedStyle(overlaysAfterPush[1]).zIndex) > Number(getComputedStyle(overlaysAfterPush[0]).zIndex),
      'Pushing Manual action from Manage Actions must keep Manage Actions mounted underneath (zero workbench flash) with higher z-index drill overlay on top',
    );
    const actionFooterRect = overlaysAfterPush[1].querySelector<HTMLElement>('.overlay-footer')!.getBoundingClientRect();
    const actionOverlayRect = overlaysAfterPush[1].getBoundingClientRect();
    assert(
      Math.abs(actionFooterRect.bottom - actionOverlayRect.bottom) <= 1.5,
      `CustomActionModal footer must stay pinned to bottom of overlay (footerBottom=${actionFooterRect.bottom}, overlayBottom=${actionOverlayRect.bottom})`,
    );
    await clickElement(overlaysAfterPush[1].querySelector<HTMLButtonElement>('[data-overlay-back]')!);
    assert(
      document.querySelectorAll('.overlay').length === 1
        && document.querySelector('.overlay') === managerOverlay
        && !managerOverlay!.hasAttribute('inert'),
      'Back from New Custom Action must reveal the same mounted Manage Actions instance without remounting or flashing workbench',
    );
    await click('Create');
    assert(document.querySelectorAll('.overlay').length === 2, 'Pushing Create from Manage Actions must stack over Manage Actions');
    await click('Close New Custom Action');
    assert(!document.querySelector('.overlay'), 'Clicking × on stacked New Custom Action must close all action overlays straight back to workbench');

    // Verify ProviderConnectionForm footer stays pinned to bottom even when Advanced fields expand:
    await click('Settings');
    await openSettingsTab('models');
    await click('Provider');
    await clickElement(document.querySelector<HTMLButtonElement>('.advanced-toggle')!);
    const modelOverlayEl = document.querySelector<HTMLElement>('.overlay[data-overlay-depth="1"]')!;
    const modelFooterRect = modelOverlayEl.querySelector<HTMLElement>('.overlay-footer')!.getBoundingClientRect();
    const modelOverlayRect = modelOverlayEl.getBoundingClientRect();
    assert(
      Math.abs(modelFooterRect.bottom - modelOverlayRect.bottom) <= 1.5,
      `Add Provider footer must stay pinned to bottom of overlay even with Advanced expanded (footerBottom=${modelFooterRect.bottom}, overlayBottom=${modelOverlayRect.bottom})`,
    );
    await click('Close Add Provider');
    assert(!document.querySelector('.overlay'), 'Closing Add Provider via × must return straight to workbench');
    checks.push('Zero-flash Overlay Navigation Stack (Manage Actions -> Manual/Create -> Back/Close) and bottom-pinned form footers');
  }

  // CustomActionModal selective repo sync with tri-state Select All, deferred until clicking Save:
  {
    const baseState = fixture('typing');
    const multiRepoState: WorkbenchState = {
      ...baseState,
      repositories: [
        ...baseState.repositories,
        {
          id: 'web',
          name: 'picket-web',
          branches: [{ name: 'main', worktreeId: 'web-tree' }],
        },
      ],
      worktrees: [
        ...baseState.worktrees,
        {
          ...baseState.worktrees[0],
          repositoryId: 'web',
          worktreeId: 'web-tree',
          sessionId: 'web-session',
          label: 'main',
          path: '/test-only/picket-web',
          branch: 'main',
          files: [],
          selectedIds: [],
          unselectedIds: [],
          prompt: { text: '', revision: 0 },
          messages: [],
        },
      ],
      repositorySettings: {
        ...baseState.repositorySettings,
        web: {
          editor: 'idea',
          commitLanguage: 'en',
          customActions: [{ id: 'act-web-existing', label: 'Release Tag', prompt: 'npm version patch' }],
        },
      },
    };
    await mount(multiRepoState);

    await click('Switch primary action');
    await click('New Action…');
    assert(Boolean(document.querySelector('[data-sync-picker]')), 'New Custom Action modal must render selective sync picker when other repos exist');

    const selectAllInput = () => document.querySelector<HTMLInputElement>('[data-sync-select-all]')!;
    const cliRow = () => document.querySelector<HTMLElement>('[data-sync-repo-id="cli"]')!;
    const webRow = () => document.querySelector<HTMLElement>('[data-sync-repo-id="web"]')!;
    const cliCheckbox = () => cliRow().querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    const webCheckbox = () => webRow().querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    const saveSubmitButton = () => document.querySelector<HTMLButtonElement>('.custom-action-form button[type="submit"]')!;

    assert(
      !selectAllInput().checked && !selectAllInput().indeterminate
        && !cliCheckbox().checked && !webCheckbox().checked
        && saveSubmitButton().textContent?.trim() === 'Save Action',
      'Sync picker must start unchecked with default Save Action label',
    );

    const labelInput = document.querySelector<HTMLInputElement>('.custom-action-form input[maxlength]')!;
    const promptTextarea = document.querySelector<HTMLTextAreaElement>('.custom-action-form textarea')!;
    type(labelInput, 'Release Tag');
    type(promptTextarea, 'git tag v0.2.0 && git push origin v0.2.0');
    assert(
      (webRow().textContent ?? '').includes('Same name'),
      `Matching existing label in picket-web must show Same name badge (got: ${webRow().textContent})`,
    );

    // Check only `cli` -> Select All becomes indeterminate, button becomes Save & Sync (1):
    await clickElement(cliCheckbox());
    assert(
      !selectAllInput().checked && selectAllInput().indeterminate
        && cliCheckbox().checked && !webCheckbox().checked
        && saveSubmitButton().textContent?.trim() === 'Save & Sync (1)',
      `Checking 1 of 2 repos must set Select All indeterminate and update submit button (text=${saveSubmitButton().textContent})`,
    );

    // Clicking Select All selects both and marks picket-web as Will overwrite:
    await clickElement(selectAllInput());
    assert(
      selectAllInput().checked && !selectAllInput().indeterminate
        && cliCheckbox().checked && webCheckbox().checked
        && (webRow().textContent ?? '').includes('Will overwrite')
        && saveSubmitButton().textContent?.trim() === 'Save & Sync (2)',
      'Clicking Select All must check all repos and show Will overwrite for same-name target',
    );

    const switchRepoTo = async (currentRepoName: string, targetRepoName: string) => {
      await click(currentRepoName);
      const targetBtn = [...document.querySelectorAll<HTMLButtonElement>('.repository-menu button')]
        .find((b) => b.textContent?.includes(targetRepoName));
      assert(targetBtn, `Missing repository menu entry: ${targetRepoName}`);
      await clickElement(targetBtn);
    };

    // Clicking Cancel must NOT sync anything to cli:
    await click('Cancel');
    assert(!document.querySelector('.overlay'), 'Cancel must close CustomActionModal');
    await switchRepoTo('my-order-service', 'picket-cli');
    await click('Switch primary action');
    assert(
      !(document.querySelector('.popover')?.textContent ?? '').includes('Release Tag'),
      'Cancelling modal after ticking sync targets must NOT sync action to other repositories',
    );
    await click('Switch primary action');

    // Switch back to my-order-service, create action selecting ONLY picket-cli, and click Save & Sync (1):
    await switchRepoTo('picket-cli', 'my-order-service');
    await click('Switch primary action');
    await click('New Action…');
    type(document.querySelector<HTMLInputElement>('.custom-action-form input[maxlength]')!, 'Release Tag');
    type(document.querySelector<HTMLTextAreaElement>('.custom-action-form textarea')!, 'git tag v0.2.0 && git push origin v0.2.0');
    await clickElement(cliCheckbox());
    await clickElement(saveSubmitButton());
    assert(!document.querySelector('.overlay'), 'Clicking Save & Sync must save and close modal');

    // Verify picket-cli received the synced action while picket-web kept its original prompt untouched:
    await switchRepoTo('my-order-service', 'picket-cli');
    await click('Switch primary action');
    assert(
      (document.querySelector('.popover')?.textContent ?? '').includes('Release Tag'),
      'Selected target repository picket-cli must receive the synced action on Save',
    );
    await click('Switch primary action');

    await switchRepoTo('picket-cli', 'picket-web');
    await click('Switch primary action');
    await click('Manage Actions…');
    assert(
      (document.querySelector('.overlay')?.textContent ?? '').includes('npm version patch'),
      'Unchecked repository picket-web must keep its original Release Tag prompt untouched',
    );
    await click('Close Manage Actions');

    // Edit action in my-order-service and verify picket-cli shows Synced -> Will update when checked:
    await switchRepoTo('picket-web', 'my-order-service');
    await click('Switch primary action');
    await click('Manage Actions…');
    await click('Edit action Release Tag');
    assert(
      (cliRow().textContent ?? '').includes('Synced') && !cliCheckbox().checked,
      `Previously synced repo must show Synced badge and stay unchecked by default on Edit (got: ${cliRow().textContent})`,
    );
    await clickElement(cliCheckbox());
    assert(
      (cliRow().textContent ?? '').includes('Will update')
        && saveSubmitButton().textContent?.trim() === 'Save & Sync (1)',
      'Checking previously synced repo on Edit must display Will update and Save & Sync (1)',
    );
    await click('Close Edit Custom Action');
    checks.push('CustomActionModal selective repo sync: tri-state Select All, Same name/Synced/Will update badges, Cancel leaves repos untouched, Save syncs only checked repos');
  }
}
