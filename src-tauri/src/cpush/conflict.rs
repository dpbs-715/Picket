//! User-authorized Agent conflict resolution during rebase and Pi conflict resolver invocation.

use super::{
    CPUSH_AI_IDLE_TIMEOUT, CPushCoordinator,
    helpers::{apply_rebased_commits, enter_conflict_state, summarize_commits},
    types::*,
};
use crate::{
    foundation::pi::{
        PiCommand, PiConflictFile, PiConflictResolution, PiPaths, PiSupervisor, PiTaskOutcome,
        RequestContext,
    },
    i18n,
    models::store::{ModelStore, ResolvedRuntimeSpec},
    repository::{
        RepositoryStore,
        remote::{ExtractedConflictFile, RebaseContinueOutcome},
    },
};
use std::path::Path;

impl CPushCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_agent_conflict_merge(
        &self,
        storage_dir: &Path,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        models: &ModelStore,
        repo_store: &RepositoryStore,
        request_id: &str,
        session: &mut StoredCPushSession,
        previous_msg: &CPushUiMessage,
        instruction: &str,
        on_progress: &mut (impl FnMut(CPushProgressEvent) -> Result<(), String> + Send),
    ) -> Result<CPushCommandReply, String> {
        let locale = session.locale;
        if session.model_id.trim().is_empty() {
            return Err(i18n::cpush::cpush_agent_merge_no_model_error(locale).into());
        }
        let spec = models
            .resolve_runtime_spec_with_locale(storage_dir, &session.model_id, locale)
            .await?;
        let read_target = self.session_read_target(session);
        let batch = repo_store
            .extract_conflict_batch(&read_target, locale)
            .await?;

        let answer_text = if instruction.is_empty() {
            i18n::cpush::cpush_option_agent_merge(locale).to_owned()
        } else {
            instruction.to_owned()
        };
        let prev_timeline = session.timeline.clone();
        let prev_msg = previous_msg.clone();

        session.qa_log.push(CPushQaItem {
            question_id: previous_msg.id.clone(),
            question: previous_msg.title.clone(),
            answer: answer_text.clone(),
            option_id: instruction.is_empty().then(|| "agent-merge".into()),
        });
        session.session_status = CPushSessionStatus::Running;
        session.timeline = vec![
            CPushTimelineItem {
                state: "done".into(),
                label: i18n::cpush::cpush_timeline_authorized_agent_merge(locale, &answer_text),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: i18n::cpush::cpush_timeline_merging_files(locale, batch.files.len()),
            },
        ];
        session.message = Some(CPushUiMessage {
            id: format!("progress-merge-{request_id}"),
            kind: "progress".into(),
            title: i18n::cpush::cpush_progress_merging_title(locale).into(),
            description: i18n::cpush::cpush_progress_merging_desc(locale, instruction),
            scope: Some(i18n::cpush::cpush_merging_scope(locale, &batch.upstream)),
            files: None,
            commits: None,
            options: None,
            progress: Some(CPushMessageProgress {
                completed: 3,
                total: 4,
                label: i18n::cpush::cpush_progress_merging_steps(locale).into(),
            }),
        });
        self.save_in_memory(session)?;
        let _ = on_progress(CPushProgressEvent {
            protocol_version: CPUSH_PROTOCOL_VERSION,
            session: session.to_public(),
            repository_snapshot: None,
        });

        let resolution = match self
            .invoke_pi_conflict_resolver(
                paths,
                supervisor,
                &spec,
                request_id,
                session,
                &batch.upstream,
                instruction,
                &batch.files,
            )
            .await
        {
            Ok(res) => res,
            Err(err) => {
                session.qa_log.pop();
                session.session_status = CPushSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description =
                    i18n::cpush::cpush_merge_resolver_failed_desc(locale, &err);
                session.message = Some(restored_msg);
                self.save_in_memory(session)?;
                let _ = self.persist_all(storage_dir).await;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: None,
                });
                return Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: None,
                });
            }
        };

        let continue_outcome = repo_store
            .apply_conflict_resolution_and_continue(
                &read_target,
                &batch.files,
                &resolution.resolved_files,
                locale,
            )
            .await;

        match continue_outcome {
            Err(err) => {
                let refreshed = repo_store
                    .refresh_with_locale(&read_target, locale)
                    .await
                    .ok();
                session.session_status = CPushSessionStatus::Conflict;
                session.timeline = prev_timeline;
                let mut restored_msg = prev_msg;
                restored_msg.description = i18n::cpush::cpush_merge_apply_failed_desc(locale, &err);
                session.message = Some(restored_msg);
                self.save_in_memory(session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: refreshed.clone(),
                });
                Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: refreshed,
                })
            }
            Ok(RebaseContinueOutcome::NextConflict {
                upstream,
                conflicted_files,
                snapshot,
            }) => {
                enter_conflict_state(
                    session,
                    &upstream,
                    &conflicted_files,
                    true,
                    Some(i18n::cpush::cpush_next_commit_conflict_reason(locale)),
                );
                self.save_in_memory(session)?;
                self.persist_all(storage_dir).await?;
                let public = session.to_public();
                let _ = on_progress(CPushProgressEvent {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    session: public.clone(),
                    repository_snapshot: Some(snapshot.clone()),
                });
                Ok(CPushCommandReply {
                    protocol_version: CPUSH_PROTOCOL_VERSION,
                    kind: "cPush",
                    request_id: request_id.to_owned(),
                    session: public,
                    prepared_plan: None,
                    repository_snapshot: Some(snapshot),
                })
            }
            Ok(RebaseContinueOutcome::Completed {
                upstream,
                rebased_commits,
                snapshot,
                ..
            }) => {
                apply_rebased_commits(session, &rebased_commits);
                if session.push_authorized {
                    self.sync_and_push_commits(
                        storage_dir,
                        repo_store,
                        request_id,
                        session,
                        None,
                        Some(snapshot),
                        on_progress,
                    )
                    .await
                } else {
                    // Standalone Pull & Rebase never auto-pushes (`R06`):
                    session.session_status = CPushSessionStatus::Done;
                    session.can_retry_remaining = false;
                    session.timeline = vec![
                        CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_agent_merge_done(
                                locale,
                                &resolution.summary,
                            ),
                        },
                        CPushTimelineItem {
                            state: "done".into(),
                            label: i18n::cpush::cpush_timeline_rebase_done_no_push(
                                locale, &upstream,
                            ),
                        },
                    ];
                    let commits = summarize_commits(&session.commits);
                    session.message = Some(CPushUiMessage {
                        id: format!("done-merge-pull-{}", session.run_id),
                        kind: "done".into(),
                        title: i18n::cpush::cpush_done_merge_pull_title(locale, &upstream),
                        description: i18n::cpush::cpush_done_merge_pull_desc(
                            locale,
                            &resolution.summary,
                        ),
                        scope: Some(session.branch.clone()),
                        files: None,
                        commits: (!commits.is_empty()).then_some(commits),
                        options: None,
                        progress: None,
                    });
                    self.save_in_memory(session)?;
                    self.persist_all(storage_dir).await?;
                    let public = session.to_public();
                    let _ = on_progress(CPushProgressEvent {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        session: public.clone(),
                        repository_snapshot: Some(snapshot.clone()),
                    });
                    Ok(CPushCommandReply {
                        protocol_version: CPUSH_PROTOCOL_VERSION,
                        kind: "cPush",
                        request_id: request_id.to_owned(),
                        session: public,
                        prepared_plan: None,
                        repository_snapshot: Some(snapshot),
                    })
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn invoke_pi_conflict_resolver(
        &self,
        paths: &PiPaths,
        supervisor: &PiSupervisor,
        spec: &ResolvedRuntimeSpec,
        request_id: &str,
        session: &StoredCPushSession,
        upstream: &str,
        instruction: &str,
        files: &[ExtractedConflictFile],
    ) -> Result<PiConflictResolution, String> {
        let context = RequestContext {
            repository_id: session.repository_id.clone(),
            worktree_id: session.worktree_id.clone(),
            session_id: session.session_id.clone(),
            scope_version: session.frozen_scope_version.clone(),
        };
        let pi_request_id = format!("{request_id}-conflict");
        let files_wire: Vec<PiConflictFile> = files
            .iter()
            .map(|f| PiConflictFile {
                file_id: f.file_id.clone(),
                path: f.path.clone(),
                base_content: f.base_content.clone(),
                upstream_content: f.upstream_content.clone(),
                local_content: f.local_content.clone(),
                conflicted_content: f.conflicted_content.clone(),
            })
            .collect();
        let commits_wire: Vec<String> = session
            .commits
            .iter()
            .map(|c| format!("{} {}", c.short_oid, c.title))
            .collect();
        let command = PiCommand::ResolveConflict {
            request_id: pi_request_id.clone(),
            context: context.clone(),
            model: spec.to_pi_spec(),
            branch: session.branch.clone(),
            upstream: upstream.to_owned(),
            user_instruction: instruction.to_owned(),
            commits: commits_wire,
            files: files_wire,
            locale: session.locale,
        };

        let outcome = supervisor
            .run_task(
                paths,
                pi_request_id,
                context,
                &command,
                CPUSH_AI_IDLE_TIMEOUT,
                Some(&spec.api_key),
                |_| Ok(()),
            )
            .await?;

        match outcome {
            PiTaskOutcome::ConflictResolved { resolution } => Ok(resolution),
            PiTaskOutcome::Cancelled => {
                Err(i18n::cpush::cpush_conflict_cancelled_error(session.locale).into())
            }
            _ => Err("Conflict merge returned an unexpected event type".into()),
        }
    }
}
