import { act, createElement } from 'react';
import type { GitOpenFileReply } from '../../../../../shared/ide-protocol';
import type { WorkbenchRestoreReply } from '../../../../../shared/workbench-persistence-protocol';
import { Workbench } from '../../Workbench';
import { disconnectedBackend } from '../../commands';
import { fixture } from '../fixtures';
import { repositorySnapshot } from '../repository-fixture';
import { assert, click, clickElement, text, tick, type BrowserCheckHarness } from './harness';

export async function runLocalKeepChecks(harness: BrowserCheckHarness) {
  const { root, checks, nextKey, nextReadSequence, createReadsBackend, mount } = harness;
  const reads = createReadsBackend();
  // 19. Local Keep fixed group: default collapsed, right-click & drag move-in/move-out, select-all tri-state, gentle notice, conflict visibility, and cold-restart hydration:
  const localKeepOpenCalls: string[] = [];
  const localKeepSetCalls: Array<{ paths: string[]; keep: boolean }> = [];
  const localKeepState = fixture();
  localKeepState.worktrees = localKeepState.worktrees.map((tree) => ({
    ...tree,
    files: tree.files.map((file) => ({ ...file, commitToken: `tok-lk-${tree.worktreeId}-${file.id}` })),
  }));
  await mount(localKeepState, {
    ...disconnectedBackend,
    repositoryReadsAvailable: true,
    async execute(command) {
      if (command.kind === 'openFile') {
        localKeepOpenCalls.push(command.fileId ?? command.path ?? '');
        return {
          protocolVersion: 1,
          kind: 'openFile',
          requestId: command.requestId,
          target: command.target,
          editor: command.editor,
          filePath: 'src/service/Order.ts',
          action: 'diff',
        } satisfies GitOpenFileReply;
      }
      if (command.kind === 'setLocalKeep') {
        localKeepSetCalls.push({ paths: command.paths, keep: command.keep });
        const seq = nextReadSequence();
        const snap = repositorySnapshot(seq);
        const currentKept = new Set<string>(
          localKeepSetCalls.reduce<string[]>((acc, item) => {
            const next = new Set(acc);
            for (const p of item.paths) {
              if (item.keep) next.add(p);
              else next.delete(p);
            }
            return [...next];
          }, []),
        );
        snap.repository.localKeepPaths = [...currentKept].sort();
        snap.worktrees = snap.worktrees.map((wt) => ({
          ...wt,
          files: wt.files.map((f) => ({
            ...f,
            localKeep: currentKept.has(f.path),
            commitToken: `tok-seq${seq}-${f.id}-${currentKept.has(f.path) ? 'keep' : 'norm'}`,
          })),
        }));
        return {
          protocolVersion: 1,
          kind: 'localKeep',
          requestId: command.requestId,
          target: command.target,
          snapshot: snap,
        };
      }
      return disconnectedBackend.execute(command);
    },
  });

  const keepHeader = () => document.querySelector<HTMLButtonElement>('.local-keep-header')!;
  const keepCount = () => document.querySelector('.local-keep-count')?.textContent?.trim();
  assert(
    keepHeader().getAttribute('aria-expanded') === 'false' && keepCount() === '0',
    'Local Keep group must render at bottom collapsed by default with 0 count',
  );

  // IDEA-style single-click, Shift+Click range, Cmd+Click multi-select, and batch right-click Keep Locally:
  const rowByPath = (path: string) => document.querySelector<HTMLElement>(`.file-row[data-file-path="${path}"]`)!;
  const clickRowWithMods = async (path: string, init: MouseEventInit = {}) => {
    await act(async () => {
      rowByPath(path).querySelector<HTMLButtonElement>('.file-open')!.dispatchEvent(
        new MouseEvent('click', { bubbles: true, cancelable: true, ...init }),
      );
    });
  };
  await clickRowWithMods('src/service/Order.ts');
  assert(
    rowByPath('src/service/Order.ts').classList.contains('is-selected')
      && document.querySelectorAll('.file-row.is-selected').length === 1
      && localKeepOpenCalls.length === 0,
    'Single click must select only the clicked file row without opening IDE',
  );
  await clickRowWithMods('src/utils/retry.ts', { shiftKey: true });
  assert(
    rowByPath('src/service/Order.ts').classList.contains('is-selected')
      && rowByPath('src/api/pay.ts').classList.contains('is-selected')
      && rowByPath('src/utils/retry.ts').classList.contains('is-selected')
      && document.querySelectorAll('.file-row.is-selected').length === 3,
    'Shift+Click must select the contiguous range of file rows',
  );
  await clickRowWithMods('src/api/pay.ts', { metaKey: true });
  assert(
    !rowByPath('src/api/pay.ts').classList.contains('is-selected')
      && document.querySelectorAll('.file-row.is-selected').length === 2,
    'Cmd+Click on a selected row must toggle it off',
  );
  await clickRowWithMods('src/api/pay.ts', { metaKey: true });
  assert(document.querySelectorAll('.file-row.is-selected').length === 3, 'Cmd+Click on an unselected row must add it to selection');
  await act(async () => {
    rowByPath('src/api/pay.ts').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 90, clientY: 150 }));
  });
  const batchMenuItems = [...document.querySelectorAll<HTMLButtonElement>('.file-context-menu [role="menuitem"]')];
  assert(
    batchMenuItems.length === 1
      && batchMenuItems[0].textContent?.includes('Keep Locally (Do Not Commit)')
      && batchMenuItems[0].textContent?.includes('3'),
    'Right-clicking multi-selected rows must show single Keep Locally menu item with batch count 3',
  );
  await clickElement(batchMenuItems[0]);
  assert(
    keepCount() === '3' && localKeepOpenCalls.length === 0,
    'Clicking Keep Locally on multi-selected rows must move all 3 files to Local Keep simultaneously',
  );
  await clickElement(keepHeader());
  await clickRowWithMods('src/service/Order.ts');
  await clickRowWithMods('src/utils/retry.ts', { shiftKey: true });
  await act(async () => {
    rowByPath('src/service/Order.ts').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 90, clientY: 220 }));
  });
  const restoreBatchItem = document.querySelector<HTMLButtonElement>('.file-context-menu [role="menuitem"]')!;
  assert(
    document.querySelectorAll('.file-context-menu [role="menuitem"]').length === 1
      && restoreBatchItem.textContent?.includes('Move Back to Changes')
      && restoreBatchItem.textContent?.includes('3'),
    'Right-clicking multi-selected Local Keep rows must show Move Back to Changes with count 3',
  );
  await clickElement(restoreBatchItem);
  assert(keepCount() === '0', 'Batch Move Back to Changes must restore all 3 files');
  await clickElement(keepHeader());
  assert(keepHeader().getAttribute('aria-expanded') === 'false', 'Local Keep should collapse back after batch restore test');

  const notesRow = () => document.querySelector<HTMLElement>('.file-row[data-file-path="RELEASE_NOTES.md"]')!;
  await act(async () => {
    notesRow().dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 90, clientY: 160 }));
  });
  const menuItem = document.querySelector<HTMLButtonElement>('.file-context-menu [role="menuitem"]')!;
  assert(menuItem?.textContent?.includes('Keep Locally (Do Not Commit)'), 'Right-clicking normal file did not open Keep Locally menu item');
  await clickElement(menuItem);
  assert(
    keepCount() === '1'
      && keepHeader().getAttribute('aria-expanded') === 'false'
      && !document.querySelector('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]')
      && localKeepOpenCalls.length === 0,
    'Context-menu Keep Locally must move file to collapsed Local Keep without opening IDE',
  );

  const topSelectAll = () => document.querySelector<HTMLInputElement>('.changes-summary .file-checkbox')!;
  assert(topSelectAll().indeterminate && !topSelectAll().checked, 'Top Select All should be indeterminate when lockfile in normal changes is unselected');
  await act(async () => { topSelectAll().click(); });
  assert(
    topSelectAll().checked && !topSelectAll().indeterminate && text().includes('4/4 selected'),
    'Selecting all normal changes must mark top Select All checked even when Local Keep file is unselected',
  );
  assert(!document.querySelector('.local-keep-notice'), 'Gentle Local Keep notice must not appear when no Local Keep file is checked');

  await clickElement(keepHeader());
  assert(keepHeader().getAttribute('aria-expanded') === 'true' && !!document.querySelector('.local-keep-list .file-row[data-file-path="RELEASE_NOTES.md"]'), 'Expanding Local Keep did not reveal kept file row');
  const keptCheckbox = () => document.querySelector<HTMLInputElement>('.local-keep-list input[aria-label="Select RELEASE_NOTES.md"]')!;
  assert(!keptCheckbox().checked, 'Select All must never select Local Keep files');
  await act(async () => { keptCheckbox().click(); });
  assert(
    keptCheckbox().checked
      && !!document.querySelector('.local-keep-notice')
      && text().includes('1 Local Keep file is checked and will be included'),
    'Checking a Local Keep file did not display the inline gentle notice',
  );
  await clickElement(document.querySelector<HTMLButtonElement>('.local-keep-notice button')!);
  assert(!keptCheckbox().checked && !document.querySelector('.local-keep-notice') && topSelectAll().checked, 'Uncheck button in gentle notice should uncheck only Local Keep files');

  await act(async () => { keptCheckbox().click(); });
  assert(keptCheckbox().checked && topSelectAll().checked, 'Top Select All should remain checked when all normal changes are selected');
  await act(async () => { topSelectAll().click(); });
  assert(
    !topSelectAll().checked && !topSelectAll().indeterminate && !keptCheckbox().checked && text().includes('No changes selected'),
    'Deselect All must also clear manually checked Local Keep files',
  );

  const keptRow = document.querySelector<HTMLElement>('.local-keep-list .file-row[data-file-path="RELEASE_NOTES.md"]')!;
  const keptRowRect = keptRow.getBoundingClientRect();
  const normalZone = document.querySelector<HTMLElement>('.normal-changes-zone')!;
  const normalZoneRect = normalZone.getBoundingClientRect();
  await act(async () => {
    keptRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 10, clientX: keptRowRect.left + 20, clientY: keptRowRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 10, clientX: keptRowRect.left + 28, clientY: keptRowRect.top + 18 }));
  });
  assert(normalZone.classList.contains('drop-zone-ready'), 'Dragging a Local Keep file did not highlight normal changes zone as ready');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 10, clientX: normalZoneRect.left + 40, clientY: normalZoneRect.top + 16 }));
  });
  assert(normalZone.classList.contains('drop-zone-over') && text().includes('Drop to move back to changes'), 'Hovering normal changes zone during drag did not show active drop highlight');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 10, clientX: normalZoneRect.left + 40, clientY: normalZoneRect.top + 16 }));
  });
  assert(
    keepCount() === '0'
      && !!document.querySelector('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]')
      && localKeepOpenCalls.length === 0,
    'Pointer drag-drop back to normal changes failed or accidentally opened IDE',
  );

  await clickElement(keepHeader());
  assert(keepHeader().getAttribute('aria-expanded') === 'false', 'Local Keep should collapse on header click');
  const payRow = document.querySelector<HTMLElement>('.normal-changes-zone .file-row[data-file-path="src/api/pay.ts"]')!;
  const payRect = payRow.getBoundingClientRect();
  const keepRect = keepHeader().getBoundingClientRect();
  const keepSection = document.querySelector<HTMLElement>('.local-keep-section')!;
  await act(async () => {
    payRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 11, clientX: payRect.left + 20, clientY: payRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 11, clientX: payRect.left + 32, clientY: payRect.top + 22 }));
  });
  assert(keepSection.classList.contains('drop-zone-ready') && !!document.querySelector('.file-drag-ghost'), 'Pointer drag past threshold did not activate drag ghost or Local Keep drop zone');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 11, clientX: keepRect.left + 40, clientY: keepRect.top + 12 }));
  });
  assert(keepSection.classList.contains('drop-zone-over') && text().includes('Drop to keep locally'), 'Pointer hover over collapsed Local Keep header did not show active drop highlight');
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 11, clientX: keepRect.left + 40, clientY: keepRect.top + 12 }));
    keepHeader().click();
  });
  assert(
    keepCount() === '1'
      && keepHeader().getAttribute('aria-expanded') === 'false'
      && localKeepOpenCalls.length === 0,
    'Pointer drop onto collapsed Local Keep header must move file without toggling expand or opening IDE',
  );

  // Fixed-bottom Local Keep + drag-to-edge auto-scrolling on overflowing file lists:
  await mount(fixture('long-paths'));
  const changesPanelEl = document.querySelector<HTMLElement>('.changes-panel')!;
  const scrollList = document.querySelector<HTMLElement>('.changes-list')!;
  const pinnedKeepSection = document.querySelector<HTMLElement>('.local-keep-section')!;
  const pinnedKeepHeader = document.querySelector<HTMLButtonElement>('.local-keep-header')!;
  assert(
    scrollList.scrollHeight > scrollList.clientHeight + 300
      && scrollList.scrollTop === 0
      && !scrollList.contains(pinnedKeepSection)
      && Math.abs(pinnedKeepSection.getBoundingClientRect().bottom - changesPanelEl.getBoundingClientRect().bottom) <= 2,
    'Local Keep must be fixed at the bottom of .changes-panel outside .changes-list even when file list overflows at scrollTop=0',
  );
  // Dragging a file directly onto the fixed bottom Local Keep bar at scrollTop=0 immediately moves it without scrolling:
  const midRow = document.querySelector<HTMLElement>('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]')!;
  const midRect = midRow.getBoundingClientRect();
  const pinnedKeepRect = pinnedKeepHeader.getBoundingClientRect();
  let nativeDragCancelled = false;
  await act(async () => {
    const nativeDragStart = new DragEvent('dragstart', { bubbles: true, cancelable: true });
    midRow.dispatchEvent(nativeDragStart);
    nativeDragCancelled = nativeDragStart.defaultPrevented;
    midRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 21, clientX: midRect.left + 24, clientY: midRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 21, clientX: pinnedKeepRect.left + 40, clientY: pinnedKeepRect.top + 12 }));
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 21, clientX: pinnedKeepRect.left + 40, clientY: pinnedKeepRect.top + 12 }));
  });
  assert(nativeDragCancelled && keepCount() === '1' && scrollList.scrollTop === 0, 'Dropping directly onto fixed-bottom Local Keep at scrollTop=0 must move the file immediately');

  // Holding dragged file near bottom edge of .changes-list (above Local Keep) auto-scrolls .changes-list downward:
  const topRow = document.querySelector<HTMLElement>('.normal-changes-zone .file-row')!;
  const topRowRect = topRow.getBoundingClientRect();
  const listRect = scrollList.getBoundingClientRect();
  await act(async () => {
    topRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 22, clientX: topRowRect.left + 24, clientY: topRowRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 22, clientX: listRect.left + 60, clientY: listRect.bottom - 4 }));
  });
  await act(async () => {
    await new Promise<void>((resolve) => {
      const deadline = performance.now() + 6000;
      const waitBottom = () => {
        if (scrollList.scrollTop + scrollList.clientHeight >= scrollList.scrollHeight - 2 || performance.now() >= deadline) {
          resolve();
          return;
        }
        requestAnimationFrame(waitBottom);
      };
      requestAnimationFrame(waitBottom);
    });
  });
  assert(scrollList.scrollTop > 300, `Holding near bottom edge of .changes-list must auto-scroll down (scrollTop=${scrollList.scrollTop})`);
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 22, clientX: listRect.left + 60, clientY: listRect.bottom - 4 }));
  });

  // Expand fixed Local Keep at bottom, grab kept row, drag near top edge of .changes-list -> auto-scrolls back to top and drops back:
  await clickElement(pinnedKeepHeader);
  const keptNotesRow = document.querySelector<HTMLElement>('.local-keep-list .file-row[data-file-path="RELEASE_NOTES.md"]')!;
  const keptNotesRect = keptNotesRow.getBoundingClientRect();
  const listRectAfterExpand = scrollList.getBoundingClientRect();
  await act(async () => {
    keptNotesRow.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 23, clientX: keptNotesRect.left + 24, clientY: keptNotesRect.top + 10 }));
    window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 23, clientX: listRectAfterExpand.left + 60, clientY: listRectAfterExpand.top + 4 }));
  });
  await act(async () => {
    await new Promise<void>((resolve) => {
      const deadline = performance.now() + 6000;
      const waitTop = () => {
        if (scrollList.scrollTop <= 2 || performance.now() >= deadline) {
          resolve();
          return;
        }
        requestAnimationFrame(waitTop);
      };
      requestAnimationFrame(waitTop);
    });
  });
  assert(
    scrollList.scrollTop <= 2,
    `Holding dragged file near top edge must auto-scroll back to top (scrollTop=${scrollList.scrollTop}, scrollHeight=${scrollList.scrollHeight}, clientHeight=${scrollList.clientHeight})`,
  );
  await act(async () => {
    window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 23, clientX: listRectAfterExpand.left + 60, clientY: listRectAfterExpand.top + 24 }));
  });
  assert(keepCount() === '0', 'Dropping back into normal changes after auto-scrolling up must restore the file');

  const conflictKeepState = fixture('conflict');
  conflictKeepState.worktrees[0].files = conflictKeepState.worktrees[0].files.map((file) =>
    file.status === 'U' ? { ...file, localKeep: true } : file,
  );
  await mount(conflictKeepState);
  assert(
    keepHeader().getAttribute('aria-expanded') === 'false'
      && !!document.querySelector('.local-keep-conflict-badge')
      && (document.querySelector('.local-keep-conflict-badge')?.textContent ?? '').includes('conflict'),
    'Collapsed Local Keep header must surface conflict badge when a conflicted file is inside Local Keep',
  );

  const coldKeepSnap = repositorySnapshot(nextReadSequence());
  coldKeepSnap.repository.localKeepPaths = ['RELEASE_NOTES.md'];
  coldKeepSnap.worktrees[0].files = coldKeepSnap.worktrees[0].files.map((f) =>
    f.path === 'RELEASE_NOTES.md' ? { ...f, localKeep: true } : f,
  );
  await act(async () => {
    root.render(createElement(Workbench, {
      key: nextKey(),
      backend: {
        ...reads,
        async loadWorkbenchState(): Promise<WorkbenchRestoreReply> {
          return {
            protocolVersion: 2,
            locale: 'en',
            editor: 'idea',
            defaultCommitLanguage: 'en',
            repositorySettings: [{ repositoryId: 'orders', editor: 'idea', commitLanguage: 'en', localKeepPaths: ['RELEASE_NOTES.md'] }],
            activeWorktreeId: 'pay-tree',
            emptyPrompt: '',
            snapshots: [coldKeepSnap],
            worktrees: [],
            warning: null,
          };
        },
        async saveWorkbenchState() {},
      },
    }));
  });
  await tick();
  assert(
    keepHeader().getAttribute('aria-expanded') === 'false'
      && keepCount() === '1'
      && !document.querySelector('.normal-changes-zone .file-row[data-file-path="RELEASE_NOTES.md"]'),
    'Cold-start hydration did not restore Local Keep group collapsed with kept file excluded from normal changes',
  );

  // Full filename visibility in List & Tree views + draggable Local Keep height resize handle (hidden when empty):
  {
    window.localStorage.removeItem('picket.localKeepHeight');
    const emptyKeepState = fixture('typing');
    await mount(emptyKeepState);
    await clickElement(keepHeader());
    assert(
      keepHeader().getAttribute('aria-expanded') === 'true'
        && keepCount() === '0'
        && !document.querySelector('.local-keep-resizer'),
      'Resize handle must stay hidden when Local Keep is expanded with 0 files',
    );

    const longJavaPath = 'inspur-scm-aggregation-service/src/main/java/com/inspur/scm/aggregation/report/service/ShipSpendTimeCalculationService.java';
    const siblingVoPath = 'inspur-scm-aggregation-service/src/main/java/com/inspur/scm/aggregation/report/vo/LimsIndicatorsSaveVO.java';
    const siblingTestPath = 'inspur-scm-aggregation-service/src/test/java/com/inspur/scm/aggregation/report/service/InspectPendingMatchServiceImplTest.java';
    const longSqlPath = 'sql-change/lims_indicators_optional_fields_20261127.sql';
    const layoutState = fixture('typing');
    layoutState.worktrees[0].files = [
      {
        id: 'sql-long',
        path: longSqlPath,
        status: 'A',
        range: { kind: 'unstaged', added: 69, removed: 0 },
      },
      {
        id: 'java-vo',
        path: siblingVoPath,
        status: 'M',
        range: { kind: 'unstaged', added: 3, removed: 0 },
      },
      {
        id: 'java-main',
        path: longJavaPath,
        status: 'M',
        range: { kind: 'unstaged', added: 42, removed: 43 },
      },
      {
        id: 'java-test',
        path: siblingTestPath,
        status: 'M',
        range: { kind: 'unstaged', added: 91, removed: 0 },
      },
      ...Array.from({ length: 12 }, (_, idx) => ({
        id: `kept-${idx}`,
        path: `inspur-meta-starter/src/main/resources/application-profile-${idx}.yml`,
        status: 'M' as const,
        localKeep: true,
        range: { kind: 'unstaged' as const, added: 3, removed: 2 },
      })),
    ];
    layoutState.worktrees[0].selectedIds = ['sql-long', 'java-vo', 'java-main', 'java-test'];
    await mount(layoutState);

    const assertSingleLineFileName = (nameEl: HTMLElement, expectedExt: string, context: string) => {
      const stemEl = nameEl.querySelector<HTMLElement>('.file-stem')!;
      const extEl = nameEl.querySelector<HTMLElement>('.file-ext')!;
      const nameRect = nameEl.getBoundingClientRect();
      const stemRect = stemEl.getBoundingClientRect();
      const extRect = extEl.getBoundingClientRect();
      assert(
        Boolean(stemEl)
          && Boolean(extEl)
          && extEl.textContent === expectedExt
          && nameRect.width > 200
          && nameRect.height <= 20
          && Math.abs(extRect.top - stemRect.top) <= 2
          && extRect.width > 0
          && extRect.right <= nameRect.right + 1
          && nameEl.scrollWidth <= nameEl.clientWidth + 1,
        `${context}: filename must stay on one line with intact extension (w=${nameRect.width}, h=${nameRect.height}, stemTop=${stemRect.top}, extTop=${extRect.top})`,
      );
    };

    const listNameEl = document.querySelector<HTMLElement>(`.file-row[data-file-path="${longJavaPath}"] .file-name`)!;
    const listRowEl = listNameEl.closest<HTMLElement>('.file-row')!;
    assert(
      listRowEl.classList.contains('has-directory'),
      'List view must stack directory below filename',
    );
    assertSingleLineFileName(listNameEl, '.java', 'List view long Java filename');

    const listSqlNameEl = document.querySelector<HTMLElement>(`.file-row[data-file-path="${longSqlPath}"] .file-name`)!;
    const listSqlStemEl = listSqlNameEl.querySelector<HTMLElement>('.file-stem')!;
    assertSingleLineFileName(listSqlNameEl, '.sql', 'List view long SQL filename');
    assert(
      listSqlStemEl.scrollWidth > listSqlStemEl.clientWidth,
      'Over-long SQL stem must ellipsize cleanly on one line instead of wrapping the extension',
    );

    await click('Switch to directory tree view');
    const moduleHeading = document.querySelector<HTMLElement>('.normal-changes-zone .directory-heading[data-dir-path="inspur-scm-aggregation-service/src"]')!;
    const compactedMainHeading = document.querySelector<HTMLElement>('.normal-changes-zone .directory-heading[data-dir-path="inspur-scm-aggregation-service/src/main/java/com/inspur/scm/aggregation/report"]')!;
    const treeNameEl = document.querySelector<HTMLElement>(`.file-row.grouped[data-file-path="${longJavaPath}"] .file-name`)!;
    const treeSqlNameEl = document.querySelector<HTMLElement>(`.file-row.grouped[data-file-path="${longSqlPath}"] .file-name`)!;
    assert(
      Boolean(moduleHeading)
        && moduleHeading.classList.contains('is-module')
        && moduleHeading.getBoundingClientRect().height <= 28
        && Boolean(compactedMainHeading)
        && (compactedMainHeading.textContent ?? '').includes('main/…')
        && (compactedMainHeading.textContent ?? '').includes('report')
        && compactedMainHeading.getBoundingClientRect().height <= 28,
      `Directory tree must render single-line compressed hierarchy (moduleH=${moduleHeading?.getBoundingClientRect().height})`,
    );
    assertSingleLineFileName(treeNameEl, '.java', 'Tree view long Java filename');
    assertSingleLineFileName(treeSqlNameEl, '.sql', 'Tree view long SQL filename');

    // Module-level checkbox deselects/selects all descendant files in one click; folding hides & restores descendants:
    const moduleCheckbox = moduleHeading.querySelector<HTMLInputElement>('.file-checkbox')!;
    await act(async () => { moduleCheckbox.click(); });
    assert(
      !moduleCheckbox.checked
        && !document.querySelector<HTMLInputElement>(`[aria-label="Select ${siblingVoPath}"]`)!.checked
        && !document.querySelector<HTMLInputElement>(`[aria-label="Select ${longJavaPath}"]`)!.checked
        && !document.querySelector<HTMLInputElement>(`[aria-label="Select ${siblingTestPath}"]`)!.checked,
      'Unchecking top-level module folder must deselect all descendant files',
    );
    await act(async () => { moduleCheckbox.click(); });
    assert(moduleCheckbox.checked, 'Rechecking top-level module folder must select all descendant files');

    const moduleToggle = moduleHeading.querySelector<HTMLButtonElement>('.directory-toggle')!;
    await clickElement(moduleToggle);
    assert(
      moduleToggle.getAttribute('aria-expanded') === 'false'
        && !document.querySelector(`.normal-changes-zone .file-row[data-file-path="${longJavaPath}"]`),
      'Collapsing module folder must hide descendant rows',
    );
    await clickElement(moduleToggle);
    assert(
      moduleToggle.getAttribute('aria-expanded') === 'true'
        && Boolean(document.querySelector(`.normal-changes-zone .file-row[data-file-path="${longJavaPath}"]`)),
      'Re-expanding module folder must restore descendant rows',
    );
    await clickElement(keepHeader());
    assert(
      document.querySelectorAll('.local-keep-list .directory-heading').length === 1
        && (document.querySelector('.local-keep-list .directory-heading')?.textContent ?? '').includes('inspur-meta-starter/src/main/resources')
        && document.querySelectorAll('.local-keep-list .file-row.grouped').length === 12,
      'Local Keep must also switch to directory tree grouping when tree view is active',
    );
    await click('Switch to list view');
    assert(
      document.querySelectorAll('.local-keep-list .directory-heading').length === 0
        && document.querySelectorAll('.local-keep-list .file-row.has-directory').length === 12,
      'Local Keep must switch back to flat list rows when list view is active',
    );
    await clickElement(keepHeader());

    assert(!document.querySelector('.local-keep-resizer'), 'Resize handle must stay hidden while Local Keep is collapsed');
    await clickElement(keepHeader());
    const resizer = document.querySelector<HTMLElement>('.local-keep-resizer')!;
    const keepList = document.querySelector<HTMLElement>('.local-keep-list')!;
    const initialKeepHeight = keepList.getBoundingClientRect().height;
    assert(
      Boolean(resizer) && initialKeepHeight >= 170 && initialKeepHeight <= 190,
      `Expanded Local Keep with 12 files should default to ~180px height with resize handle (got ${initialKeepHeight})`,
    );

    // Drag splitter upward by 120px -> Local Keep grows, normal changes retains headroom:
    const resizerRect = resizer.getBoundingClientRect();
    const startY = resizerRect.top + 4;
    await act(async () => {
      resizer.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerId: 41, clientX: resizerRect.left + 80, clientY: startY }));
      window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, pointerId: 41, clientX: resizerRect.left + 80, clientY: startY - 120 }));
      window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 41, clientX: resizerRect.left + 80, clientY: startY - 120 }));
    });
    const tallerHeight = keepList.getBoundingClientRect().height;
    const normalListHeight = document.querySelector<HTMLElement>('.changes-list')!.getBoundingClientRect().height;
    assert(
      tallerHeight >= initialKeepHeight + 90 && normalListHeight >= 90,
      `Dragging Local Keep resizer upward must enlarge Local Keep while keeping normal changes visible (taller=${tallerHeight}, normal=${normalListHeight})`,
    );

    // Double-click resizer -> resets cleanly back to default ~180px and clears persisted override:
    await act(async () => {
      resizer.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    });
    const resetHeight = keepList.getBoundingClientRect().height;
    assert(
      Math.abs(resetHeight - 180) <= 4 && window.localStorage.getItem('picket.localKeepHeight') === null,
      `Double-clicking Local Keep resizer must restore default height (got ${resetHeight})`,
    );
  }
  checks.push('Local Keep fixed group: collapsed default, right-click & drag move-in/move-out, resizable height, full list/tree filenames, select-all tri-state, gentle notice, conflict badge, and cold-restart restore');
}
