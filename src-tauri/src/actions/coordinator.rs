//! Phase 2 Custom Action & `Create with Pi` coordinator:
//! manages same-repo write exclusion, real-time bidirectional `ask_user_question` oneshot
//! round-trips, Rust-verified `install_platform_cli` grants, 3-tier controlled CLI policy
//! enforcement with Tier-2 preview confirmation + "don't ask again" prompt-hash binding, and
//! direct reuse of `RepositoryStore::commit` / `push_branch` / `sync_rebase_onto_upstream`.

use super::{
    cli_env::{check_cli_env, detect_repo_remotes, install_platform_cli},
    executor::{exec_controlled_git, exec_controlled_platform_cli},
    policy::{CommandDecision, classify_git_command, classify_platform_cli_command},
    session_store::{
        self, ActionSessionSnapshot, PersistedActionSession, SubmitGroupCheckpoint,
        SubmitPromotionCheckpoint, SubmitSessionLedger,
    },
    submit_cleanup::cleanup_verified_submit_duplicates,
    submit_worktree::{
        commit_in_isolated_worktree, create_isolated_submit_worktree, parse_worktree_list,
        remove_verified_submit_worktree,
    },
    types::*,
};
use crate::{
    cpush::{CPushCoordinator, types::CPushLandedCommit},
    foundation::pi::{
        PiCommand, PiPaths, PiPlanFileEntry, PiSupervisor, PiTaskOutcome, PiToolExecutionResult,
        RequestContext,
    },
    i18n::{self, LanguageKind},
    models::store::ModelStore,
    repository::{
        self, RepositoryStore,
        remote::{PushOutcome, RebaseSyncOutcome},
        types::{CommitRequest, CommitSelection},
    },
    skills::RuntimeSkillSnapshot,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

const CUSTOM_ACTION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Default)]
struct SessionAuthGrants {
    authorized_installs: HashSet<String>,
}

struct PendingQuestion {
    sender: oneshot::Sender<AskUserQuestionResponse>,
    grants: Arc<Mutex<SessionAuthGrants>>,
    install_options: HashMap<String, String>,
}

#[derive(Default)]
struct CoordinatorInner {
    active_operations: AtomicUsize,
    active_write_repos: Mutex<HashSet<String>>,
    pending_questions: Mutex<HashMap<(String, String), PendingQuestion>>,
    override_cli_dirs: Mutex<Option<Vec<PathBuf>>>,
    seq: AtomicU64,
}

#[derive(Clone, Default)]
pub struct CustomActionCoordinator {
    inner: Arc<CoordinatorInner>,
}

struct ActiveWriteRepoGuard {
    inner: Arc<CoordinatorInner>,
    repository_id: String,
}

impl Drop for ActiveWriteRepoGuard {
    fn drop(&mut self) {
        self.inner.active_operations.fetch_sub(1, Ordering::SeqCst);
        if let Ok(mut repos) = self.inner.active_write_repos.lock() {
            repos.remove(&self.repository_id);
        }
    }
}

struct PendingQuestionGuard {
    inner: Arc<CoordinatorInner>,
    key: (String, String),
}

impl Drop for PendingQuestionGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.inner.pending_questions.lock() {
            map.remove(&self.key);
        }
    }
}

#[derive(Default)]
struct ActionRunState {
    timeline: Vec<CustomActionTimelineItem>,
    commits: Vec<CPushLandedCommit>,
    links: Vec<CustomActionLink>,
    committed_file_ids: HashSet<String>,
    skip_write_confirm: bool,
    trusted_action_update: Option<TrustedActionUpdate>,
    wrote_repo: bool,
    submit_ledger: SubmitSessionLedger,
}

fn find_group_for_worktree_or_id(
    storage_dir: &Path,
    repository_id: &str,
    ledger: &SubmitSessionLedger,
    group_id: Option<&str>,
    worktree_path: Option<&str>,
) -> Option<SubmitGroupCheckpoint> {
    if let Some(wt) = worktree_path {
        let canon = std::fs::canonicalize(wt).unwrap_or_else(|_| PathBuf::from(wt));
        if let Some(group) = ledger.groups.iter().find(|g| {
            g.worktree_path.as_deref().is_some_and(|p| {
                std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)) == canon
            })
        }) {
            return Some(group.clone());
        }
        if let Some((_, group)) =
            session_store::find_owned_worktree_group(storage_dir, repository_id, Path::new(wt))
        {
            return Some(group);
        }
    }
    if let Some(gid) = group_id {
        if let Some(group) = ledger.groups.iter().find(|g| g.group_id == gid) {
            return Some(group.clone());
        }
        for session in session_store::load_all_sessions(storage_dir) {
            if session.repository_id == repository_id
                && let Some(group) = session
                    .submit_ledger
                    .groups
                    .into_iter()
                    .find(|g| g.group_id == gid)
            {
                return Some(group);
            }
        }
    }
    None
}

fn resolve_authorized_git_cwd(
    storage_dir: &Path,
    repository_id: &str,
    active_root: &Path,
    ledger: &SubmitSessionLedger,
    requested_worktree: Option<&str>,
) -> Result<PathBuf, String> {
    let Some(wt_raw) = requested_worktree.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(active_root.to_path_buf());
    };
    let wt_path = PathBuf::from(wt_raw);
    if !wt_path.exists() {
        return Err(format!("Requested worktreePath does not exist: `{wt_raw}`"));
    }
    let canon_root =
        std::fs::canonicalize(active_root).unwrap_or_else(|_| active_root.to_path_buf());
    let canon_wt = std::fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
    if canon_wt == canon_root {
        return Ok(canon_wt);
    }
    let owned_in_run = ledger.groups.iter().any(|g| {
        g.worktree_path.as_deref().is_some_and(|p| {
            std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)) == canon_wt
        })
    });
    if owned_in_run
        || session_store::find_owned_worktree_group(storage_dir, repository_id, &canon_wt).is_some()
    {
        return Ok(canon_wt);
    }
    Err(format!(
        "Refused `run_git` outside the active repository worktree or Picket-owned isolated submit worktrees: `{wt_raw}`"
    ))
}

fn detect_install_option_tool(option: &AskUserQuestionOption) -> Option<&'static str> {
    let id_lower = option.id.to_ascii_lowercase();
    let preview_lower = option.preview.as_deref().unwrap_or("").to_ascii_lowercase();
    let label_lower = option.label.to_ascii_lowercase();
    let zh_install = i18n::action_install_label_keyword_zh();
    if id_lower.contains("install-gh")
        || id_lower == "install_gh"
        || preview_lower.contains("brew install gh")
        || (label_lower.contains("install")
            && label_lower.contains("gh")
            && !label_lower.contains("glab"))
        || (option.label.contains(zh_install)
            && label_lower.contains("gh")
            && !label_lower.contains("glab"))
    {
        return Some("gh");
    }
    if id_lower.contains("install-glab")
        || id_lower == "install_glab"
        || preview_lower.contains("brew install glab")
        || (label_lower.contains("install") && label_lower.contains("glab"))
        || (option.label.contains(zh_install) && label_lower.contains("glab"))
    {
        return Some("glab");
    }
    None
}

fn build_tier2_confirm_questionnaire(
    locale: LanguageKind,
    reason: &str,
    preview: &str,
    allow_dont_ask_again: bool,
    action_label: &str,
) -> AskUserQuestionRequest {
    AskUserQuestionRequest {
        questions: vec![AskUserQuestionItem {
            id: "confirm-write".into(),
            header: i18n::action_tier2_header(locale).into(),
            question: i18n::action_tier2_question(locale, reason),
            multi_select: None,
            options: vec![
                AskUserQuestionOption {
                    id: "approve-write".into(),
                    label: i18n::action_tier2_approve_label(locale).into(),
                    description: i18n::action_tier2_approve_desc(locale).into(),
                    recommended: Some(true),
                    preview: Some(preview.to_owned()),
                },
                AskUserQuestionOption {
                    id: "cancel-write".into(),
                    label: i18n::action_tier2_cancel_label(locale).into(),
                    description: i18n::action_tier2_cancel_desc(locale).into(),
                    recommended: None,
                    preview: None,
                },
            ],
        }],
        context: Some(AskUserQuestionContext {
            scope: Some(action_label.to_owned()),
            affected_file_ids: None,
            links: None,
            allow_dont_ask_again: allow_dont_ask_again.then_some(true),
            dont_ask_again_label: allow_dont_ask_again
                .then(|| i18n::action_tier2_dont_ask_label(locale).into()),
        }),
    }
}

fn push_timeline_step(timeline: &mut Vec<CustomActionTimelineItem>, state: &str, label: String) {
    if let Some(last) = timeline.last_mut()
        && last.state == "active"
        && state != "active"
    {
        last.state = state.to_owned();
        last.label = label;
        return;
    }
    if state == "active" {
        for item in timeline.iter_mut() {
            if item.state == "active" {
                item.state = "done".into();
            }
        }
    }
    if timeline.len() >= 12 {
        timeline.remove(0);
    }
    timeline.push(CustomActionTimelineItem {
        state: state.to_owned(),
        label,
    });
}

fn append_unique_links(target: &mut Vec<CustomActionLink>, incoming: Vec<CustomActionLink>) {
    for link in incoming {
        if !target.iter().any(|existing| existing.url == link.url) {
            target.push(link);
        }
    }
}

fn format_tool_status_label(locale: LanguageKind, tool_name: &str, args: &Value) -> String {
    match tool_name {
        "install_platform_cli" => {
            let tool = args.get("tool").and_then(Value::as_str).unwrap_or("CLI");
            i18n::action_tool_status_label(locale, tool_name, tool)
        }
        "run_git" => {
            let sub = args
                .get("args")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .unwrap_or("command");
            i18n::action_tool_status_label(locale, tool_name, sub)
        }
        "run_platform_cli" => {
            let cli = args.get("cli").and_then(Value::as_str).unwrap_or("cli");
            let sub = args
                .get("args")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            let detail = format!("{cli} {sub}");
            i18n::action_tool_status_label(locale, tool_name, detail.trim())
        }
        "read_repo_file" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("file");
            i18n::action_tool_status_label(locale, tool_name, path)
        }
        "manage_submit_worktree" => {
            let op = args.get("op").and_then(Value::as_str).unwrap_or("worktree");
            i18n::action_tool_status_label(locale, tool_name, op)
        }
        _ => i18n::action_tool_status_label(locale, tool_name, ""),
    }
}

impl CustomActionCoordinator {
    pub fn has_active_operation(&self) -> bool {
        self.inner.active_operations.load(Ordering::SeqCst) > 0
    }

    pub fn has_active_operation_for_repo(&self, repository_id: &str) -> bool {
        self.inner
            .active_write_repos
            .lock()
            .map(|set| set.contains(repository_id))
            .unwrap_or(false)
    }

    fn override_cli_dirs(&self) -> Option<Vec<PathBuf>> {
        self.inner
            .override_cli_dirs
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    #[cfg(test)]
    pub(crate) fn set_override_cli_dirs(&self, dirs: Option<Vec<PathBuf>>) {
        if let Ok(mut guard) = self.inner.override_cli_dirs.lock() {
            *guard = dirs;
        }
    }

    pub fn restore_sessions(
        &self,
        storage_dir: &Path,
        locale: LanguageKind,
    ) -> Vec<ActionSessionSnapshot> {
        session_store::reconcile_sessions_on_startup(storage_dir, locale)
    }

    pub fn dismiss_done_notice(
        &self,
        storage_dir: &Path,
        request: &DismissCustomActionDoneRequest,
    ) -> Result<bool, String> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
        session_store::dismiss_session_message(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            request.run_id.as_deref(),
            &request.message_id,
        )?;
        Ok(true)
    }

    fn acquire_repo_write(
        &self,
        repository_id: &str,
        locale: LanguageKind,
    ) -> Result<ActiveWriteRepoGuard, String> {
        let mut repos = self
            .inner
            .active_write_repos
            .lock()
            .map_err(|_| "Custom action registry is unavailable")?;
        if repos.contains(repository_id) {
            return Err(i18n::action_already_running_error(locale).into());
        }
        repos.insert(repository_id.to_owned());
        self.inner.active_operations.fetch_add(1, Ordering::SeqCst);
        Ok(ActiveWriteRepoGuard {
            inner: Arc::clone(&self.inner),
            repository_id: repository_id.to_owned(),
        })
    }

    pub fn respond_question(&self, request: &RespondCustomActionRequest) -> Result<bool, String> {
        repository::validate_request_id_with_locale(&request.request_id, request.locale)?;
        let key = (request.run_id.clone(), request.call_id.clone());
        let entry = {
            let mut map = self
                .inner
                .pending_questions
                .lock()
                .map_err(|_| "Question registry is unavailable")?;
            map.remove(&key)
        };
        let Some(pending) = entry else {
            return Err(i18n::action_question_inactive_error(request.locale).into());
        };

        // Hard-record any sensitive install authorization based strictly on the real optionId clicked by the user.
        if !request.response.cancelled
            && let Ok(mut grants) = pending.grants.lock()
        {
            for answer in &request.response.answers {
                for opt_id in &answer.selected_option_ids {
                    if let Some(tool) = pending.install_options.get(opt_id) {
                        grants.authorized_installs.insert(tool.clone());
                    }
                }
            }
        }

        pending
            .sender
            .send(request.response.clone())
            .map_err(|_| i18n::action_question_settled_error(request.locale).to_owned())?;
        Ok(true)
    }

    async fn suspend_for_question(
        &self,
        run_id: &str,
        call_id: &str,
        request: &AskUserQuestionRequest,
        grants: Arc<Mutex<SessionAuthGrants>>,
    ) -> Result<AskUserQuestionResponse, String> {
        let mut install_options = HashMap::new();
        for question in &request.questions {
            for option in &question.options {
                if let Some(tool) = detect_install_option_tool(option) {
                    install_options.insert(option.id.clone(), tool.to_owned());
                }
            }
        }
        let (tx, rx) = oneshot::channel();
        let key = (run_id.to_owned(), call_id.to_owned());
        {
            let mut map = self
                .inner
                .pending_questions
                .lock()
                .map_err(|_| "Question registry is unavailable")?;
            map.insert(
                key.clone(),
                PendingQuestion {
                    sender: tx,
                    grants,
                    install_options,
                },
            );
        }
        let _cleanup = PendingQuestionGuard {
            inner: Arc::clone(&self.inner),
            key,
        };
        rx.await
            .map_err(|_| "Interactive question channel closed before a response arrived.".into())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_custom_action<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        store: &RepositoryStore,
        cpush: &CPushCoordinator,
        request: &StartCustomActionRequest,
        on_progress: F,
    ) -> Result<CustomActionCommandReply, String>
    where
        F: Fn(CustomActionProgressEvent) + Send + Sync + 'static,
    {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let action_label = request.action.label.trim().to_owned();
        let action_prompt = request
            .action
            .prompt
            .replace("\r\n", "\n")
            .trim()
            .to_owned();
        if action_label.is_empty() || action_prompt.is_empty() {
            return Err(i18n::action_empty_fields_error(locale).into());
        }
        if cpush.has_active_operation_for_repo(&request.target.repository_id) {
            return Err(i18n::action_cpush_active_error(locale).into());
        }
        let _repo_guard = self.acquire_repo_write(&request.target.repository_id, locale)?;

        let run_id = request.request_id.clone();
        let worktree_root = store
            .worktree_root_for_target(&request.target, locale)
            .await?;
        let live_snapshot = store.refresh_with_locale(&request.target, locale).await?;
        let active_tree = live_snapshot
            .worktrees
            .iter()
            .find(|wt| wt.worktree_id == request.target.worktree_id)
            .ok_or_else(|| "Target worktree not found in live repository snapshot".to_string())?;

        // Build optional authorized selected file context without requiring any selection.
        let mut selected_files: Vec<PiPlanFileEntry> = Vec::new();
        let mut unselected_paths: Vec<String> = Vec::new();
        if !request.selections.is_empty() {
            if let Ok(scope) = store
                .extract_cpush_scope(&request.target, &request.selections, locale)
                .await
            {
                selected_files = scope
                    .files
                    .into_iter()
                    .map(|f| PiPlanFileEntry {
                        file_id: f.file_id,
                        path: f.path,
                        previous_path: f.previous_path,
                        status: f.status,
                        range_kind: f.range_kind,
                        added: f.added,
                        removed: f.removed,
                        ignored_for_review: f.ignored_for_review,
                        binary: f.binary,
                        patch: f.patch,
                        skip_reason: f.skip_reason,
                    })
                    .collect();
                unselected_paths = scope.unselected_paths;
            }
        } else {
            unselected_paths = active_tree.files.iter().map(|f| f.path.clone()).collect();
        }

        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());
        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));

        let expected_hash = compute_prompt_hash(&action_prompt);
        let initially_trusted = request.action.skip_write_confirm == Some(true)
            && request.action.trusted_prompt_hash.as_deref() == Some(expected_hash.as_str());

        let prior_session = session_store::find_latest_resumable_session(
            storage_dir,
            &request.target.repository_id,
            &request.target.worktree_id,
            Some(&request.action.id),
        )
        .or_else(|| {
            session_store::find_latest_resumable_session(
                storage_dir,
                &request.target.repository_id,
                &request.target.worktree_id,
                None,
            )
        })
        .filter(|prior| {
            matches!(
                prior.status.as_str(),
                "interrupted" | "failed" | "cancelled"
            ) || (prior.status == "done"
                && (active_tree.files.is_empty()
                    || prior
                        .submit_ledger
                        .groups
                        .iter()
                        .any(|g| g.worktree_path.is_some() && !g.worktree_removed)))
        });
        let previous_checkpoint = prior_session
            .as_ref()
            .map(session_store::format_resume_checkpoint_for_prompt);
        let initial_ledger = prior_session
            .as_ref()
            .map(|s| s.submit_ledger.clone())
            .unwrap_or_default();

        let run_state = Arc::new(Mutex::new(ActionRunState {
            timeline: vec![CustomActionTimelineItem {
                state: "active".into(),
                label: i18n::action_preparing_label(locale, &action_label),
            }],
            commits: Vec::new(),
            links: Vec::new(),
            committed_file_ids: HashSet::new(),
            skip_write_confirm: initially_trusted,
            trusted_action_update: None,
            wrote_repo: false,
            submit_ledger: initial_ledger,
        }));
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let progress_cb = Arc::new(on_progress);
        let remotes = Arc::new(detect_repo_remotes(&worktree_root).await);
        let frozen_selections = Arc::new(request.selections.clone());
        let live_changes = Arc::new(active_tree.files.clone());
        let storage_dir_buf = Arc::new(storage_dir.to_path_buf());
        let target_clone = request.target.clone();
        let action_id = request.action.id.clone();

        let persist_state = Arc::new({
            let run_state = Arc::clone(&run_state);
            let storage_dir = Arc::clone(&storage_dir_buf);
            let run_id = run_id.clone();
            let repo_id = request.target.repository_id.clone();
            let wt_id = request.target.worktree_id.clone();
            let action_id = action_id.clone();
            let action_label = action_label.clone();
            let prompt_hash = expected_hash.clone();
            move |status: &str, summary: String| {
                if let Ok(st) = run_state.lock() {
                    let record = PersistedActionSession {
                        run_id: run_id.clone(),
                        repository_id: repo_id.clone(),
                        worktree_id: wt_id.clone(),
                        action_id: action_id.clone(),
                        action_label: action_label.clone(),
                        action_prompt_hash: prompt_hash.clone(),
                        status: status.to_owned(),
                        updated_at_ms: session_store::now_millis(),
                        summary,
                        dismissed_message_id: None,
                        timeline: st.timeline.clone(),
                        commits: st.commits.clone(),
                        links: st.links.clone(),
                        submit_ledger: st.submit_ledger.clone(),
                    };
                    let _ = session_store::upsert_session(&storage_dir, record);
                }
            }
        });

        let emit_progress = {
            let run_state = Arc::clone(&run_state);
            let progress_cb = Arc::clone(&progress_cb);
            let run_id = run_id.clone();
            let repo_id = request.target.repository_id.clone();
            let wt_id = request.target.worktree_id.clone();
            let session_id = request.target.session_id.clone();
            let action_id = action_id.clone();
            let action_label = action_label.clone();
            move |status: &'static str,
                  pending_call_id: Option<String>,
                  message: Option<CustomActionUiMessage>,
                  repository_snapshot: Option<crate::repository::types::RepositorySnapshot>| {
                let (timeline, commits, links, trusted) = run_state
                    .lock()
                    .map(|st| {
                        (
                            st.timeline.clone(),
                            st.commits.clone(),
                            st.links.clone(),
                            st.trusted_action_update.clone(),
                        )
                    })
                    .unwrap_or_default();
                progress_cb(CustomActionProgressEvent {
                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                    run_id: run_id.clone(),
                    repository_id: repo_id.clone(),
                    worktree_id: wt_id.clone(),
                    session_id: session_id.clone(),
                    action_id: action_id.clone(),
                    action_label: action_label.clone(),
                    status,
                    pending_call_id,
                    message,
                    timeline,
                    commits,
                    links,
                    trusted_action_update: trusted,
                    repository_snapshot,
                });
            }
        };

        emit_progress(
            "running",
            None,
            Some(CustomActionUiMessage {
                id: format!("progress-{run_id}"),
                kind: "progress".into(),
                title: action_label.clone(),
                description: i18n::action_analyzing_desc(locale).into(),
                scope: None,
                files: None,
                commits: None,
                links: None,
                questionnaire: None,
                progress: Some(CustomActionMessageProgress {
                    completed: 1,
                    total: 3,
                    label: i18n::action_started_label(locale).into(),
                }),
            }),
            None,
        );
        persist_state("running", String::new());

        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let command = PiCommand::RunCustomAction {
            request_id: run_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: active_tree.branch.clone(),
            detached: active_tree.detached,
            remote_branches: live_snapshot
                .repository
                .remote_branches
                .iter()
                .map(|b| b.name.clone())
                .collect(),
            action_label: action_label.clone(),
            action_prompt: action_prompt.clone(),
            extra_prompt: request.extra_prompt.clone(),
            selected_files,
            unselected_paths,
            local_keep_paths: live_snapshot.repository.local_keep_paths.clone(),
            locale,
            commit_language: request.commit_language,
            skill_overrides: skill_snapshot.catalog_option(),
            previous_checkpoint,
        };

        let coordinator_for_tools = self.clone();
        let store_for_tools = store.clone();
        let emit_for_tools = Arc::new(emit_progress);
        let persist_for_tools = Arc::clone(&persist_state);
        let run_state_for_tools = Arc::clone(&run_state);
        let grants_for_tools = Arc::clone(&grants);
        let skill_snapshot_for_tools = Arc::clone(&skill_snapshot);
        let worktree_root_for_tools = worktree_root.clone();
        let storage_dir_for_tools = Arc::clone(&storage_dir_buf);
        let remotes_for_tools = Arc::clone(&remotes);
        let selections_for_tools = Arc::clone(&frozen_selections);
        let live_changes_for_tools = Arc::clone(&live_changes);
        let target_for_tools = target_clone.clone();
        let run_id_for_tools = run_id.clone();
        let action_id_for_tools = action_id.clone();
        let action_label_for_tools = action_label.clone();
        let expected_hash_for_tools = expected_hash.clone();

        let outcome = supervisor
            .run_task_with_tools(
                paths,
                run_id.clone(),
                context,
                &command,
                CUSTOM_ACTION_IDLE_TIMEOUT,
                api_key_for_redaction.as_deref(),
                |_event| Ok(()),
                move |call_id, tool_name, args| {
                    let coordinator = coordinator_for_tools.clone();
                    let store = store_for_tools.clone();
                    let emit = Arc::clone(&emit_for_tools);
                    let persist = Arc::clone(&persist_for_tools);
                    let run_state = Arc::clone(&run_state_for_tools);
                    let grants = Arc::clone(&grants_for_tools);
                    let skills = Arc::clone(&skill_snapshot_for_tools);
                    let root = worktree_root_for_tools.clone();
                    let storage_dir = Arc::clone(&storage_dir_for_tools);
                    let remotes = Arc::clone(&remotes_for_tools);
                    let frozen_selections = Arc::clone(&selections_for_tools);
                    let live_changes = Arc::clone(&live_changes_for_tools);
                    let target = target_for_tools.clone();
                    let run_id = run_id_for_tools.clone();
                    let action_id = action_id_for_tools.clone();
                    let action_label = action_label_for_tools.clone();
                    let expected_hash = expected_hash_for_tools.clone();
                    async move {
                        let status_label = format_tool_status_label(locale, &tool_name, &args);
                        if tool_name != "read_skill_override_doc" && tool_name != "ask_user_question" {
                            if let Ok(mut st) = run_state.lock() {
                                push_timeline_step(&mut st.timeline, "active", status_label.clone());
                            }
                            emit(
                                "running",
                                None,
                                Some(CustomActionUiMessage {
                                    id: format!("progress-{run_id}"),
                                    kind: "progress".into(),
                                    title: action_label.clone(),
                                    description: status_label.clone(),
                                    scope: None,
                                    files: None,
                                    commits: None,
                                    links: None,
                                    questionnaire: None,
                                    progress: Some(CustomActionMessageProgress {
                                        completed: 2,
                                        total: 3,
                                        label: status_label.clone(),
                                    }),
                                }),
                                None,
                            );
                        }

                        let override_dirs = coordinator.override_cli_dirs();

                        match tool_name.as_str() {
                            "read_skill_override_doc" => {
                                let skill = args.get("skill").and_then(Value::as_str).unwrap_or("");
                                let path = args
                                    .get("path")
                                    .and_then(Value::as_str)
                                    .unwrap_or("SKILL.md");
                                match skills.read_doc(skill, path) {
                                    Ok(Some(content)) => PiToolExecutionResult::success(content),
                                    Ok(None) => PiToolExecutionResult::error("Not found in overrides"),
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "read_repo_file" | "search_repo" => {
                                match store.execute_read_tool(&target, &tool_name, &args, locale).await {
                                    Ok(content) => {
                                        if let Ok(mut st) = run_state.lock() {
                                            push_timeline_step(&mut st.timeline, "done", status_label);
                                        }
                                        PiToolExecutionResult::success(content)
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "check_cli_env" => {
                                let snapshot = check_cli_env(&root, override_dirs.as_deref()).await;
                                if let Ok(mut st) = run_state.lock() {
                                    push_timeline_step(&mut st.timeline, "done", status_label);
                                }
                                let json_text = serde_json::to_string_pretty(&snapshot)
                                    .unwrap_or_else(|_| "{}".into());
                                PiToolExecutionResult::success(json_text)
                            }
                            "install_platform_cli" => {
                                let tool = args
                                    .get("tool")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .trim()
                                    .to_owned();
                                let authorized = grants
                                    .lock()
                                    .map(|g| g.authorized_installs.contains(&tool))
                                    .unwrap_or(false);
                                if !authorized {
                                    return PiToolExecutionResult::error(format!(
                                        "Installation of `{tool}` is blocked because the user has not explicitly approved it via `ask_user_question` in this session. Ask the user with an option whose id is `install-{tool}` and preview `brew install {tool}` first."
                                    ));
                                }
                                let emit_stream = Arc::clone(&emit);
                                let run_id_s = run_id.clone();
                                let label_s = action_label.clone();
                                match install_platform_cli(&tool, &root, override_dirs.as_deref(), move |line| {
                                    emit_stream(
                                        "running",
                                        None,
                                        Some(CustomActionUiMessage {
                                            id: format!("progress-{run_id_s}"),
                                            kind: "progress".into(),
                                            title: label_s.clone(),
                                            description: line.clone(),
                                            scope: None,
                                            files: None,
                                            commits: None,
                                            links: None,
                                            questionnaire: None,
                                            progress: Some(CustomActionMessageProgress {
                                                completed: 2,
                                                total: 3,
                                                label: line,
                                            }),
                                        }),
                                        None,
                                    );
                                })
                                .await
                                {
                                    Ok(probe) => {
                                        if let Ok(mut st) = run_state.lock() {
                                            push_timeline_step(
                                                &mut st.timeline,
                                                "done",
                                                i18n::action_installed_cli_label(locale, &tool),
                                            );
                                        }
                                        PiToolExecutionResult::success(
                                            serde_json::to_string_pretty(&probe)
                                                .unwrap_or_else(|_| "Installed".into()),
                                        )
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "ask_user_question" => {
                                let req: AskUserQuestionRequest = match serde_json::from_value(args) {
                                    Ok(parsed) => parsed,
                                    Err(err) => {
                                        return PiToolExecutionResult::error(format!(
                                            "Invalid ask_user_question payload: {err}"
                                        ));
                                    }
                                };
                                let title = req
                                    .questions
                                    .first()
                                    .map(|q| q.header.clone())
                                    .unwrap_or_else(|| action_label.clone());
                                let desc = req
                                    .questions
                                    .first()
                                    .map(|q| q.question.clone())
                                    .unwrap_or_default();
                                emit(
                                    "needsDecision",
                                    Some(call_id.clone()),
                                    Some(CustomActionUiMessage {
                                        id: format!("ask-{run_id}-{call_id}"),
                                        kind: "question".into(),
                                        title,
                                        description: desc.clone(),
                                        scope: req.context.as_ref().and_then(|c| c.scope.clone()),
                                        files: None,
                                        commits: None,
                                        links: req.context.as_ref().and_then(|c| c.links.clone()),
                                        questionnaire: Some(req.clone()),
                                        progress: None,
                                    }),
                                    None,
                                );
                                persist("needsDecision", desc);
                                match coordinator
                                    .suspend_for_question(&run_id, &call_id, &req, Arc::clone(&grants))
                                    .await
                                {
                                    Ok(response) => {
                                        persist("running", String::new());
                                        let resuming_label =
                                            i18n::action_resuming_label(locale).to_owned();
                                        emit(
                                            "running",
                                            None,
                                            Some(CustomActionUiMessage {
                                                id: format!("progress-{run_id}"),
                                                kind: "progress".into(),
                                                title: action_label.clone(),
                                                description: resuming_label.clone(),
                                                scope: None,
                                                files: None,
                                                commits: None,
                                                links: None,
                                                questionnaire: None,
                                                progress: Some(CustomActionMessageProgress {
                                                    completed: 2,
                                                    total: 3,
                                                    label: resuming_label,
                                                }),
                                            }),
                                            None,
                                        );
                                        let details = serde_json::to_value(&response).ok();
                                        let content = serde_json::to_string(&response)
                                            .unwrap_or_else(|_| "{}".into());
                                        PiToolExecutionResult {
                                            ok: true,
                                            content,
                                            details,
                                            is_error: false,
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "run_git" => {
                                let raw_args: Vec<String> = args
                                    .get("args")
                                    .and_then(Value::as_array)
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let requested_worktree =
                                    args.get("worktreePath").and_then(Value::as_str);
                                let effective_cwd = {
                                    let ledger = run_state
                                        .lock()
                                        .map(|st| st.submit_ledger.clone())
                                        .unwrap_or_default();
                                    match resolve_authorized_git_cwd(
                                        &storage_dir,
                                        &target.repository_id,
                                        &root,
                                        &ledger,
                                        requested_worktree,
                                    ) {
                                        Ok(cwd) => cwd,
                                        Err(err) => return PiToolExecutionResult::error(err),
                                    }
                                };
                                let decision = classify_git_command(&raw_args);
                                let is_read_only = match decision {
                                    CommandDecision::DenyRedline { code, reason } => {
                                        return PiToolExecutionResult::error(format!(
                                            "Blocked by Picket hard redline ({code}): {reason}"
                                        ));
                                    }
                                    CommandDecision::AllowTier0 => true,
                                    CommandDecision::AllowTier1 => false,
                                    CommandDecision::RequireConfirmTier2 {
                                        reason,
                                        preview,
                                        allow_dont_ask_again,
                                    } => {
                                        let already_trusted = allow_dont_ask_again
                                            && run_state
                                                .lock()
                                                .map(|st| st.skip_write_confirm)
                                                .unwrap_or(false);
                                        if !already_trusted {
                                            let confirm_call_id = format!("confirm-{call_id}");
                                            let q = build_tier2_confirm_questionnaire(
                                                locale,
                                                &reason,
                                                &preview,
                                                allow_dont_ask_again,
                                                &action_label,
                                            );
                                            emit(
                                                "needsDecision",
                                                Some(confirm_call_id.clone()),
                                                Some(CustomActionUiMessage {
                                                    id: format!("ask-{run_id}-{confirm_call_id}"),
                                                    kind: "question".into(),
                                                    title: q.questions[0].header.clone(),
                                                    description: q.questions[0].question.clone(),
                                                    scope: Some(action_label.clone()),
                                                    files: None,
                                                    commits: None,
                                                    links: None,
                                                    questionnaire: Some(q.clone()),
                                                    progress: None,
                                                }),
                                                None,
                                            );
                                            match coordinator
                                                .suspend_for_question(
                                                    &run_id,
                                                    &confirm_call_id,
                                                    &q,
                                                    Arc::clone(&grants),
                                                )
                                                .await
                                            {
                                                Ok(resp) => {
                                                    let approved = !resp.cancelled
                                                        && resp.answers.iter().any(|ans| {
                                                            ans.selected_option_ids
                                                                .iter()
                                                                .any(|id| id == "approve-write")
                                                        });
                                                    if !approved {
                                                        return PiToolExecutionResult::error(
                                                            "User declined to run the write command.",
                                                        );
                                                    }
                                                    if allow_dont_ask_again
                                                        && resp.dont_ask_again == Some(true)
                                                        && let Ok(mut st) = run_state.lock()
                                                    {
                                                        st.skip_write_confirm = true;
                                                        st.trusted_action_update =
                                                            Some(TrustedActionUpdate {
                                                                repository_id: target
                                                                    .repository_id
                                                                    .clone(),
                                                                action_id: action_id.clone(),
                                                                trusted_prompt_hash: expected_hash
                                                                    .clone(),
                                                            });
                                                    }
                                                }
                                                Err(err) => {
                                                    return PiToolExecutionResult::error(err);
                                                }
                                            }
                                        }
                                        false
                                    }
                                };
                                let allow_remote = raw_args.iter().any(|a| {
                                    matches!(a.as_str(), "fetch" | "push" | "pull" | "ls-remote")
                                });
                                match exec_controlled_git(
                                    &effective_cwd,
                                    &raw_args,
                                    allow_remote,
                                    is_read_only,
                                )
                                .await
                                {
                                    Ok(out) => {
                                        let should_persist = (!is_read_only && out.success)
                                            || !out.extracted_links.is_empty();
                                        if let Ok(mut st) = run_state.lock() {
                                            if !is_read_only && out.success {
                                                st.wrote_repo = true;
                                            }
                                            append_unique_links(
                                                &mut st.links,
                                                out.extracted_links.clone(),
                                            );
                                            push_timeline_step(
                                                &mut st.timeline,
                                                if out.success { "done" } else { "failed" },
                                                status_label,
                                            );
                                        }
                                        if should_persist {
                                            persist("running", String::new());
                                        }
                                        let body = format!(
                                            "exitCode: {:?}\nstdout:\n{}\nstderr:\n{}",
                                            out.exit_code, out.stdout, out.stderr
                                        );
                                        if out.success {
                                            PiToolExecutionResult::success(body)
                                        } else {
                                            PiToolExecutionResult::error(body)
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "run_platform_cli" => {
                                let cli = args.get("cli").and_then(Value::as_str).unwrap_or("");
                                let raw_args: Vec<String> = args
                                    .get("args")
                                    .and_then(Value::as_array)
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let body_text = args.get("bodyText").and_then(Value::as_str);
                                let body_flag = args.get("bodyFlag").and_then(Value::as_str);
                                let decision =
                                    classify_platform_cli_command(cli, &raw_args, &remotes);
                                match decision {
                                    CommandDecision::DenyRedline { code, reason } => {
                                        return PiToolExecutionResult::error(format!(
                                            "Blocked by Picket hard redline ({code}): {reason}"
                                        ));
                                    }
                                    CommandDecision::AllowTier0 | CommandDecision::AllowTier1 => {}
                                    CommandDecision::RequireConfirmTier2 {
                                        reason,
                                        preview,
                                        allow_dont_ask_again,
                                    } => {
                                        let already_trusted = allow_dont_ask_again
                                            && run_state
                                                .lock()
                                                .map(|st| st.skip_write_confirm)
                                                .unwrap_or(false);
                                        if !already_trusted {
                                            let confirm_call_id = format!("confirm-{call_id}");
                                            let q = build_tier2_confirm_questionnaire(
                                                locale,
                                                &reason,
                                                &preview,
                                                allow_dont_ask_again,
                                                &action_label,
                                            );
                                            emit(
                                                "needsDecision",
                                                Some(confirm_call_id.clone()),
                                                Some(CustomActionUiMessage {
                                                    id: format!("ask-{run_id}-{confirm_call_id}"),
                                                    kind: "question".into(),
                                                    title: q.questions[0].header.clone(),
                                                    description: q.questions[0].question.clone(),
                                                    scope: Some(action_label.clone()),
                                                    files: None,
                                                    commits: None,
                                                    links: None,
                                                    questionnaire: Some(q.clone()),
                                                    progress: None,
                                                }),
                                                None,
                                            );
                                            match coordinator
                                                .suspend_for_question(
                                                    &run_id,
                                                    &confirm_call_id,
                                                    &q,
                                                    Arc::clone(&grants),
                                                )
                                                .await
                                            {
                                                Ok(resp) => {
                                                    let approved = !resp.cancelled
                                                        && resp.answers.iter().any(|ans| {
                                                            ans.selected_option_ids
                                                                .iter()
                                                                .any(|id| id == "approve-write")
                                                        });
                                                    if !approved {
                                                        return PiToolExecutionResult::error(
                                                            "User declined to run the platform CLI command.",
                                                        );
                                                    }
                                                    if allow_dont_ask_again
                                                        && resp.dont_ask_again == Some(true)
                                                        && let Ok(mut st) = run_state.lock()
                                                    {
                                                        st.skip_write_confirm = true;
                                                        st.trusted_action_update =
                                                            Some(TrustedActionUpdate {
                                                                repository_id: target
                                                                    .repository_id
                                                                    .clone(),
                                                                action_id: action_id.clone(),
                                                                trusted_prompt_hash: expected_hash
                                                                    .clone(),
                                                            });
                                                    }
                                                }
                                                Err(err) => {
                                                    return PiToolExecutionResult::error(err);
                                                }
                                            }
                                        }
                                    }
                                }
                                match exec_controlled_platform_cli(
                                    cli,
                                    &root,
                                    &raw_args,
                                    body_text,
                                    body_flag,
                                    override_dirs.as_deref(),
                                )
                                .await
                                {
                                    Ok(out) => {
                                        let has_links = !out.extracted_links.is_empty();
                                        if let Ok(mut st) = run_state.lock() {
                                            append_unique_links(
                                                &mut st.links,
                                                out.extracted_links.clone(),
                                            );
                                            push_timeline_step(
                                                &mut st.timeline,
                                                if out.success { "done" } else { "failed" },
                                                status_label,
                                            );
                                        }
                                        if out.success && has_links {
                                            persist("running", String::new());
                                        }
                                        let body = format!(
                                            "exitCode: {:?}\nstdout:\n{}\nstderr:\n{}",
                                            out.exit_code, out.stdout, out.stderr
                                        );
                                        if out.success {
                                            PiToolExecutionResult::success(body)
                                        } else {
                                            PiToolExecutionResult::error(body)
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "commit_exact_scope" => {
                                let requested_ids: Vec<String> = args
                                    .get("fileIds")
                                    .and_then(Value::as_array)
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let message = args
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .trim()
                                    .to_owned();
                                let should_push = args
                                    .get("push")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                if requested_ids.is_empty() || message.is_empty() {
                                    return PiToolExecutionResult::error(
                                        "commit_exact_scope requires non-empty `fileIds` and `message`.",
                                    );
                                }
                                let already_committed = run_state
                                    .lock()
                                    .map(|st| st.committed_file_ids.clone())
                                    .unwrap_or_default();
                                let mut matched_selections: Vec<CommitSelection> = Vec::new();
                                for fid in &requested_ids {
                                    if already_committed.contains(fid) {
                                        return PiToolExecutionResult::error(format!(
                                            "File `{fid}` was already committed in this run."
                                        ));
                                    }
                                    let Some(sel) =
                                        frozen_selections.iter().find(|s| s.file_id == *fid)
                                    else {
                                        return PiToolExecutionResult::error(format!(
                                            "File `{fid}` is outside the user's selected scope (or is in Local Keep). Never widen past selected files."
                                        ));
                                    };
                                    matched_selections.push(sel.clone());
                                }
                                let step_n = coordinator.inner.seq.fetch_add(1, Ordering::SeqCst) + 1;
                                let commit_req = CommitRequest {
                                    request_id: format!("act-commit-{step_n}"),
                                    target: target.clone(),
                                    selections: matched_selections,
                                    message,
                                    push: false,
                                    locale,
                                };
                                match store.commit(&commit_req).await {
                                    Ok((snap, landed)) => {
                                        if let Ok(mut st) = run_state.lock() {
                                            st.wrote_repo = true;
                                            for fid in &requested_ids {
                                                st.committed_file_ids.insert(fid.clone());
                                            }
                                            st.commits.push(CPushLandedCommit {
                                                oid: landed.oid.clone(),
                                                short_oid: landed.short_oid.clone(),
                                                title: landed.title.clone(),
                                                branch: landed.branch.clone(),
                                                file_ids: requested_ids.clone(),
                                            });
                                            push_timeline_step(
                                                &mut st.timeline,
                                                "done",
                                                format!("{} {}", landed.short_oid, landed.title),
                                            );
                                        }
                                        let mut response_json = json!({
                                            "committed": true,
                                            "oid": landed.oid,
                                            "shortOid": landed.short_oid,
                                            "title": landed.title,
                                            "branch": landed.branch,
                                        });
                                        let mut latest_snap = snap;
                                        if should_push {
                                            match store.push_branch(&target, None, locale).await {
                                                Ok(PushOutcome::Pushed { remote_branch, snapshot }) => {
                                                    latest_snap = snapshot;
                                                    response_json["pushed"] = json!(true);
                                                    response_json["remoteBranch"] = json!(remote_branch);
                                                }
                                                Ok(PushOutcome::MissingUpstream { remotes, snapshot }) => {
                                                    latest_snap = snapshot;
                                                    response_json["pushed"] = json!(false);
                                                    response_json["pushNote"] = json!(format!("No upstream configured. Remotes: {remotes:?}"));
                                                }
                                                Ok(PushOutcome::Failed { reason, snapshot, .. }) => {
                                                    latest_snap = snapshot;
                                                    response_json["pushed"] = json!(false);
                                                    response_json["pushError"] = json!(reason);
                                                }
                                                Err(err) => {
                                                    response_json["pushed"] = json!(false);
                                                    response_json["pushError"] = json!(err);
                                                }
                                            }
                                        }
                                        emit("running", None, None, Some(latest_snap));
                                        persist("running", String::new());
                                        PiToolExecutionResult::success(response_json.to_string())
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "sync_and_push" => {
                                let mode = args
                                    .get("mode")
                                    .and_then(Value::as_str)
                                    .unwrap_or("push");
                                let upstream_remote = args
                                    .get("setUpstreamRemote")
                                    .and_then(Value::as_str)
                                    .filter(|s| !s.trim().is_empty());
                                match mode {
                                    "pullRebase" => {
                                        match store
                                            .sync_rebase_onto_upstream_with_locale(&target, locale)
                                            .await
                                        {
                                            Ok(RebaseSyncOutcome::AlreadyUpToDate { snapshot }) => {
                                                emit("running", None, None, Some(snapshot));
                                                PiToolExecutionResult::success(
                                                    json!({ "status": "alreadyUpToDate" }).to_string(),
                                                )
                                            }
                                            Ok(RebaseSyncOutcome::Completed {
                                                rebased_commits,
                                                snapshot,
                                            }) => {
                                                if let Ok(mut st) = run_state.lock() {
                                                    st.wrote_repo = true;
                                                }
                                                emit("running", None, None, Some(snapshot));
                                                PiToolExecutionResult::success(
                                                    json!({
                                                        "status": "rebased",
                                                        "rebasedCount": rebased_commits.len()
                                                    })
                                                    .to_string(),
                                                )
                                            }
                                            Ok(RebaseSyncOutcome::Conflict {
                                                upstream,
                                                conflicted_files,
                                                snapshot,
                                            }) => {
                                                emit("running", None, None, Some(snapshot));
                                                PiToolExecutionResult::error(format!(
                                                    "Rebase onto {upstream} hit conflicts in {} file(s); resolve conflicts before continuing.",
                                                    conflicted_files.len()
                                                ))
                                            }
                                            Ok(RebaseSyncOutcome::MissingUpstream { remotes, .. }) => {
                                                PiToolExecutionResult::error(format!(
                                                    "Current branch has no upstream tracking branch (available remotes: {remotes:?})."
                                                ))
                                            }
                                            Ok(RebaseSyncOutcome::UnsafeDirtyOverlap {
                                                overlapping_paths,
                                                ..
                                            }) => PiToolExecutionResult::error(format!(
                                                "Uncommitted local changes overlap upstream commits: {overlapping_paths:?}."
                                            )),
                                            Ok(RebaseSyncOutcome::UnsafePublishedHistory {
                                                reason,
                                                ..
                                            }) => PiToolExecutionResult::error(reason),
                                            Err(err) => PiToolExecutionResult::error(err),
                                        }
                                    }
                                    _ => {
                                        match store
                                            .push_branch(&target, upstream_remote, locale)
                                            .await
                                        {
                                            Ok(PushOutcome::Pushed {
                                                remote_branch,
                                                snapshot,
                                            }) => {
                                                if let Ok(mut st) = run_state.lock() {
                                                    st.wrote_repo = true;
                                                    push_timeline_step(
                                                        &mut st.timeline,
                                                        "done",
                                                        i18n::action_pushed_remote_label(
                                                            locale,
                                                            &remote_branch,
                                                        ),
                                                    );
                                                }
                                                emit("running", None, None, Some(snapshot));
                                                PiToolExecutionResult::success(
                                                    json!({
                                                        "pushed": true,
                                                        "remoteBranch": remote_branch
                                                    })
                                                    .to_string(),
                                                )
                                            }
                                            Ok(PushOutcome::MissingUpstream {
                                                remotes,
                                                snapshot,
                                            }) => {
                                                emit("running", None, None, Some(snapshot));
                                                PiToolExecutionResult::error(format!(
                                                    "Branch has no upstream. Pass setUpstreamRemote (available: {remotes:?})."
                                                ))
                                            }
                                            Ok(PushOutcome::Failed {
                                                reason,
                                                non_fast_forward,
                                                snapshot,
                                                ..
                                            }) => {
                                                emit("running", None, None, Some(snapshot));
                                                PiToolExecutionResult::error(format!(
                                                    "Push failed (nonFastForward={non_fast_forward}): {reason}"
                                                ))
                                            }
                                            Err(err) => PiToolExecutionResult::error(err),
                                        }
                                    }
                                }
                            }
                            "manage_submit_worktree" => {
                                let op = args.get("op").and_then(Value::as_str).unwrap_or("");
                                match op {
                                    "create" => {
                                        let group_id = args
                                            .get("groupId")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .trim()
                                            .to_owned();
                                        let title = args
                                            .get("title")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .trim()
                                            .to_owned();
                                        let branch = args
                                            .get("branch")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .trim()
                                            .to_owned();
                                        let base_ref = args
                                            .get("baseRef")
                                            .and_then(Value::as_str)
                                            .unwrap_or("HEAD")
                                            .trim()
                                            .to_owned();
                                        let reuse_existing = args
                                            .get("reuseExistingBranch")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false);
                                        let requested_ids: Vec<String> = args
                                            .get("fileIds")
                                            .and_then(Value::as_array)
                                            .map(|arr| {
                                                arr.iter()
                                                    .filter_map(Value::as_str)
                                                    .map(str::to_owned)
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                        if group_id.is_empty()
                                            || branch.is_empty()
                                            || requested_ids.is_empty()
                                        {
                                            return PiToolExecutionResult::error(
                                                "manage_submit_worktree(create) requires non-empty `groupId`, `branch`, and `fileIds`.",
                                            );
                                        }
                                        let mut matched_changes = Vec::with_capacity(requested_ids.len());
                                        for fid in &requested_ids {
                                            if !frozen_selections.iter().any(|s| s.file_id == *fid) {
                                                return PiToolExecutionResult::error(format!(
                                                    "File `{fid}` is outside the user's selected scope (or is in Local Keep). Never transfer unselected files."
                                                ));
                                            }
                                            let Some(change) =
                                                live_changes.iter().find(|c| c.id == *fid)
                                            else {
                                                return PiToolExecutionResult::error(format!(
                                                    "Selected file `{fid}` was not found in the active worktree snapshot."
                                                ));
                                            };
                                            matched_changes.push(change.clone());
                                        }
                                        match create_isolated_submit_worktree(
                                            &storage_dir,
                                            &target.repository_id,
                                            &root,
                                            &group_id,
                                            &branch,
                                            &base_ref,
                                            reuse_existing,
                                            &matched_changes,
                                        )
                                        .await
                                        {
                                            Ok(created) => {
                                                if let Ok(mut st) = run_state.lock() {
                                                    st.wrote_repo = true;
                                                    if st.submit_ledger.workflow_kind.is_empty() {
                                                        st.submit_ledger.workflow_kind = "submit".into();
                                                    }
                                                    if st.submit_ledger.original_branch.is_none() {
                                                        st.submit_ledger.original_branch =
                                                            Some(created.original_branch.clone());
                                                    }
                                                    if st.submit_ledger.original_head.is_none() {
                                                        st.submit_ledger.original_head =
                                                            Some(created.original_head.clone());
                                                    }
                                                    if let Some(existing) = st
                                                        .submit_ledger
                                                        .groups
                                                        .iter_mut()
                                                        .find(|g| g.group_id == created.group_id)
                                                    {
                                                        if !title.is_empty() {
                                                            existing.title = title;
                                                        }
                                                        existing.branch = Some(created.branch.clone());
                                                        existing.base_ref = Some(created.base_ref.clone());
                                                        existing.worktree_path =
                                                            Some(created.worktree_path.clone());
                                                        existing.worktree_removed = false;
                                                        existing.transferred_paths =
                                                            created.transferred_paths.clone();
                                                        existing.preflight_snapshots =
                                                            created.preflight_snapshots.clone();
                                                    } else {
                                                        st.submit_ledger.groups.push(
                                                            SubmitGroupCheckpoint {
                                                                group_id: created.group_id.clone(),
                                                                title,
                                                                issue_url: None,
                                                                issue_number: None,
                                                                issue_closed: false,
                                                                branch: Some(created.branch.clone()),
                                                                base_ref: Some(
                                                                    created.base_ref.clone(),
                                                                ),
                                                                remote: None,
                                                                verified_head_sha: None,
                                                                worktree_path: Some(
                                                                    created.worktree_path.clone(),
                                                                ),
                                                                worktree_removed: false,
                                                                pr_url: None,
                                                                pr_number: None,
                                                                pr_state: None,
                                                                transferred_paths: created
                                                                    .transferred_paths
                                                                    .clone(),
                                                                preflight_snapshots: created
                                                                    .preflight_snapshots
                                                                    .clone(),
                                                            },
                                                        );
                                                    }
                                                    push_timeline_step(
                                                        &mut st.timeline,
                                                        "done",
                                                        status_label,
                                                    );
                                                }
                                                persist("running", String::new());
                                                let body = serde_json::to_string_pretty(&created)
                                                    .unwrap_or_else(|_| "{}".into());
                                                PiToolExecutionResult::success(body)
                                            }
                                            Err(err) => PiToolExecutionResult::error(err),
                                        }
                                    }
                                    "commit" => {
                                        let group_id =
                                            args.get("groupId").and_then(Value::as_str);
                                        let worktree_path =
                                            args.get("worktreePath").and_then(Value::as_str);
                                        let message = args
                                            .get("message")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .trim()
                                            .to_owned();
                                        let push = args
                                            .get("push")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false);
                                        let remote = args
                                            .get("remote")
                                            .and_then(Value::as_str)
                                            .map(str::trim)
                                            .filter(|s| !s.is_empty());
                                        let ledger = run_state
                                            .lock()
                                            .map(|st| st.submit_ledger.clone())
                                            .unwrap_or_default();
                                        let Some(group) = find_group_for_worktree_or_id(
                                            &storage_dir,
                                            &target.repository_id,
                                            &ledger,
                                            group_id,
                                            worktree_path,
                                        ) else {
                                            return PiToolExecutionResult::error(
                                                "No Picket-owned isolated submit worktree found for the specified groupId/worktreePath.",
                                            );
                                        };
                                        let Some(ref wt_path) = group.worktree_path else {
                                            return PiToolExecutionResult::error(
                                                "Recorded submit group has no worktreePath.",
                                            );
                                        };
                                        match commit_in_isolated_worktree(
                                            Path::new(wt_path),
                                            &group.transferred_paths,
                                            &message,
                                            push,
                                            remote,
                                        )
                                        .await
                                        {
                                            Ok(res) => {
                                                let remote_name =
                                                    remote.unwrap_or("origin").to_owned();
                                                if let Ok(mut st) = run_state.lock() {
                                                    st.wrote_repo = true;
                                                    if let Some(existing) = st
                                                        .submit_ledger
                                                        .groups
                                                        .iter_mut()
                                                        .find(|g| g.group_id == group.group_id)
                                                    {
                                                        existing.verified_head_sha =
                                                            Some(res.oid.clone());
                                                        existing.branch = Some(res.branch.clone());
                                                        if res.pushed {
                                                            existing.remote = Some(remote_name);
                                                        }
                                                    } else {
                                                        let mut updated = group.clone();
                                                        updated.verified_head_sha =
                                                            Some(res.oid.clone());
                                                        updated.branch = Some(res.branch.clone());
                                                        if res.pushed {
                                                            updated.remote = Some(remote_name);
                                                        }
                                                        st.submit_ledger.groups.push(updated);
                                                    }
                                                    st.commits.push(CPushLandedCommit {
                                                        oid: res.oid.clone(),
                                                        short_oid: res.short_oid.clone(),
                                                        title: res.title.clone(),
                                                        branch: res.branch.clone(),
                                                        file_ids: Vec::new(),
                                                    });
                                                    push_timeline_step(
                                                        &mut st.timeline,
                                                        "done",
                                                        format!("{} {}", res.short_oid, res.title),
                                                    );
                                                }
                                                persist("running", String::new());
                                                let body = serde_json::to_string_pretty(&res)
                                                    .unwrap_or_else(|_| "{}".into());
                                                PiToolExecutionResult::success(body)
                                            }
                                            Err(err) => PiToolExecutionResult::error(err),
                                        }
                                    }
                                    "list" => {
                                        let mut all_groups = run_state
                                            .lock()
                                            .map(|st| st.submit_ledger.groups.clone())
                                            .unwrap_or_default();
                                        for session in session_store::load_all_sessions(&storage_dir)
                                        {
                                            if session.repository_id != target.repository_id {
                                                continue;
                                            }
                                            for g in session.submit_ledger.groups {
                                                if !all_groups.iter().any(|existing| {
                                                    existing.worktree_path == g.worktree_path
                                                        && existing.group_id == g.group_id
                                                }) {
                                                    all_groups.push(g);
                                                }
                                            }
                                        }
                                        match parse_worktree_list(&root, &all_groups).await {
                                            Ok(entries) => {
                                                if let Ok(mut st) = run_state.lock() {
                                                    push_timeline_step(
                                                        &mut st.timeline,
                                                        "done",
                                                        status_label,
                                                    );
                                                }
                                                let body = serde_json::to_string_pretty(&entries)
                                                    .unwrap_or_else(|_| "[]".into());
                                                PiToolExecutionResult::success(body)
                                            }
                                            Err(err) => PiToolExecutionResult::error(err),
                                        }
                                    }
                                    "remove" => {
                                        let group_id =
                                            args.get("groupId").and_then(Value::as_str);
                                        let worktree_path =
                                            args.get("worktreePath").and_then(Value::as_str);
                                        let expected_head_sha = args
                                            .get("expectedHeadSha")
                                            .and_then(Value::as_str)
                                            .map(str::trim)
                                            .filter(|s| !s.is_empty());
                                        let delete_local_branch = args
                                            .get("deleteLocalBranch")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false);
                                        let ledger = run_state
                                            .lock()
                                            .map(|st| st.submit_ledger.clone())
                                            .unwrap_or_default();
                                        let Some(group) = find_group_for_worktree_or_id(
                                            &storage_dir,
                                            &target.repository_id,
                                            &ledger,
                                            group_id,
                                            worktree_path,
                                        ) else {
                                            return PiToolExecutionResult::error(
                                                "Refused to remove worktree: not owned by any Picket submit session.",
                                            );
                                        };
                                        match remove_verified_submit_worktree(
                                            &root,
                                            &group,
                                            expected_head_sha,
                                            delete_local_branch,
                                        )
                                        .await
                                        {
                                            Ok(res) => {
                                                if let Ok(mut st) = run_state.lock() {
                                                    st.wrote_repo = true;
                                                    if let Some(existing) = st
                                                        .submit_ledger
                                                        .groups
                                                        .iter_mut()
                                                        .find(|g| g.group_id == group.group_id)
                                                    {
                                                        existing.worktree_removed = true;
                                                    }
                                                    push_timeline_step(
                                                        &mut st.timeline,
                                                        "done",
                                                        status_label,
                                                    );
                                                }
                                                persist("running", String::new());
                                                if let Ok(snap) =
                                                    store.refresh_with_locale(&target, locale).await
                                                {
                                                    emit("running", None, None, Some(snap));
                                                }
                                                let body = serde_json::to_string_pretty(&res)
                                                    .unwrap_or_else(|_| "{}".into());
                                                PiToolExecutionResult::success(body)
                                            }
                                            Err(err) => PiToolExecutionResult::error(err),
                                        }
                                    }
                                    other => PiToolExecutionResult::error(format!(
                                        "Unsupported manage_submit_worktree op `{other}` (expected create | commit | list | remove)."
                                    )),
                                }
                            }
                            "cleanup_submit_duplicates" => {
                                let user_confirmed = args
                                    .get("userConfirmed")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                if !user_confirmed {
                                    return PiToolExecutionResult::error(
                                        "Step 5 duplicate cleanup is opt-in: ask the user via `ask_user_question` first, then invoke `cleanup_submit_duplicates` with `userConfirmed: true`.",
                                    );
                                }
                                let filter_ids: HashSet<String> = args
                                    .get("groupIds")
                                    .and_then(Value::as_array)
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let ledger = run_state
                                    .lock()
                                    .map(|st| st.submit_ledger.clone())
                                    .unwrap_or_default();
                                let Some(orig_branch) = ledger.original_branch.as_deref() else {
                                    return PiToolExecutionResult::error(
                                        "No recorded original branch for Step 5 cleanup; create isolated submit worktrees first.",
                                    );
                                };
                                let Some(orig_head) = ledger.original_head.as_deref() else {
                                    return PiToolExecutionResult::error(
                                        "No recorded original HEAD for Step 5 cleanup; create isolated submit worktrees first.",
                                    );
                                };
                                let groups: Vec<SubmitGroupCheckpoint> = ledger
                                    .groups
                                    .iter()
                                    .filter(|g| {
                                        filter_ids.is_empty() || filter_ids.contains(&g.group_id)
                                    })
                                    .cloned()
                                    .collect();
                                match cleanup_verified_submit_duplicates(
                                    &root,
                                    orig_branch,
                                    orig_head,
                                    &groups,
                                )
                                .await
                                {
                                    Ok(outcome) => {
                                        if let Ok(mut st) = run_state.lock() {
                                            if !outcome.cleaned_paths.is_empty() {
                                                st.wrote_repo = true;
                                            }
                                            st.submit_ledger.cleanup = Some(outcome.to_record());
                                            push_timeline_step(
                                                &mut st.timeline,
                                                "done",
                                                status_label,
                                            );
                                        }
                                        persist("running", String::new());
                                        if let Ok(snap) =
                                            store.refresh_with_locale(&target, locale).await
                                        {
                                            emit("running", None, None, Some(snap));
                                        }
                                        let body = serde_json::to_string_pretty(&outcome)
                                            .unwrap_or_else(|_| "{}".into());
                                        PiToolExecutionResult::success(body)
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "record_action_checkpoint" => {
                                let mut added_links: Vec<CustomActionLink> = Vec::new();
                                if let Ok(mut st) = run_state.lock() {
                                    if let Some(wk) =
                                        args.get("workflowKind").and_then(Value::as_str)
                                        && !wk.trim().is_empty()
                                    {
                                        st.submit_ledger.workflow_kind = wk.trim().to_owned();
                                    }
                                    if let Some(ft) =
                                        args.get("firstTarget").and_then(Value::as_str)
                                        && !ft.trim().is_empty()
                                    {
                                        st.submit_ledger.first_target = Some(ft.trim().to_owned());
                                    }
                                    if let Some(fin) =
                                        args.get("finalTarget").and_then(Value::as_str)
                                        && !fin.trim().is_empty()
                                    {
                                        st.submit_ledger.final_target = Some(fin.trim().to_owned());
                                    }
                                    if let Some(gobj) = args.get("group").and_then(Value::as_object)
                                        && let Some(gid) = gobj
                                            .get("groupId")
                                            .and_then(Value::as_str)
                                            .map(str::trim)
                                            .filter(|s| !s.is_empty())
                                    {
                                        let entry = if let Some(existing) = st
                                            .submit_ledger
                                            .groups
                                            .iter_mut()
                                            .find(|g| g.group_id == gid)
                                        {
                                            existing
                                        } else {
                                            st.submit_ledger.groups.push(SubmitGroupCheckpoint {
                                                group_id: gid.to_owned(),
                                                title: String::new(),
                                                issue_url: None,
                                                issue_number: None,
                                                issue_closed: false,
                                                branch: None,
                                                base_ref: None,
                                                remote: None,
                                                verified_head_sha: None,
                                                worktree_path: None,
                                                worktree_removed: false,
                                                pr_url: None,
                                                pr_number: None,
                                                pr_state: None,
                                                transferred_paths: Vec::new(),
                                                preflight_snapshots: Vec::new(),
                                            });
                                            st.submit_ledger.groups.last_mut().unwrap()
                                        };
                                        if let Some(v) = gobj.get("title").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.title = v.trim().to_owned();
                                        }
                                        if let Some(v) = gobj.get("issueUrl").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.issue_url = Some(v.trim().to_owned());
                                            let label = gobj
                                                .get("issueNumber")
                                                .and_then(Value::as_str)
                                                .map(|n| format!("Issue #{n}"))
                                                .unwrap_or_else(|| "Issue".into());
                                            added_links.push(CustomActionLink {
                                                label,
                                                url: v.trim().to_owned(),
                                            });
                                        }
                                        if let Some(v) =
                                            gobj.get("issueNumber").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.issue_number = Some(v.trim().to_owned());
                                        }
                                        if let Some(v) =
                                            gobj.get("issueClosed").and_then(Value::as_bool)
                                        {
                                            entry.issue_closed = v;
                                        }
                                        if let Some(v) = gobj.get("branch").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.branch = Some(v.trim().to_owned());
                                        }
                                        if let Some(v) = gobj.get("baseRef").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.base_ref = Some(v.trim().to_owned());
                                        }
                                        if let Some(v) = gobj.get("remote").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.remote = Some(v.trim().to_owned());
                                        }
                                        if let Some(v) =
                                            gobj.get("verifiedHeadSha").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.verified_head_sha = Some(v.trim().to_owned());
                                        }
                                        if let Some(v) = gobj.get("prUrl").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.pr_url = Some(v.trim().to_owned());
                                            let label = gobj
                                                .get("prNumber")
                                                .and_then(Value::as_str)
                                                .map(|n| format!("PR #{n}"))
                                                .unwrap_or_else(|| "PR/MR".into());
                                            added_links.push(CustomActionLink {
                                                label,
                                                url: v.trim().to_owned(),
                                            });
                                        }
                                        if let Some(v) = gobj.get("prNumber").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.pr_number = Some(v.trim().to_owned());
                                        }
                                        if let Some(v) = gobj.get("prState").and_then(Value::as_str)
                                            && !v.trim().is_empty()
                                        {
                                            entry.pr_state = Some(v.trim().to_owned());
                                        }
                                    }
                                    if let Some(pobj) =
                                        args.get("promotion").and_then(Value::as_object)
                                    {
                                        let src = pobj
                                            .get("sourceBranch")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .trim()
                                            .to_owned();
                                        let dst = pobj
                                            .get("targetBranch")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .trim()
                                            .to_owned();
                                        if !src.is_empty() && !dst.is_empty() {
                                            let prev = st.submit_ledger.promotion.clone();
                                            let pr_url = pobj
                                                .get("prUrl")
                                                .and_then(Value::as_str)
                                                .map(|s| s.trim().to_owned())
                                                .or_else(|| prev.as_ref().and_then(|p| p.pr_url.clone()));
                                            let pr_number = pobj
                                                .get("prNumber")
                                                .and_then(Value::as_str)
                                                .map(|s| s.trim().to_owned())
                                                .or_else(|| {
                                                    prev.as_ref().and_then(|p| p.pr_number.clone())
                                                });
                                            let pr_state = pobj
                                                .get("prState")
                                                .and_then(Value::as_str)
                                                .map(|s| s.trim().to_owned())
                                                .or_else(|| {
                                                    prev.as_ref().and_then(|p| p.pr_state.clone())
                                                });
                                            let verified_head_sha = pobj
                                                .get("verifiedHeadSha")
                                                .and_then(Value::as_str)
                                                .map(|s| s.trim().to_owned())
                                                .or_else(|| {
                                                    prev.as_ref()
                                                        .and_then(|p| p.verified_head_sha.clone())
                                                });
                                            if let Some(ref url) = pr_url {
                                                added_links.push(CustomActionLink {
                                                    label: format!("{src} -> {dst}"),
                                                    url: url.clone(),
                                                });
                                            }
                                            st.submit_ledger.promotion =
                                                Some(SubmitPromotionCheckpoint {
                                                    source_branch: src,
                                                    target_branch: dst,
                                                    pr_url,
                                                    pr_number,
                                                    pr_state,
                                                    verified_head_sha,
                                                });
                                        }
                                    }
                                    if let Some(links_arr) =
                                        args.get("links").and_then(Value::as_array)
                                    {
                                        for item in links_arr {
                                            if let (Some(label), Some(url)) = (
                                                item.get("label").and_then(Value::as_str),
                                                item.get("url").and_then(Value::as_str),
                                            ) && !url.trim().is_empty()
                                            {
                                                added_links.push(CustomActionLink {
                                                    label: label.trim().to_owned(),
                                                    url: url.trim().to_owned(),
                                                });
                                            }
                                        }
                                    }
                                    append_unique_links(&mut st.links, added_links);
                                    push_timeline_step(&mut st.timeline, "done", status_label);
                                }
                                persist("running", String::new());
                                let summary_json = run_state
                                    .lock()
                                    .map(|st| {
                                        json!({
                                            "saved": true,
                                            "groupCount": st.submit_ledger.groups.len(),
                                            "linkCount": st.links.len(),
                                            "hasPromotion": st.submit_ledger.promotion.is_some(),
                                        })
                                        .to_string()
                                    })
                                    .unwrap_or_else(|_| "{\"saved\":true}".into());
                                PiToolExecutionResult::success(summary_json)
                            }
                            other => PiToolExecutionResult::error(format!(
                                "Unsupported tool `{other}` for custom action execution."
                            )),
                        }
                    }
                },
            )
            .await;

        let refreshed_snapshot = store
            .refresh_with_locale(&request.target, locale)
            .await
            .ok();

        match outcome {
            Ok(PiTaskOutcome::CustomActionCompleted { summary, links }) => {
                let (timeline, commits, merged_links, trusted) = {
                    let mut st = run_state.lock().unwrap();
                    append_unique_links(
                        &mut st.links,
                        links
                            .into_iter()
                            .map(|l| CustomActionLink {
                                label: l.label,
                                url: l.url,
                            })
                            .collect(),
                    );
                    for step in &mut st.timeline {
                        if step.state == "active" {
                            step.state = "done".into();
                        }
                    }
                    (
                        st.timeline.clone(),
                        st.commits.clone(),
                        st.links.clone(),
                        st.trusted_action_update.clone(),
                    )
                };
                let commit_summaries: Vec<CustomActionMessageCommitSummary> = commits
                    .iter()
                    .map(|c| CustomActionMessageCommitSummary {
                        hash: c.short_oid.clone(),
                        title: c.title.clone(),
                    })
                    .collect();
                let done_msg = CustomActionUiMessage {
                    id: format!("done-action-{run_id}"),
                    kind: "done".into(),
                    title: i18n::action_completed_title(locale, &action_label),
                    description: summary.clone(),
                    scope: None,
                    files: None,
                    commits: (!commit_summaries.is_empty()).then_some(commit_summaries),
                    links: (!merged_links.is_empty()).then_some(merged_links.clone()),
                    questionnaire: None,
                    progress: None,
                };
                persist_state("done", summary.clone());
                Ok(CustomActionCommandReply {
                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                    kind: "customAction",
                    request_id: request.request_id.clone(),
                    run_id,
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    action_id,
                    action_label,
                    status: "done",
                    summary,
                    message: Some(done_msg),
                    timeline,
                    commits,
                    links: merged_links,
                    trusted_action_update: trusted,
                    repository_snapshot: refreshed_snapshot,
                })
            }
            Ok(PiTaskOutcome::Cancelled) => {
                let (timeline, commits, merged_links, trusted) = run_state
                    .lock()
                    .map(|st| {
                        (
                            st.timeline.clone(),
                            st.commits.clone(),
                            st.links.clone(),
                            st.trusted_action_update.clone(),
                        )
                    })
                    .unwrap_or_default();
                let cancel_summary: String = i18n::action_cancelled_summary(locale).into();
                persist_state("cancelled", cancel_summary.clone());
                Ok(CustomActionCommandReply {
                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                    kind: "customAction",
                    request_id: request.request_id.clone(),
                    run_id,
                    repository_id: request.target.repository_id.clone(),
                    worktree_id: request.target.worktree_id.clone(),
                    session_id: request.target.session_id.clone(),
                    action_id,
                    action_label,
                    status: "cancelled",
                    summary: cancel_summary,
                    message: None,
                    timeline,
                    commits,
                    links: merged_links,
                    trusted_action_update: trusted,
                    repository_snapshot: refreshed_snapshot,
                })
            }
            Ok(other) => {
                let msg = format!("Unexpected Pi outcome for custom action: {other:?}");
                persist_state("failed", msg.clone());
                Err(msg)
            }
            Err(err) => {
                persist_state("failed", err.clone());
                Err(err)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn generate_custom_action<F>(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        store: &RepositoryStore,
        request: &GenerateCustomActionRequest,
        on_progress: F,
    ) -> Result<GenerateCustomActionReply, String>
    where
        F: Fn(GenerateCustomActionProgressEvent) + Send + Sync + 'static,
    {
        let locale = request.locale;
        repository::validate_request_id_with_locale(&request.request_id, locale)?;
        let user_request = request.user_request.trim().to_owned();
        if user_request.is_empty() || user_request.chars().count() > 2_000 {
            return Err(i18n::action_create_empty_request_error(locale).into());
        }

        let run_id = request.request_id.clone();
        let worktree_root = store
            .worktree_root_for_target(&request.target, locale)
            .await?;
        let live_snapshot = store.refresh_with_locale(&request.target, locale).await?;
        let active_tree = live_snapshot
            .worktrees
            .iter()
            .find(|wt| wt.worktree_id == request.target.worktree_id)
            .ok_or_else(|| "Target worktree not found in live repository snapshot".to_string())?;
        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &request.model_id, locale)
            .await?;
        let api_key_for_redaction = (!spec.api_key.trim().is_empty()).then(|| spec.api_key.clone());
        let skill_snapshot = Arc::new(RuntimeSkillSnapshot::load_from_data_dir(storage_dir));
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let progress_cb = Arc::new(on_progress);

        progress_cb(GenerateCustomActionProgressEvent {
            protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
            run_id: run_id.clone(),
            status: "running",
            status_label: i18n::action_create_inspecting_label(locale).into(),
            pending_call_id: None,
            questionnaire: None,
        });

        let context = RequestContext {
            repository_id: request.target.repository_id.clone(),
            worktree_id: request.target.worktree_id.clone(),
            session_id: request.target.session_id.clone(),
            scope_version: request.target.scope_version.clone(),
        };
        let command = PiCommand::GenerateCustomAction {
            request_id: run_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: active_tree.branch.clone(),
            existing_labels: request.existing_labels.clone(),
            user_request,
            locale,
            skill_overrides: skill_snapshot.catalog_option(),
        };

        let coordinator = self.clone();
        let store_clone = store.clone();
        let target_clone = request.target.clone();
        let run_id_clone = run_id.clone();

        let outcome = supervisor
            .run_task_with_tools(
                paths,
                run_id.clone(),
                context,
                &command,
                CUSTOM_ACTION_IDLE_TIMEOUT,
                api_key_for_redaction.as_deref(),
                |_event| Ok(()),
                move |call_id, tool_name, args| {
                    let coordinator = coordinator.clone();
                    let store = store_clone.clone();
                    let target = target_clone.clone();
                    let root = worktree_root.clone();
                    let skills = Arc::clone(&skill_snapshot);
                    let grants = Arc::clone(&grants);
                    let cb = Arc::clone(&progress_cb);
                    let run_id = run_id_clone.clone();
                    async move {
                        let override_dirs = coordinator.override_cli_dirs();
                        let label = format_tool_status_label(locale, &tool_name, &args);
                        if tool_name != "read_skill_override_doc" && tool_name != "ask_user_question" {
                            cb(GenerateCustomActionProgressEvent {
                                protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                run_id: run_id.clone(),
                                status: "running",
                                status_label: label,
                                pending_call_id: None,
                                questionnaire: None,
                            });
                        }
                        match tool_name.as_str() {
                            "read_skill_override_doc" => {
                                let skill = args.get("skill").and_then(Value::as_str).unwrap_or("");
                                let path = args
                                    .get("path")
                                    .and_then(Value::as_str)
                                    .unwrap_or("SKILL.md");
                                match skills.read_doc(skill, path) {
                                    Ok(Some(content)) => PiToolExecutionResult::success(content),
                                    Ok(None) => PiToolExecutionResult::error("Not found in overrides"),
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "read_repo_file" | "search_repo" => {
                                match store.execute_read_tool(&target, &tool_name, &args, locale).await {
                                    Ok(content) => PiToolExecutionResult::success(content),
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "check_cli_env" => {
                                let snap = check_cli_env(&root, override_dirs.as_deref()).await;
                                PiToolExecutionResult::success(
                                    serde_json::to_string_pretty(&snap)
                                        .unwrap_or_else(|_| "{}".into()),
                                )
                            }
                            "run_git" => {
                                let raw_args: Vec<String> = args
                                    .get("args")
                                    .and_then(Value::as_array)
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                // Strictly Tier-0 read-only during Create button drafting!
                                if classify_git_command(&raw_args) != CommandDecision::AllowTier0 {
                                    return PiToolExecutionResult::error(
                                        "Create with Pi is strictly read-only; only Tier-0 Git inspection commands are allowed.",
                                    );
                                }
                                match exec_controlled_git(&root, &raw_args, false, true).await {
                                    Ok(out) => {
                                        let body = format!(
                                            "exitCode: {:?}\nstdout:\n{}\nstderr:\n{}",
                                            out.exit_code, out.stdout, out.stderr
                                        );
                                        if out.success {
                                            PiToolExecutionResult::success(body)
                                        } else {
                                            PiToolExecutionResult::error(body)
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            "ask_user_question" => {
                                let req: AskUserQuestionRequest = match serde_json::from_value(args) {
                                    Ok(parsed) => parsed,
                                    Err(err) => {
                                        return PiToolExecutionResult::error(format!(
                                            "Invalid ask_user_question payload: {err}"
                                        ));
                                    }
                                };
                                cb(GenerateCustomActionProgressEvent {
                                    protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                    run_id: run_id.clone(),
                                    status: "needsDecision",
                                    status_label: i18n::action_create_needs_input_label(locale)
                                        .into(),
                                    pending_call_id: Some(call_id.clone()),
                                    questionnaire: Some(req.clone()),
                                });
                                match coordinator
                                    .suspend_for_question(&run_id, &call_id, &req, grants)
                                    .await
                                {
                                    Ok(resp) => {
                                        cb(GenerateCustomActionProgressEvent {
                                            protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                                            run_id: run_id.clone(),
                                            status: "running",
                                            status_label: i18n::action_create_drafting_label(
                                                locale,
                                            )
                                            .into(),
                                            pending_call_id: None,
                                            questionnaire: None,
                                        });
                                        let details = serde_json::to_value(&resp).ok();
                                        let content = serde_json::to_string(&resp)
                                            .unwrap_or_else(|_| "{}".into());
                                        PiToolExecutionResult {
                                            ok: true,
                                            content,
                                            details,
                                            is_error: false,
                                        }
                                    }
                                    Err(err) => PiToolExecutionResult::error(err),
                                }
                            }
                            other => PiToolExecutionResult::error(format!(
                                "Tool `{other}` is not available in Create with Picket read-only mode."
                            )),
                        }
                    }
                },
            )
            .await;

        match outcome {
            Ok(PiTaskOutcome::CustomActionDrafted { draft }) => Ok(GenerateCustomActionReply {
                protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                kind: "generateCustomAction",
                request_id: request.request_id.clone(),
                status: "done",
                draft: Some(CustomActionDraft {
                    label: draft.label,
                    prompt: draft.prompt,
                    summary: draft.summary,
                }),
            }),
            Ok(PiTaskOutcome::Cancelled) => Ok(GenerateCustomActionReply {
                protocol_version: CUSTOM_ACTION_PROTOCOL_VERSION,
                kind: "generateCustomAction",
                request_id: request.request_id.clone(),
                status: "cancelled",
                draft: None,
            }),
            Ok(other) => Err(format!(
                "Unexpected Pi outcome for generate_custom_action: {other:?}"
            )),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ask_user_question_oneshot_records_install_grant_only_when_user_approves() {
        let coordinator = CustomActionCoordinator::default();
        let grants = Arc::new(Mutex::new(SessionAuthGrants::default()));
        let req = AskUserQuestionRequest {
            questions: vec![AskUserQuestionItem {
                id: "q-1".into(),
                header: "Install gh".into(),
                question: "GitHub CLI is missing. Install via Homebrew?".into(),
                multi_select: None,
                options: vec![
                    AskUserQuestionOption {
                        id: "install-gh".into(),
                        label: "Install gh via Homebrew".into(),
                        description: "Runs brew install gh".into(),
                        recommended: Some(true),
                        preview: Some("brew install gh".into()),
                    },
                    AskUserQuestionOption {
                        id: "skip".into(),
                        label: "Skip".into(),
                        description: "Cancel installation".into(),
                        recommended: None,
                        preview: None,
                    },
                ],
            }],
            context: None,
        };

        // 1. Declining/skipping must NOT record an install grant:
        let coord_clone = coordinator.clone();
        let grants_clone = Arc::clone(&grants);
        let req_clone = req.clone();
        let waiter = tokio::spawn(async move {
            coord_clone
                .suspend_for_question("run-1", "tc-1", &req_clone, grants_clone)
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        coordinator
            .respond_question(&RespondCustomActionRequest {
                request_id: "resp-1".into(),
                run_id: "run-1".into(),
                call_id: "tc-1".into(),
                response: AskUserQuestionResponse {
                    cancelled: false,
                    answers: vec![AskUserQuestionAnswer {
                        question_index: 0,
                        question_id: "q-1".into(),
                        header: "Install gh".into(),
                        selected_option_ids: vec!["skip".into()],
                        selected_labels: vec!["Skip".into()],
                        custom_text: None,
                    }],
                    unanswered: vec![],
                    global_note: None,
                    dont_ask_again: None,
                },
                locale: LanguageKind::En,
            })
            .unwrap();
        let resp = waiter.await.unwrap().unwrap();
        assert!(!resp.cancelled);
        assert!(!grants.lock().unwrap().authorized_installs.contains("gh"));

        // 2. Approving `install-gh` records the grant in Rust:
        let coord_clone = coordinator.clone();
        let grants_clone = Arc::clone(&grants);
        let waiter2 = tokio::spawn(async move {
            coord_clone
                .suspend_for_question("run-1", "tc-2", &req, grants_clone)
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        coordinator
            .respond_question(&RespondCustomActionRequest {
                request_id: "resp-2".into(),
                run_id: "run-1".into(),
                call_id: "tc-2".into(),
                response: AskUserQuestionResponse {
                    cancelled: false,
                    answers: vec![AskUserQuestionAnswer {
                        question_index: 0,
                        question_id: "q-1".into(),
                        header: "Install gh".into(),
                        selected_option_ids: vec!["install-gh".into()],
                        selected_labels: vec!["Install gh via Homebrew".into()],
                        custom_text: None,
                    }],
                    unanswered: vec![],
                    global_note: None,
                    dont_ask_again: None,
                },
                locale: LanguageKind::En,
            })
            .unwrap();
        let resp2 = waiter2.await.unwrap().unwrap();
        assert!(!resp2.cancelled);
        assert!(grants.lock().unwrap().authorized_installs.contains("gh"));
        assert!(!grants.lock().unwrap().authorized_installs.contains("glab"));
    }
}
