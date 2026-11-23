pub mod ai_context;
#[cfg(test)]
mod branch_tests;
mod branches;
mod commit;
#[cfg(test)]
mod commit_tests;
#[cfg(test)]
mod edge_tests;
pub mod editor;
#[cfg(test)]
mod editor_tests;
mod parse;
pub mod persistence;
#[cfg(test)]
mod persistence_tests;
pub mod remote;
mod runner;
pub mod smoke;
#[cfg(test)]
mod tests;
pub mod types;

use parse::{StatusEntry, WorktreeEntry};
use runner::GitReader;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use types::*;

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Default)]
pub struct RepositoryStore {
    inner: Arc<StoreInner>,
}
#[derive(Default)]
struct StoreInner {
    reader: GitReader,
    sequence: AtomicU64,
    repositories: Mutex<HashMap<String, Registration>>,
    opened_order: Mutex<Vec<String>>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    branch_requests: Mutex<HashSet<String>>,
    commit_requests: Mutex<HashSet<String>>,
    verified_remotes: Mutex<HashSet<(String, String)>>,
}
#[derive(Clone)]
struct Registration {
    sequence: u64,
    primary_root: PathBuf,
    paths: HashMap<String, PathBuf>,
    branch_tokens: HashMap<String, String>,
    commit_states: HashMap<String, commit::CommitRegistration>,
}
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn identity(prefix: &str, path: &Path) -> String {
    format!("{prefix}-{}", hash(path.as_os_str().as_encoded_bytes()))
}
fn display(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "存在非 UTF-8 目录，当前版本不能安全打开".into())
}
fn label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("工作树")
        .to_owned()
}
fn ignored_for_review(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|name| name.to_str()),
        Some(
            "pnpm-lock.yaml"
                | "package-lock.json"
                | "yarn.lock"
                | "Cargo.lock"
                | "bun.lock"
                | "bun.lockb"
        )
    )
}

impl RepositoryStore {
    pub async fn shutdown(&self) {
        self.inner.reader.shutdown().await;
    }
    pub(crate) fn registered_roots(&self) -> Result<Vec<(String, PathBuf)>> {
        let order = self
            .inner
            .opened_order
            .lock()
            .map_err(|_| "仓库顺序记录不可用")?
            .clone();
        let repositories = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "仓库注册表不可用")?;
        Ok(order
            .into_iter()
            .filter_map(|id| {
                repositories
                    .get(&id)
                    .map(|reg| (id, reg.primary_root.clone()))
            })
            .collect())
    }
    pub async fn open(&self, directory: &Path) -> Result<RepositorySnapshot> {
        let directory = fs::canonicalize(directory)
            .map_err(|_| "所选目录不可访问，请选择本机 Git 工作目录".to_string())?;
        if !directory.is_dir() {
            return Err("请选择目录，而不是文件".into());
        }
        let dir_name = label(&directory);
        let root = match self
            .inner
            .reader
            .text(&directory, &["rev-parse", "--show-toplevel"])
            .await
        {
            Ok(root) => root,
            Err(_) => {
                let is_bare = self
                    .inner
                    .reader
                    .text(&directory, &["rev-parse", "--is-bare-repository"])
                    .await
                    .is_ok_and(|out| out.trim() == "true");
                if is_bare {
                    return Err(format!(
                        "无法打开“{dir_name}”：该目录是 Git 裸仓库（无工作区文件），请选择普通 Git 工作目录"
                    ));
                }
                return Err(format!(
                    "无法打开“{dir_name}”：该目录未受 Git 管理（未找到 .git），请先执行 git init 或选择其他 Git 仓库"
                ));
            }
        };
        let root = fs::canonicalize(root).map_err(|_| "Git 工作目录不可访问".to_string())?;
        self.read_repository(&root).await
    }
    pub async fn refresh(&self, target: &ReadTarget) -> Result<RepositorySnapshot> {
        let path = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "仓库注册表不可用")?
            .get(&target.repository_id)
            .and_then(|repository| repository.paths.get(&target.worktree_id))
            .cloned()
            .ok_or("仓库或工作树未在当前窗口打开，请重新选择目录")?;
        let common = self
            .inner
            .reader
            .text(
                &path,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "无法访问 Git 公共目录")?;
        if identity("repo", &common) != target.repository_id {
            return Err("工作树所属仓库已变化，请重新打开；已保留原草稿和选择".into());
        }
        let snapshot = self.read_repository(&path).await?;
        if snapshot.repository.id != target.repository_id {
            return Err("工作树所属仓库已变化，请重新打开；已保留原草稿和选择".into());
        }
        Ok(snapshot)
    }
    fn repository_gate(&self, repository_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>> {
        Ok(self
            .inner
            .gates
            .lock()
            .map_err(|_| "仓库协调器不可用")?
            .entry(repository_id.into())
            .or_default()
            .clone())
    }
    async fn read_repository(&self, root: &Path) -> Result<RepositorySnapshot> {
        let common = self
            .inner
            .reader
            .text(
                root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "无法访问 Git 公共目录")?;
        let _guard = self
            .repository_gate(&identity("repo", &common))?
            .lock_owned()
            .await;
        self.read_repository_locked(root).await
    }
    async fn read_repository_locked(&self, root: &Path) -> Result<RepositorySnapshot> {
        let sequence = self.inner.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let reader = &self.inner.reader;
        let common = reader
            .text(
                root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common).map_err(|_| "无法访问 Git 公共目录".to_string())?;
        let repository_id = identity("repo", &common);
        let active_id = identity("wt", root);
        let entries = parse::worktrees(
            &reader
                .bytes(root, &["worktree", "list", "--porcelain", "-z"])
                .await?,
        )?;
        let mut worktrees = Vec::new();
        let mut paths = HashMap::new();
        let mut commit_states = HashMap::new();
        for mut entry in entries.into_iter().filter(|entry| !entry.bare) {
            // Canonicalize live paths to keep symlink-opened and worktree-list
            // identities identical; retain missing paths as visibly unavailable.
            if let Ok(path) = fs::canonicalize(&entry.path) {
                entry.path = path;
            }
            let id = identity("wt", &entry.path);
            let tree = match self.read_worktree(&repository_id, &entry).await {
                Ok((tree, commit_state)) => {
                    commit_states.insert(id.clone(), commit_state);
                    tree
                }
                Err(error) if id == active_id => return Err(error),
                Err(error) => WorktreeSnapshot {
                    repository_id: repository_id.clone(),
                    worktree_id: id.clone(),
                    label: label(&entry.path),
                    path: display(&entry.path)?,
                    branch: entry.branch.clone(),
                    detached: entry.branch == "(detached)",
                    head: entry.head.clone(),
                    status_version: String::new(),
                    branch_token: None,
                    read_error: Some(error.clone()),
                    remote: Remote::Error { reason: error },
                    files: vec![],
                },
            };
            paths.insert(id, entry.path);
            worktrees.push(tree);
        }
        if !worktrees.iter().any(|tree| tree.worktree_id == active_id) {
            return Err("Git 未列出所选工作树，请重新打开".into());
        }
        let branch_output = reader
            .bytes(
                root,
                &["for-each-ref", "--format=%(refname)%00", "refs/heads/"],
            )
            .await?;
        let mut branches = Vec::new();
        for record in branch_output.split(|byte| *byte == 0) {
            let record = parse::utf8(record)?;
            let Some(name) = record.trim_start_matches('\n').strip_prefix("refs/heads/") else {
                continue;
            };
            branches.push(Branch {
                name: name.into(),
                worktree_id: worktrees
                    .iter()
                    .find(|tree| tree.branch == name && !tree.detached)
                    .map(|tree| tree.worktree_id.clone()),
            });
        }
        for tree in &worktrees {
            if !tree.detached && !branches.iter().any(|branch| branch.name == tree.branch) {
                branches.push(Branch {
                    name: tree.branch.clone(),
                    worktree_id: Some(tree.worktree_id.clone()),
                });
            }
        }
        let name = if common.file_name().is_some_and(|name| name == ".git") {
            label(common.parent().unwrap_or(root))
        } else {
            label(&common).trim_end_matches(".git").to_owned()
        };
        let snapshot = RepositorySnapshot {
            repository: RepositoryInfo {
                id: repository_id.clone(),
                name,
                branches,
            },
            worktrees,
            active_worktree_id: active_id,
            read_sequence: sequence,
        };
        let mut registered = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "仓库注册表不可用")?;
        if registered
            .get(&repository_id)
            .is_none_or(|previous| previous.sequence < sequence)
        {
            let branch_tokens = snapshot
                .worktrees
                .iter()
                .filter_map(|tree| {
                    tree.branch_token
                        .as_ref()
                        .map(|token| (tree.worktree_id.clone(), token.clone()))
                })
                .collect();
            registered.insert(
                repository_id.clone(),
                Registration {
                    sequence,
                    primary_root: root.to_path_buf(),
                    paths,
                    branch_tokens,
                    commit_states,
                },
            );
            drop(registered);
            let mut order = self
                .inner
                .opened_order
                .lock()
                .map_err(|_| "仓库顺序记录不可用")?;
            if !order.iter().any(|existing| existing == &repository_id) {
                order.push(repository_id);
            }
        }
        Ok(snapshot)
    }
    async fn read_worktree(
        &self,
        repository_id: &str,
        entry: &WorktreeEntry,
    ) -> Result<(WorktreeSnapshot, commit::CommitRegistration)> {
        let root = &entry.path;
        let reader = self.inner.reader.for_worktree(root).await?;
        let before = reader
            .bytes(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        let status = parse::status(&before)?;
        if status.entries.len() > 10_000 {
            return Err("变更文件超过 10000 项，未载入不完整列表".into());
        }
        if status.entries.iter().any(|entry| entry.mode == "160000") {
            return Err(
                "存在子模块变更，当前只读阶段尚不能安全计算其范围；请在 Git / IDE 中查看".into(),
            );
        }
        let mut fingerprints = HashMap::new();
        for file in &status.entries {
            if !staged(file) {
                fingerprints.insert(file.path.clone(), self.working_file(root, &file.path)?);
            }
        }
        let staged_stats = parse::numstat(
            &reader
                .bytes(
                    root,
                    &[
                        "diff",
                        "--cached",
                        "--numstat",
                        "-z",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--find-renames",
                    ],
                )
                .await?,
        )?;
        let unstaged_stats = parse::numstat(
            &reader
                .bytes(
                    root,
                    &[
                        "diff",
                        "--numstat",
                        "-z",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--find-renames",
                    ],
                )
                .await?,
        )?;
        let mut files = Vec::new();
        let mut working_hashes = HashMap::new();
        for file in &status.entries {
            let index = staged(file);
            let stats = if file.xy == "??" {
                let output = reader
                    .run(
                        root,
                        &[
                            "diff",
                            "--no-index",
                            "--numstat",
                            "-z",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--",
                            "/dev/null",
                            &file.path,
                        ],
                    )
                    .await?;
                if !matches!(output.code, Some(0 | 1)) {
                    return Err("无法读取未跟踪文件的 Git 增删统计".into());
                }
                parse::untracked_numstat(&output.bytes, &file.path)?
            } else {
                if index {
                    &staged_stats
                } else {
                    &unstaged_stats
                }
                .get(&file.path)
                .cloned()
                .unwrap_or_default()
            };
            let working = if !index {
                let current = self.working_file(root, &file.path)?;
                if fingerprints
                    .get(&file.path)
                    .is_some_and(|previous| previous != &current)
                {
                    return Err("文件在读取期间发生变化，请重新刷新；当前草稿与选择已保留".into());
                }
                working_hashes.insert(file.path.clone(), current.clone());
                Some(current)
            } else {
                None
            };
            let conflict = file.conflicted;
            let kind = if index {
                if file.xy.as_bytes().get(1).is_some_and(|byte| *byte != b'.') {
                    "partial"
                } else {
                    "staged"
                }
            } else {
                "unstaged"
            };
            let version = format!(
                "{}\0{}\0{}\0{}\0{}",
                file.path,
                file.previous.as_deref().unwrap_or(""),
                if index { "index" } else { "worktree" },
                if index {
                    &file.index_oid
                } else {
                    working.as_deref().unwrap_or("")
                },
                file.index_oid,
            );
            let effective = if file.xy == "??" {
                b'A'
            } else {
                file.xy.as_bytes()[usize::from(!index)]
            };
            let status_code = if conflict {
                "U"
            } else {
                match effective {
                    b'A' => "A",
                    b'D' => "D",
                    b'R' => "R",
                    b'C' => "C",
                    b'T' => "T",
                    _ => "M",
                }
            };
            files.push(Change {
                id: format!("file-{}", hash(&file.path)),
                path: file.path.clone(),
                previous_path: file.previous.clone(),
                status: status_code.into(),
                git_status: file.xy.clone(),
                range: Range {
                    kind: kind.into(),
                    added: stats.added,
                    removed: stats.removed,
                },
                binary: stats.binary,
                ignored_for_review: ignored_for_review(&file.path),
                content_version: hash(version),
                commit_token: String::new(),
            });
        }
        let after = reader
            .bytes(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        if before != after {
            return Err("Git 状态在读取期间发生变化，请重新刷新；当前草稿与选择已保留".into());
        }
        let raw_detached = status.branch == "(detached)";
        let rebase_branch = if raw_detached {
            self.active_rebase_branch(root).await
        } else {
            None
        };
        let display_branch = rebase_branch
            .clone()
            .unwrap_or_else(|| status.branch.clone());
        let detached = raw_detached && rebase_branch.is_none();
        let (effective_upstream, effective_divergence) =
            if status.upstream.is_none() && rebase_branch.is_some() {
                self.resolve_branch_upstream_and_divergence(root, &display_branch)
                    .await
            } else {
                (status.upstream, status.divergence)
            };
        let remote = match (effective_upstream, effective_divergence) {
            (Some(branch), Some((ahead, behind))) => {
                let remote_name = branch.split('/').next().unwrap_or("");
                let verified = self.is_remote_verified(repository_id, remote_name);
                Remote::Tracked {
                    branch,
                    ahead,
                    behind,
                    local_only: !verified,
                }
            }
            (Some(_), None) => Remote::Error {
                reason: "本地找不到有效的跟踪引用，无法确认领先 / 落后；尚未联网刷新".into(),
            },
            (None, _) => {
                let reason = if detached {
                    "当前为分离 HEAD，未确定跟踪分支".to_owned()
                } else {
                    let remotes = self.list_remotes_unlocked(root).await.unwrap_or_default();
                    if remotes.is_empty() {
                        "尚未配置 Git 远端与跟踪分支".to_owned()
                    } else {
                        "尚未配置跟踪远端分支".to_owned()
                    }
                };
                Remote::Missing { reason }
            }
        };
        let branch_state = self.branch_state(root).await?;
        if branch_state.status != before {
            return Err("Git 状态在读取期间发生变化，请重新刷新；当前草稿与选择已保留".into());
        }
        let worktree_id = identity("wt", root);
        let commit_state = commit::CommitRegistration::build(
            repository_id,
            &worktree_id,
            &status.branch,
            raw_detached,
            status.head.as_deref(),
            &status.entries,
            &mut files,
            &working_hashes,
            &branch_state.index,
        )?;
        let version = serde_json::to_vec(&(status.head.as_deref(), &display_branch, &files))
            .map_err(|error| error.to_string())?;
        Ok((
            WorktreeSnapshot {
                repository_id: repository_id.into(),
                worktree_id,
                label: label(root),
                path: display(root)?,
                branch: display_branch,
                detached,
                head: status.head,
                status_version: hash(version),
                branch_token: Some(branch_state.token()?),
                read_error: None,
                remote,
                files,
            },
            commit_state,
        ))
    }
    fn working_file(&self, root: &Path, path: &str) -> Result<String> {
        parse::relative(path)?;
        let path = root.join(path);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(hash(b"missing\0"));
            }
            Err(_) => return Err("无法读取变更文件，当前草稿与选择已保留".into()),
        };
        let parent = fs::canonicalize(path.parent().ok_or("文件缺少父目录")?)
            .map_err(|_| "无法读取文件所在目录")?;
        if !parent.starts_with(root) {
            return Err("文件父目录指向工作树之外，已停止读取".into());
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).map_err(|_| "无法读取符号链接")?;
            let mut payload = b"symlink\0".to_vec();
            payload.extend_from_slice(target.as_os_str().as_encoded_bytes());
            return Ok(hash(payload));
        }
        if !metadata.is_file() {
            return Err("变更包含不支持的特殊文件，已停止读取".into());
        }
        let mut file = File::open(&path).map_err(|_| "无法打开变更文件")?;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let opened = file.metadata().map_err(|_| "无法核对变更文件")?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() || !opened.is_file() {
            return Err("文件在打开时被替换，请重新刷新".into());
        }
        let mut digest = Sha256::new();
        digest.update(if opened.permissions().mode() & 0o111 != 0 {
            b"file:755\0"
        } else {
            b"file:644\0"
        });
        let mut buffer = [0; 64 * 1024];
        loop {
            if self.inner.reader.cancelled() {
                return Err("应用正在退出，已停止 Git 读取".into());
            }
            let count = file.read(&mut buffer).map_err(|_| "变更文件读取失败")?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }
}
fn staged(file: &StatusEntry) -> bool {
    !file.conflicted
        && file.xy != "??"
        && file.xy.as_bytes().first().is_some_and(|byte| *byte != b'.')
}

pub fn validate_request_id(request_id: &str) -> Result<()> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("无效的仓库读取请求身份".into());
    }
    Ok(())
}
