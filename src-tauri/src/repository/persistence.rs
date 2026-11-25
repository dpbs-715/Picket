//! Local workbench persistence (UI locale, editor preference, default commit language,
//! per-repository settings, opened repositories, and per-worktree extra prompts, selections,
//! tabs, and UI scope versions).
//! Never stores API keys or Git write tokens; repository paths come only from Rust.
use super::*;
use crate::i18n::{self, LanguageKind, deserialize_language_or_default};
use serde::{Deserialize, Serialize};
use std::{io::Write, os::unix::fs::PermissionsExt};
use tempfile::NamedTempFile;

const STORAGE_VERSION: u8 = 2;
const LEGACY_V1_STORAGE_VERSION: u8 = 1;
const STORAGE_FILE: &str = "workbench-state.json";
const MAX_PROMPT_BYTES: usize = 1024 * 1024;
const MAX_STORED_REPOSITORIES: usize = 32;
const MAX_STORED_WORKTREES: usize = 128;
const MIN_FULL_WINDOW_WIDTH: u32 = 400;
const MIN_FULL_WINDOW_HEIGHT: u32 = 480;
const MAX_STORED_WINDOW_DIMENSION: u32 = 16_384;
const MAX_STORED_WINDOW_COORD_ABS: i32 = 65_536;

fn is_valid_window_preferences(prefs: &PersistedWindowPreferences) -> bool {
    if let Some(size) = prefs.full_size
        && (size.width < MIN_FULL_WINDOW_WIDTH
            || size.width > MAX_STORED_WINDOW_DIMENSION
            || size.height < MIN_FULL_WINDOW_HEIGHT
            || size.height > MAX_STORED_WINDOW_DIMENSION)
    {
        return false;
    }
    if let Some(pos) = prefs.full_position
        && (pos.x.saturating_abs() > MAX_STORED_WINDOW_COORD_ABS
            || pos.y.saturating_abs() > MAX_STORED_WINDOW_COORD_ABS)
    {
        return false;
    }
    if let Some(pos) = prefs.compact_position
        && (pos.x.saturating_abs() > MAX_STORED_WINDOW_COORD_ABS
            || pos.y.saturating_abs() > MAX_STORED_WINDOW_COORD_ABS)
    {
        return false;
    }
    true
}

#[derive(Clone, Default)]
pub struct WorkbenchPersistence {
    last_revision: Arc<tokio::sync::Mutex<u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredRepository {
    repository_id: String,
    root_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredLocalKeep {
    common_dir: String,
    paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredWorkbenchFile {
    version: u8,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    locale: LanguageKind,
    editor: EditorKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    default_commit_language: LanguageKind,
    #[serde(default)]
    repository_settings: Vec<PersistedRepositorySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_preferences: Option<PersistedWindowPreferences>,
    #[serde(default)]
    local_keep: Vec<StoredLocalKeep>,
    active_worktree_id: Option<String>,
    empty_prompt: String,
    repositories: Vec<StoredRepository>,
    worktrees: Vec<PersistedWorktreeState>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyV1WorktreeState {
    repository_id: String,
    worktree_id: String,
    #[expect(dead_code)]
    draft_text: String,
    #[expect(dead_code)]
    draft_editing: bool,
    #[expect(dead_code)]
    draft_outdated: bool,
    selected_ids: Vec<String>,
    scope_version: String,
    tab: WorkbenchTab,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyV1WorkbenchFile {
    version: u8,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    locale: LanguageKind,
    editor: EditorKind,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    default_commit_language: LanguageKind,
    #[serde(default)]
    repository_settings: Vec<PersistedRepositorySettings>,
    #[serde(default)]
    local_keep: Vec<StoredLocalKeep>,
    active_worktree_id: Option<String>,
    #[expect(dead_code)]
    empty_draft: String,
    #[expect(dead_code)]
    empty_editing: bool,
    repositories: Vec<StoredRepository>,
    worktrees: Vec<LegacyV1WorktreeState>,
}

fn empty_restore_reply(warning: Option<String>) -> WorkbenchRestoreReply {
    WorkbenchRestoreReply {
        protocol_version: STORAGE_VERSION,
        locale: LanguageKind::En,
        editor: EditorKind::Idea,
        default_commit_language: LanguageKind::En,
        repository_settings: Vec::new(),
        window_preferences: None,
        installed_editors: None,
        active_worktree_id: None,
        empty_prompt: String::new(),
        worktrees: Vec::new(),
        snapshots: Vec::new(),
        connections: None,
        models: None,
        active_model_id: None,
        c_push_sessions: None,
        warning,
    }
}

fn parse_workbench_bytes(bytes: &[u8]) -> Option<StoredWorkbenchFile> {
    if let Ok(parsed) = serde_json::from_slice::<StoredWorkbenchFile>(bytes)
        && parsed.version == STORAGE_VERSION
        && parsed
            .window_preferences
            .as_ref()
            .is_none_or(is_valid_window_preferences)
    {
        return Some(parsed);
    }
    if let Ok(v1) = serde_json::from_slice::<LegacyV1WorkbenchFile>(bytes)
        && v1.version == LEGACY_V1_STORAGE_VERSION
    {
        return Some(StoredWorkbenchFile {
            version: STORAGE_VERSION,
            locale: v1.locale,
            editor: v1.editor,
            default_commit_language: v1.default_commit_language,
            repository_settings: v1.repository_settings,
            window_preferences: None,
            local_keep: v1.local_keep,
            active_worktree_id: v1.active_worktree_id,
            // Discard legacy v1 commit message drafts silently upon upgrading to v2:
            empty_prompt: String::new(),
            repositories: v1.repositories,
            worktrees: v1
                .worktrees
                .into_iter()
                .map(|wt| PersistedWorktreeState {
                    repository_id: wt.repository_id,
                    worktree_id: wt.worktree_id,
                    prompt_text: String::new(),
                    selected_ids: wt.selected_ids,
                    unselected_ids: Vec::new(),
                    scope_version: wt.scope_version,
                    tab: wt.tab,
                })
                .collect(),
        });
    }
    None
}

fn read_existing_valid(path: &Path) -> Option<StoredWorkbenchFile> {
    let bytes = fs::read(path).ok()?;
    parse_workbench_bytes(&bytes)
}

fn sanitize_repository_settings(
    entries: Vec<PersistedRepositorySettings>,
) -> Vec<PersistedRepositorySettings> {
    let mut seen = HashSet::with_capacity(entries.len());
    entries
        .into_iter()
        .filter(|entry| !entry.repository_id.is_empty() && seen.insert(entry.repository_id.clone()))
        .map(|mut entry| {
            entry.local_keep_paths.clear();
            entry
        })
        .collect()
}

fn merge_stored_local_keep(
    store: &RepositoryStore,
    previous: Option<&StoredWorkbenchFile>,
    locale: LanguageKind,
) -> Result<Vec<StoredLocalKeep>> {
    let ExportedLocalKeep {
        entries: live_entries,
        known_common_dirs,
    } = store.export_persisted_local_keep(locale)?;
    let mut merged = BTreeMap::<String, Vec<String>>::new();
    for (common_dir, paths) in live_entries {
        merged.insert(common_dir, paths);
    }
    if let Some(prev) = previous {
        for existing in &prev.local_keep {
            if merged.len() >= MAX_STORED_REPOSITORIES * 2 {
                break;
            }
            if existing.common_dir.is_empty() || known_common_dirs.contains(&existing.common_dir) {
                continue;
            }
            let canon_known = fs::canonicalize(&existing.common_dir)
                .ok()
                .and_then(|canon| canon.to_str().map(str::to_owned))
                .is_some_and(|canon_str| known_common_dirs.contains(&canon_str));
            if canon_known {
                continue;
            }
            let sanitized: BTreeSet<String> = existing
                .paths
                .iter()
                .filter(|path| is_valid_local_keep_path(path))
                .take(MAX_LOCAL_KEEP_PATHS_PER_REPO)
                .cloned()
                .collect();
            if !sanitized.is_empty() {
                merged.insert(existing.common_dir.clone(), sanitized.into_iter().collect());
            }
        }
    }
    Ok(merged
        .into_iter()
        .map(|(common_dir, paths)| StoredLocalKeep { common_dir, paths })
        .collect())
}

fn write_workbench_file(
    storage_dir: &Path,
    file_path: &Path,
    payload: &StoredWorkbenchFile,
) -> Result<()> {
    fs::create_dir_all(storage_dir)
        .map_err(|_| "Cannot create local application data directory".to_string())?;
    let _ = fs::set_permissions(storage_dir, fs::Permissions::from_mode(0o700));
    let bytes = serde_json::to_vec_pretty(payload).map_err(|error| error.to_string())?;
    let mut temp = NamedTempFile::new_in(storage_dir)
        .map_err(|_| "Cannot create temporary workbench state file".to_string())?;
    let _ = fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600));
    temp.write_all(&bytes)
        .map_err(|_| "Cannot write workbench state file".to_string())?;
    temp.as_file()
        .sync_all()
        .map_err(|_| "Cannot sync workbench state file to disk".to_string())?;
    temp.persist(file_path)
        .map_err(|_| "Cannot atomically persist workbench state file".to_string())?;
    Ok(())
}

impl WorkbenchPersistence {
    pub async fn restore(
        &self,
        storage_dir: &Path,
        store: &RepositoryStore,
    ) -> Result<WorkbenchRestoreReply> {
        let _guard = self.last_revision.lock().await;
        let file_path = storage_dir.join(STORAGE_FILE);
        let bytes = match fs::read(&file_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(empty_restore_reply(None));
            }
            Err(_) => {
                return Ok(empty_restore_reply(Some(
                    i18n::persistence_unreadable_warning(LanguageKind::En).into(),
                )));
            }
        };
        let Some(stored) = parse_workbench_bytes(&bytes) else {
            return Ok(empty_restore_reply(Some(
                i18n::persistence_incompatible_warning(LanguageKind::En).into(),
            )));
        };

        let raw_local_keep: Vec<(String, Vec<String>)> = stored
            .local_keep
            .iter()
            .map(|entry| (entry.common_dir.clone(), entry.paths.clone()))
            .collect();
        store.import_persisted_local_keep(&raw_local_keep)?;

        let mut snapshots = Vec::new();
        let mut skipped_count = 0usize;
        for repo in &stored.repositories {
            match store
                .open_with_locale(Path::new(&repo.root_path), stored.locale)
                .await
            {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(_) => skipped_count += 1,
            }
        }
        let mut repository_settings = sanitize_repository_settings(stored.repository_settings);
        for setting in &mut repository_settings {
            if let Some(snap) = snapshots
                .iter()
                .find(|snap| snap.repository.id == setting.repository_id)
            {
                setting.local_keep_paths = snap.repository.local_keep_paths.clone();
            } else if let Some(offline_paths) =
                store.offline_local_keep_for_repo_id(&setting.repository_id)
            {
                setting.local_keep_paths = offline_paths;
            }
        }
        let warning = (skipped_count > 0)
            .then(|| i18n::persistence_unavailable_repos_warning(stored.locale, skipped_count));
        Ok(WorkbenchRestoreReply {
            protocol_version: STORAGE_VERSION,
            locale: stored.locale,
            editor: stored.editor,
            default_commit_language: stored.default_commit_language,
            repository_settings,
            window_preferences: stored.window_preferences,
            installed_editors: None,
            active_worktree_id: stored.active_worktree_id,
            empty_prompt: stored.empty_prompt,
            worktrees: stored.worktrees,
            snapshots,
            connections: None,
            models: None,
            active_model_id: None,
            c_push_sessions: None,
            warning,
        })
    }

    pub async fn save(
        &self,
        storage_dir: &Path,
        store: &RepositoryStore,
        request: WorkbenchSaveRequest,
    ) -> Result<()> {
        let locale = request.locale;
        if request.empty_prompt.len() > MAX_PROMPT_BYTES
            || request.worktrees.len() > MAX_STORED_WORKTREES
        {
            return Err(i18n::persistence_exceeds_limits(locale).into());
        }
        if let Some(ref prefs) = request.window_preferences
            && !is_valid_window_preferences(prefs)
        {
            return Err(i18n::persistence_exceeds_limits(locale).into());
        }
        if request.repository_settings.len() > MAX_STORED_REPOSITORIES * 2 {
            return Err(i18n::persistence_invalid_repo_settings_entry(locale).into());
        }
        let mut seen_repo_settings = HashSet::with_capacity(request.repository_settings.len());
        for setting in &request.repository_settings {
            if setting.repository_id.is_empty()
                || setting.local_keep_paths.len() > MAX_LOCAL_KEEP_PATHS_PER_REPO
                || !setting
                    .local_keep_paths
                    .iter()
                    .all(|path| is_valid_local_keep_path(path))
                || !seen_repo_settings.insert(setting.repository_id.as_str())
            {
                return Err(i18n::persistence_invalid_repo_settings_entry(locale).into());
            }
        }

        let mut seen_worktrees = HashSet::with_capacity(request.worktrees.len());
        for item in &request.worktrees {
            if item.repository_id.is_empty()
                || item.worktree_id.is_empty()
                || item.prompt_text.len() > MAX_PROMPT_BYTES
                || item.selected_ids.len() > 10_000
                || item.unselected_ids.len() > 10_000
                || !seen_worktrees.insert(item.worktree_id.as_str())
            {
                return Err(i18n::persistence_invalid_worktree_entry(locale).into());
            }
        }

        let mut last_revision = self.last_revision.lock().await;
        if request.revision == 0 || request.revision <= *last_revision {
            return Ok(());
        }

        let live_roots = store.registered_roots()?;
        let mut opened_repo_ids = HashSet::with_capacity(live_roots.len());
        let mut repositories = Vec::with_capacity(live_roots.len());
        for (repository_id, root) in live_roots {
            opened_repo_ids.insert(repository_id.clone());
            repositories.push(StoredRepository {
                repository_id,
                root_path: display_with_locale(&root, locale)?,
            });
        }

        let file_path = storage_dir.join(STORAGE_FILE);
        let previous = read_existing_valid(&file_path);
        if let Some(previous_state) = &previous {
            for existing_repo in &previous_state.repositories {
                if repositories.len() >= MAX_STORED_REPOSITORIES {
                    break;
                }
                if !opened_repo_ids.contains(&existing_repo.repository_id) {
                    repositories.push(existing_repo.clone());
                }
            }
        }

        let mut repository_settings = sanitize_repository_settings(request.repository_settings);
        if let Some(previous_state) = &previous {
            let incoming_setting_ids: HashSet<String> = repository_settings
                .iter()
                .map(|item| item.repository_id.clone())
                .collect();
            for existing_setting in &previous_state.repository_settings {
                if repository_settings.len() >= MAX_STORED_REPOSITORIES {
                    break;
                }
                if !existing_setting.repository_id.is_empty()
                    && !incoming_setting_ids.contains(&existing_setting.repository_id)
                {
                    let mut cloned = existing_setting.clone();
                    cloned.local_keep_paths.clear();
                    repository_settings.push(cloned);
                }
            }
        }

        let local_keep = merge_stored_local_keep(store, previous.as_ref(), locale)?;

        let window_preferences = request.window_preferences.or_else(|| {
            previous
                .as_ref()
                .and_then(|prev| prev.window_preferences.clone())
        });

        let mut worktrees = request.worktrees;
        if let Some(previous_state) = previous {
            let incoming_ids: HashSet<String> = worktrees
                .iter()
                .map(|item| item.worktree_id.clone())
                .collect();
            for existing_tree in previous_state.worktrees {
                if worktrees.len() >= MAX_STORED_WORKTREES {
                    break;
                }
                if !opened_repo_ids.contains(&existing_tree.repository_id)
                    && !incoming_ids.contains(&existing_tree.worktree_id)
                {
                    worktrees.push(existing_tree);
                }
            }
        }

        let payload = StoredWorkbenchFile {
            version: STORAGE_VERSION,
            locale,
            editor: request.editor,
            default_commit_language: request.default_commit_language,
            repository_settings,
            window_preferences,
            local_keep,
            active_worktree_id: request.active_worktree_id,
            empty_prompt: request.empty_prompt,
            repositories,
            worktrees,
        };
        write_workbench_file(storage_dir, &file_path, &payload)?;
        *last_revision = request.revision;
        Ok(())
    }

    pub async fn sync_local_keep(
        &self,
        storage_dir: &Path,
        store: &RepositoryStore,
        repository_id: &str,
        removed_selection_paths: &[String],
        locale: LanguageKind,
    ) -> Result<()> {
        let _guard = self.last_revision.lock().await;
        let file_path = storage_dir.join(STORAGE_FILE);
        let previous = read_existing_valid(&file_path);

        let live_roots = store.registered_roots()?;
        let mut opened_repo_ids = HashSet::with_capacity(live_roots.len());
        let mut repositories = Vec::with_capacity(live_roots.len());
        for (repo_id, root) in live_roots {
            opened_repo_ids.insert(repo_id.clone());
            repositories.push(StoredRepository {
                repository_id: repo_id,
                root_path: display_with_locale(&root, locale)?,
            });
        }
        if let Some(prev) = &previous {
            for existing_repo in &prev.repositories {
                if repositories.len() >= MAX_STORED_REPOSITORIES {
                    break;
                }
                if !opened_repo_ids.contains(&existing_repo.repository_id) {
                    repositories.push(existing_repo.clone());
                }
            }
        }

        let local_keep = merge_stored_local_keep(store, previous.as_ref(), locale)?;
        let removed_ids: HashSet<String> = removed_selection_paths
            .iter()
            .map(|path| file_id_for_path(path))
            .collect();

        let payload = if let Some(mut prev) = previous {
            prev.repositories = repositories;
            prev.local_keep = local_keep;
            prev.repository_settings = sanitize_repository_settings(prev.repository_settings);
            if !removed_ids.is_empty() {
                for tree in &mut prev.worktrees {
                    if tree.repository_id == repository_id {
                        tree.selected_ids.retain(|id| !removed_ids.contains(id));
                        tree.unselected_ids.retain(|id| !removed_ids.contains(id));
                    }
                }
            }
            prev
        } else {
            StoredWorkbenchFile {
                version: STORAGE_VERSION,
                locale,
                editor: EditorKind::Idea,
                default_commit_language: LanguageKind::En,
                repository_settings: Vec::new(),
                window_preferences: None,
                local_keep,
                active_worktree_id: None,
                empty_prompt: String::new(),
                repositories,
                worktrees: Vec::new(),
            }
        };
        write_workbench_file(storage_dir, &file_path, &payload)
    }
}
