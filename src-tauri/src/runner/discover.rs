//! Read-only project manifest pre-scan and Pi Core AI command discovery (`discover_run_commands`).

use super::{
    manager::RunProcessManager,
    types::{
        DiscoverRunCommandsProgressEvent, DiscoverRunCommandsReply, DiscoverRunCommandsRequest,
        DiscoveredRunCommand, MAX_RUN_COMMANDS_PER_REPO, RUN_PROTOCOL_VERSION, normalize_run_cwd,
        normalize_run_group, normalize_run_icon,
    },
    workspace_scan::scan_workspace_modules,
};
use crate::{
    error::{AppError, AppErrorCode},
    i18n,
    models::store::ModelStore,
    pi_runtime::{
        PiCommand, PiPaths, PiSupervisor, PiTaskOutcome, PiToolExecutionResult, RequestContext,
    },
    repository::{self, RepositoryStore},
};
use std::{collections::HashSet, fs, path::Path, sync::Arc, time::Duration};

const DISCOVER_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_SCAN_ENTRIES: usize = 60;

const KNOWN_MANIFEST_FILES: &[&str] = &[
    "package.json",
    "pnpm-workspace.yaml",
    "turbo.json",
    "nx.json",
    "Cargo.toml",
    "Makefile",
    "makefile",
    "justfile",
    "Justfile",
    "pyproject.toml",
    "requirements.txt",
    "manage.py",
    "go.mod",
    "go.work",
    "pom.xml",
    "mvnw",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "gradlew",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    "Procfile",
    "README.md",
];

const IGNORED_SUBDIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    "vendor",
    ".idea",
    ".vscode",
    "coverage",
];

/// Fast, bounded, read-only scan of top-level and 1-level-deep project manifests and `scripts/`
/// filenames so the discovery model knows immediately which files to inspect via `read_repo_file`.
pub fn scan_project_manifest_paths(worktree_root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(root_entries) = fs::read_dir(worktree_root) else {
        return found;
    };
    let mut subdirs = Vec::new();
    for entry in root_entries.filter_map(Result::ok) {
        if found.len() >= MAX_SCAN_ENTRIES {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".env") {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_file() {
            if KNOWN_MANIFEST_FILES.contains(&name.as_str()) {
                found.push(name);
            }
        } else if file_type.is_dir()
            && !IGNORED_SUBDIRS.contains(&name.as_str())
            && !name.starts_with('.')
        {
            subdirs.push((name, entry.path()));
        }
    }

    subdirs.sort_by(|a, b| a.0.cmp(&b.0));

    // Include `scripts/` filenames (without reading sensitive files) and 1-level monorepo manifests:
    for (dir_name, dir_path) in subdirs {
        if found.len() >= MAX_SCAN_ENTRIES {
            break;
        }
        let Ok(children) = fs::read_dir(&dir_path) else {
            continue;
        };
        if dir_name == "scripts" || dir_name == "bin" {
            let mut names: Vec<String> = children
                .filter_map(Result::ok)
                .filter(|c| c.file_type().is_ok_and(|t| t.is_file()))
                .map(|c| c.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.') && !n.starts_with(".env"))
                .collect();
            names.sort();
            for script in names.into_iter().take(12) {
                if found.len() >= MAX_SCAN_ENTRIES {
                    break;
                }
                found.push(format!("{dir_name}/{script}"));
            }
            continue;
        }
        let mut child_files = Vec::new();
        let mut nested_dirs = Vec::new();
        for child in children.filter_map(Result::ok) {
            let child_name = child.file_name().to_string_lossy().into_owned();
            if child_name.starts_with('.') || IGNORED_SUBDIRS.contains(&child_name.as_str()) {
                continue;
            }
            let Ok(ft) = child.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_file() && KNOWN_MANIFEST_FILES.contains(&child_name.as_str()) {
                child_files.push(format!("{dir_name}/{child_name}"));
            } else if ft.is_dir() && matches!(dir_name.as_str(), "apps" | "packages" | "services") {
                nested_dirs.push((format!("{dir_name}/{child_name}"), child.path()));
            }
        }
        child_files.sort();
        for rel in child_files {
            if found.len() >= MAX_SCAN_ENTRIES {
                break;
            }
            found.push(rel);
        }
        nested_dirs.sort_by(|a, b| a.0.cmp(&b.0));
        for (nested_rel, nested_path) in nested_dirs.into_iter().take(8) {
            if found.len() >= MAX_SCAN_ENTRIES {
                break;
            }
            let Ok(grand) = fs::read_dir(&nested_path) else {
                continue;
            };
            let mut grand_files: Vec<String> = grand
                .filter_map(Result::ok)
                .filter(|g| g.file_type().is_ok_and(|t| t.is_file()))
                .map(|g| g.file_name().to_string_lossy().into_owned())
                .filter(|n| KNOWN_MANIFEST_FILES.contains(&n.as_str()))
                .map(|n| format!("{nested_rel}/{n}"))
                .collect();
            grand_files.sort();
            for rel in grand_files {
                if found.len() >= MAX_SCAN_ENTRIES {
                    break;
                }
                found.push(rel);
            }
        }
    }

    found.sort();
    found.dedup();
    found
}

struct DiscoverCancelCleanup<'a> {
    manager: &'a RunProcessManager,
    supervisor: &'a PiSupervisor,
    request_id: String,
}

impl Drop for DiscoverCancelCleanup<'_> {
    fn drop(&mut self) {
        self.manager.clear_discover_cancelled(&self.request_id);
        self.supervisor.clear_pre_cancelled(&self.request_id);
    }
}

fn cancelled_discover_reply<F>(request_id: &str, progress_cb: &Arc<F>) -> DiscoverRunCommandsReply
where
    F: Fn(DiscoverRunCommandsProgressEvent) + Send + Sync + 'static,
{
    progress_cb(DiscoverRunCommandsProgressEvent {
        protocol_version: RUN_PROTOCOL_VERSION,
        request_id: request_id.to_owned(),
        status: "cancelled",
        status_label: String::new(),
    });
    DiscoverRunCommandsReply {
        protocol_version: RUN_PROTOCOL_VERSION,
        kind: "discoverRunCommands",
        request_id: request_id.to_owned(),
        status: "cancelled",
        summary: String::new(),
        commands: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn discover_run_commands<F>(
    storage_dir: &Path,
    paths: &PiPaths,
    supervisor: &PiSupervisor,
    manager: &RunProcessManager,
    models: &ModelStore,
    store: &RepositoryStore,
    request: &DiscoverRunCommandsRequest,
    on_progress: F,
) -> Result<DiscoverRunCommandsReply, AppError>
where
    F: Fn(DiscoverRunCommandsProgressEvent) + Send + Sync + 'static,
{
    let locale = request.locale;
    repository::validate_request_id_with_locale(&request.request_id, locale)
        .map_err(|msg| AppError::runner(AppErrorCode::RunnerInvalidInput, msg))?;
    let _cancel_cleanup = DiscoverCancelCleanup {
        manager,
        supervisor,
        request_id: request.request_id.clone(),
    };
    let progress_cb = Arc::new(on_progress);
    if manager.is_discover_cancelled(&request.request_id) {
        return Ok(cancelled_discover_reply(&request.request_id, &progress_cb));
    }

    let worktree_root = store
        .worktree_root_for_target(&request.target, locale)
        .await
        .map_err(|msg| AppError::repository(AppErrorCode::RepoInvalidTarget, msg))?;
    let spec = models
        .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
        .await?;
    if manager.is_discover_cancelled(&request.request_id) {
        return Ok(cancelled_discover_reply(&request.request_id, &progress_cb));
    }
    let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());

    progress_cb(DiscoverRunCommandsProgressEvent {
        protocol_version: RUN_PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        status: "running",
        status_label: i18n::runner::discover_scanning_label(locale).into(),
    });

    let scan_root = worktree_root.clone();
    let (project_files, workspace_modules) = tokio::task::spawn_blocking(move || {
        (
            scan_project_manifest_paths(&scan_root),
            scan_workspace_modules(&scan_root),
        )
    })
    .await
    .map_err(|err| {
        AppError::runner(
            AppErrorCode::RunnerFailed,
            format!("Workspace scan task failed: {err}"),
        )
    })?;
    if manager.is_discover_cancelled(&request.request_id) {
        return Ok(cancelled_discover_reply(&request.request_id, &progress_cb));
    }

    let repository_name = store
        .cached_snapshots_in_order()
        .into_iter()
        .find(|s| s.repository.id == request.target.repository_id)
        .map(|s| s.repository.name)
        .unwrap_or_else(|| {
            worktree_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("repository")
                .to_owned()
        });

    progress_cb(DiscoverRunCommandsProgressEvent {
        protocol_version: RUN_PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        status: "running",
        status_label: i18n::runner::discover_analyzing_label(locale).into(),
    });

    let context = RequestContext {
        repository_id: request.target.repository_id.clone(),
        worktree_id: request.target.worktree_id.clone(),
        session_id: request.target.session_id.clone(),
        scope_version: request.target.scope_version.clone(),
    };
    let command = PiCommand::DiscoverRunCommands {
        request_id: request.request_id.clone(),
        context: context.clone(),
        model: spec.to_pi_spec(),
        repository_name,
        project_files,
        workspace_modules,
        existing_commands: request.existing_commands.clone(),
        locale,
    };

    let store_clone = store.clone();
    let target_clone = request.target.clone();
    let cb_clone = Arc::clone(&progress_cb);
    let req_id_clone = request.request_id.clone();

    let outcome = supervisor
        .run_task_with_tools(
            paths,
            request.request_id.clone(),
            context,
            &command,
            DISCOVER_IDLE_TIMEOUT,
            api_key_for_redaction.as_deref(),
            |_event| Ok(()),
            move |_call_id, tool_name, args| {
                let store = store_clone.clone();
                let target = target_clone.clone();
                let cb = Arc::clone(&cb_clone);
                let req_id = req_id_clone.clone();
                async move {
                    match tool_name.as_str() {
                        "read_repo_file" | "search_repo" => {
                            if let Some(path) = args.get("path").and_then(serde_json::Value::as_str)
                            {
                                let label = i18n::runner::discover_reading_file_label(locale, path);
                                cb(DiscoverRunCommandsProgressEvent {
                                    protocol_version: RUN_PROTOCOL_VERSION,
                                    request_id: req_id,
                                    status: "running",
                                    status_label: label,
                                });
                            }
                            match store
                                .execute_read_tool(&target, &tool_name, &args, locale)
                                .await
                            {
                                Ok(content) => PiToolExecutionResult::success(content),
                                Err(err) => PiToolExecutionResult::error(err),
                            }
                        }
                        other => PiToolExecutionResult::error(format!(
                            "Tool `{other}` is not allowed during read-only run command discovery."
                        )),
                    }
                }
            },
        )
        .await;

    match outcome {
        Ok(PiTaskOutcome::RunCommandsDiscovered { summary, commands }) => {
            let mut seen = HashSet::new();
            let mut clean_commands = Vec::new();
            for item in commands {
                let label = item.label.trim().to_owned();
                let cmd = item.command.replace("\r\n", "\n").trim().to_owned();
                if label.is_empty() || cmd.is_empty() || label.contains('\0') || cmd.contains('\0')
                {
                    continue;
                }
                let Ok(cwd) = normalize_run_cwd(item.cwd.as_deref()) else {
                    continue;
                };
                let group = normalize_run_group(item.group.as_deref()).ok().flatten();
                let icon = normalize_run_icon(item.icon.as_deref());
                let dedup_key = format!("{}::{}", cwd.as_deref().unwrap_or("."), cmd);
                if !seen.insert(dedup_key) {
                    continue;
                }
                let description: String = item
                    .description
                    .replace('\0', "")
                    .trim()
                    .chars()
                    .take(200)
                    .collect();
                let category_raw = item.category.replace('\0', "");
                let category = if category_raw.trim().is_empty() {
                    "dev".into()
                } else {
                    category_raw.trim().chars().take(24).collect()
                };
                clean_commands.push(DiscoveredRunCommand {
                    label: label.chars().take(48).collect(),
                    command: cmd.chars().take(1_000).collect(),
                    cwd,
                    group,
                    icon,
                    description,
                    category,
                });
                if clean_commands.len() >= MAX_RUN_COMMANDS_PER_REPO {
                    break;
                }
            }
            let final_summary = if summary.trim().is_empty() {
                i18n::runner::discover_completed_summary(locale, clean_commands.len())
            } else {
                summary.trim().to_owned()
            };
            progress_cb(DiscoverRunCommandsProgressEvent {
                protocol_version: RUN_PROTOCOL_VERSION,
                request_id: request.request_id.clone(),
                status: "done",
                status_label: final_summary.clone(),
            });
            Ok(DiscoverRunCommandsReply {
                protocol_version: RUN_PROTOCOL_VERSION,
                kind: "discoverRunCommands",
                request_id: request.request_id.clone(),
                status: "done",
                summary: final_summary,
                commands: clean_commands,
            })
        }
        Ok(PiTaskOutcome::Cancelled) => {
            Ok(cancelled_discover_reply(&request.request_id, &progress_cb))
        }
        Ok(other) => Err(AppError::model(
            AppErrorCode::PiRuntimeError,
            format!("Unexpected Pi outcome for discover_run_commands: {other:?}"),
        )),
        Err(err) => {
            progress_cb(DiscoverRunCommandsProgressEvent {
                protocol_version: RUN_PROTOCOL_VERSION,
                request_id: request.request_id.clone(),
                status: "failed",
                status_label: err.message.clone(),
            });
            Err(err)
        }
    }
}
