//! Local workbench persistence for M2 (editor preference, opened repositories,
//! and per-worktree drafts, selections, tabs, and UI scope versions).
//! Never stores API keys or Git write tokens; repository paths come only from Rust.
use super::*;
use serde::{Deserialize, Serialize};
use std::{io::Write, os::unix::fs::PermissionsExt};
use tempfile::NamedTempFile;

const STORAGE_VERSION: u8 = 1;
const STORAGE_FILE: &str = "workbench-state.json";
const MAX_DRAFT_BYTES: usize = 1024 * 1024;
const MAX_STORED_REPOSITORIES: usize = 32;
const MAX_STORED_WORKTREES: usize = 128;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredWorkbenchFile {
    version: u8,
    editor: EditorKind,
    active_worktree_id: Option<String>,
    empty_draft: String,
    empty_editing: bool,
    repositories: Vec<StoredRepository>,
    worktrees: Vec<PersistedWorktreeState>,
}

fn empty_restore_reply(warning: Option<String>) -> WorkbenchRestoreReply {
    WorkbenchRestoreReply {
        protocol_version: 1,
        editor: EditorKind::Idea,
        active_worktree_id: None,
        empty_draft: String::new(),
        empty_editing: false,
        worktrees: Vec::new(),
        snapshots: Vec::new(),
        models: None,
        active_model_id: None,
        c_push_sessions: None,
        warning,
    }
}

fn read_existing_valid(path: &Path) -> Option<StoredWorkbenchFile> {
    let bytes = fs::read(path).ok()?;
    let parsed: StoredWorkbenchFile = serde_json::from_slice(&bytes).ok()?;
    (parsed.version == STORAGE_VERSION).then_some(parsed)
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
                    "无法读取本地工作台保存记录，已从空白状态启动。".into(),
                )));
            }
        };
        let stored = match serde_json::from_slice::<StoredWorkbenchFile>(&bytes) {
            Ok(parsed) if parsed.version == STORAGE_VERSION => parsed,
            _ => {
                return Ok(empty_restore_reply(Some(
                    "本地工作台保存记录版本或格式不兼容，已从安全空状态启动；原记录未被覆盖。"
                        .into(),
                )));
            }
        };

        let mut snapshots = Vec::new();
        let mut skipped_count = 0usize;
        for repo in &stored.repositories {
            match store.open(Path::new(&repo.root_path)).await {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(_) => skipped_count += 1,
            }
        }
        let warning = (skipped_count > 0).then(|| {
            format!(
                "有 {skipped_count} 个上次打开的仓库目录当前不可访问，已保留其已保存的草稿与选择。"
            )
        });
        Ok(WorkbenchRestoreReply {
            protocol_version: 1,
            editor: stored.editor,
            active_worktree_id: stored.active_worktree_id,
            empty_draft: stored.empty_draft,
            empty_editing: stored.empty_editing,
            worktrees: stored.worktrees,
            snapshots,
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
        if request.empty_draft.len() > MAX_DRAFT_BYTES
            || request.worktrees.len() > MAX_STORED_WORKTREES
        {
            return Err("工作台草稿或工作树状态超出持久化上限".into());
        }
        let mut seen_worktrees = HashSet::with_capacity(request.worktrees.len());
        for item in &request.worktrees {
            if item.repository_id.is_empty()
                || item.worktree_id.is_empty()
                || item.draft_text.len() > MAX_DRAFT_BYTES
                || item.selected_ids.len() > 10_000
                || !seen_worktrees.insert(item.worktree_id.as_str())
            {
                return Err("工作树持久化条目无效或存在重复身份".into());
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
                root_path: display(&root)?,
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

        fs::create_dir_all(storage_dir).map_err(|_| "无法创建应用本地数据目录")?;
        let _ = fs::set_permissions(storage_dir, fs::Permissions::from_mode(0o700));
        let payload = StoredWorkbenchFile {
            version: STORAGE_VERSION,
            editor: request.editor,
            active_worktree_id: request.active_worktree_id,
            empty_draft: request.empty_draft,
            empty_editing: request.empty_editing,
            repositories,
            worktrees,
        };
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?;
        let mut temp =
            NamedTempFile::new_in(storage_dir).map_err(|_| "无法创建工作台状态临时文件")?;
        let _ = fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600));
        temp.write_all(&bytes)
            .map_err(|_| "无法写入工作台状态数据")?;
        temp.as_file()
            .sync_all()
            .map_err(|_| "无法同步工作台状态数据到磁盘")?;
        temp.persist(&file_path)
            .map_err(|_| "无法原子更新工作台状态文件")?;
        *last_revision = request.revision;
        Ok(())
    }
}
