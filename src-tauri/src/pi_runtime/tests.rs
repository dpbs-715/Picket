use super::*;
use crate::i18n::LanguageKind;
use std::time::Duration;

fn sample_model() -> PiProviderSpec {
    PiProviderSpec {
        provider: "openai",
        api_format: Some("openai-responses"),
        model_id: "gpt-4.1".into(),
        base_url: "https://api.openai.com/v1".into(),
        api_key: "sk-test".into(),
        context_window: Some(200_000),
        max_output_tokens: Some(32_768),
    }
}

#[test]
fn pi_command_serializes_locale_and_commit_language_in_camel_case() {
    let context = RequestContext {
        repository_id: "repo-1".into(),
        worktree_id: "wt-1".into(),
        session_id: "sess-1".into(),
        scope_version: "scope-1".into(),
    };
    let test_conn = serde_json::to_value(PiCommand::TestConnection {
        request_id: "req-conn".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::Zh,
    })
    .unwrap();
    assert_eq!(test_conn["type"], "testConnection");
    assert_eq!(test_conn["locale"], "zh");

    let cpush_cmd = serde_json::to_value(PiCommand::RunCustomAction {
        mode: Some(CustomActionRunMode::Cpush),
        request_id: "req-cpush".into(),
        context: Box::new(context.clone()),
        model: Box::new(sample_model()),
        branch: "feat/i18n".into(),
        detached: false,
        remote_branches: vec!["origin/feat/i18n".into()],
        action_label: "C-Push".into(),
        action_prompt: String::new(),
        extra_prompt: "split lockfile".into(),
        selected_files: Vec::new(),
        unselected_paths: Vec::new(),
        local_keep_paths: Vec::new(),
        recent_commits: Some(vec!["feat: init".into()]),
        push_authorized: Some(true),
        upstream: None,
        commits: None,
        conflict_files: None,
        locale: LanguageKind::Zh,
        commit_language: LanguageKind::En,
        approval_mode: None,
        previous_checkpoint: None,
        follow_up_prompt: None,
        skill_overrides: None,
    })
    .unwrap();
    assert_eq!(cpush_cmd["type"], "runCustomAction");
    assert_eq!(cpush_cmd["mode"], "cpush");
    assert_eq!(cpush_cmd["extraPrompt"], "split lockfile");
    assert_eq!(cpush_cmd["locale"], "zh");
    assert_eq!(cpush_cmd["commitLanguage"], "en");
    assert_eq!(cpush_cmd["pushAuthorized"], true);

    let resolve = serde_json::to_value(PiCommand::RunCustomAction {
        mode: Some(CustomActionRunMode::ResolveConflict),
        request_id: "req-conflict".into(),
        context: Box::new(context),
        model: Box::new(sample_model()),
        branch: "feat/i18n".into(),
        detached: false,
        remote_branches: Vec::new(),
        action_label: "Resolve Conflict".into(),
        action_prompt: String::new(),
        extra_prompt: "keep both".into(),
        selected_files: Vec::new(),
        unselected_paths: Vec::new(),
        local_keep_paths: Vec::new(),
        recent_commits: None,
        push_authorized: None,
        upstream: Some("origin/feat/i18n".into()),
        commits: Some(vec!["abc1234 feat: local".into()]),
        conflict_files: Some(vec![PiConflictFileSummary {
            file_id: "f1".into(),
            path: "src/app.ts".into(),
            line_count: 12,
            has_base: Some(true),
        }]),
        locale: LanguageKind::En,
        commit_language: LanguageKind::En,
        approval_mode: None,
        previous_checkpoint: None,
        follow_up_prompt: None,
        skill_overrides: None,
    })
    .unwrap();
    assert_eq!(resolve["type"], "runCustomAction");
    assert_eq!(resolve["mode"], "resolveConflict");
    assert_eq!(resolve["upstream"], "origin/feat/i18n");
    assert_eq!(resolve["conflictFiles"][0]["lineCount"], 12);
    assert_eq!(resolve["locale"], "en");
}

#[tokio::test]
async fn real_sdk_streams_and_child_is_reaped() {
    let mut deltas = String::new();
    let report = PiSupervisor::default()
        .run_probe(&PiPaths::development(), "complete-test".into(), |event| {
            if let PiEvent::TextDelta { delta, .. } = event {
                deltas.push_str(delta);
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(report.status, "completed");
    assert_eq!(report.child_exit_code, 0);
    assert!(report.delta_count > 1);
    assert!(deltas.contains("scripted SDK response"));
}

#[tokio::test]
async fn cancellation_settles_and_allows_another_request() {
    let supervisor = PiSupervisor::default();
    let report = supervisor
        .run_probe(&PiPaths::development(), "cancel-test".into(), |event| {
            if matches!(event, PiEvent::TextDelta { .. }) {
                supervisor.cancel("cancel-test")?;
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(report.status, "cancelled");
    assert_eq!(report.child_exit_code, 0);
    let next = supervisor
        .run_probe(&PiPaths::development(), "after-cancel".into(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(next.status, "completed");
    assert_eq!(next.child_exit_code, 0);
    supervisor.shutdown().await.unwrap();
    assert!(
        supervisor
            .run_probe(&PiPaths::development(), "after-close".into(), |_| Ok(()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn crashed_child_is_reaped_and_does_not_block_the_next_request() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("crash.mjs");
    std::fs::write(
        &script,
        r#"
            process.stdout.write(JSON.stringify({type:'ready', protocolVersion:1,
                piCoreVersion:'1.6.0', nodeVersion:process.versions.node, pid:process.pid}) + '\n');
            process.stdin.once('data', () => process.exit(23));
        "#,
    )
    .unwrap();
    let paths = PiPaths {
        node: PiPaths::development().node,
        script,
    };
    let supervisor = PiSupervisor::default();
    let mut crashed_pid = 0;
    let result = supervisor
        .run_probe(&paths, "crash-test".into(), |event| {
            if let PiEvent::Ready { pid, .. } = event {
                crashed_pid = *pid;
            }
            Ok(())
        })
        .await;
    assert!(result.unwrap_err().contains("23"));
    assert_ne!(crashed_pid, 0);
    assert!(
        !std::process::Command::new("/bin/kill")
            .args(["-0", &crashed_pid.to_string()])
            .output()
            .unwrap()
            .status
            .success()
    );
    let next = supervisor
        .run_probe(&PiPaths::development(), "after-crash".into(), |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(next.status, "completed");
    supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_request_is_rejected_while_running() {
    let supervisor = PiSupervisor::default();
    let other = supervisor.clone();
    let paths = PiPaths::development();
    let run =
        tokio::spawn(async move { other.run_probe(&paths, "same-id".into(), |_| Ok(())).await });
    loop {
        if supervisor.has_active_requests() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let duplicate = supervisor
        .run_probe(&PiPaths::development(), "same-id".into(), |_| Ok(()))
        .await;
    assert!(duplicate.unwrap_err().contains("already running"));
    supervisor.shutdown().await.unwrap();
    assert_eq!(run.await.unwrap().unwrap().status, "cancelled");
}

#[tokio::test]
async fn activity_heartbeats_extend_idle_timeout_while_silent_hang_times_out() {
    let directory = tempfile::tempdir().unwrap();
    let active_script = directory.path().join("thinking-stream.mjs");
    std::fs::write(
        &active_script,
        r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            rl.on('line', async (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(0); return; }
              const env = { requestId: cmd.requestId, context: cmd.context };
              emit({ ...env, type: 'started' });
              for (let i = 0; i < 4; i++) {
                await new Promise((r) => setTimeout(r, 80));
                emit({ ...env, type: 'activity' });
              }
              emit({ ...env, type: 'completed', text: 'feat: finished after long thinking\n\n- kept alive by activity heartbeats' });
            });
        "#,
    )
    .unwrap();

    let supervisor = PiSupervisor::default();
    let context = RequestContext {
        repository_id: "repo-1".into(),
        worktree_id: "wt-1".into(),
        session_id: "sess-1".into(),
        scope_version: "scope-1".into(),
    };
    let active_paths = PiPaths {
        node: PiPaths::development().node,
        script: active_script,
    };
    let active_cmd = PiCommand::TestConnection {
        request_id: "req-active".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let outcome = supervisor
        .run_task(
            &active_paths,
            "req-active".into(),
            context.clone(),
            &active_cmd,
            Duration::from_millis(180),
            None,
            |_| Ok(()),
        )
        .await
        .unwrap();
    match outcome {
        PiTaskOutcome::Completed { text } => assert!(text.contains("kept alive")),
        other => panic!("expected Completed, got {other:?}"),
    }

    let silent_script = directory.path().join("silent-hang.mjs");
    std::fs::write(
        &silent_script,
        r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            rl.on('line', (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(1); return; }
              emit({ requestId: cmd.requestId, context: cmd.context, type: 'started' });
              setTimeout(() => {}, 10_000);
            });
        "#,
    )
    .unwrap();
    let silent_paths = PiPaths {
        node: PiPaths::development().node,
        script: silent_script,
    };
    let silent_cmd = PiCommand::TestConnection {
        request_id: "req-silent".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let err = supervisor
        .run_task(
            &silent_paths,
            "req-silent".into(),
            context,
            &silent_cmd,
            Duration::from_millis(180),
            None,
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, crate::error::AppErrorCode::ModelTimeout);
    assert_eq!(err.category, crate::error::ErrorCategory::Model);
    assert_eq!(err.retryable, Some(true));
    assert_eq!(
        err.message,
        "Model request timed out; current draft and selection were preserved"
    );
}

#[tokio::test]
async fn bidirectional_tool_call_roundtrip_and_cancellation_work() {
    let directory = tempfile::tempdir().unwrap();
    let tool_script = directory.path().join("tool-loop.mjs");
    std::fs::write(
        &tool_script,
        r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            let active = null;
            rl.on('line', (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(0); return; }
              if (cmd.type === 'cancel') {
                if (active) emit({ requestId: active.requestId, context: active.context, type: 'cancelled' });
                active = null;
                return;
              }
              if (cmd.type === 'toolResult') {
                if (active && cmd.callId === 'tc-1') {
                  emit({
                    requestId: active.requestId,
                    context: active.context,
                    type: 'completed',
                    text: `tool-reply:${cmd.content}`,
                  });
                  active = null;
                }
                return;
              }
              active = { requestId: cmd.requestId, context: cmd.context };
              emit({ requestId: cmd.requestId, context: cmd.context, type: 'started' });
              emit({
                requestId: cmd.requestId,
                context: cmd.context,
                type: 'toolCall',
                callId: 'tc-1',
                toolName: 'read_repo_file',
                args: { path: 'src/auth.ts' },
              });
            });
        "#,
    )
    .unwrap();

    let supervisor = PiSupervisor::default();
    let context = RequestContext {
        repository_id: "repo-1".into(),
        worktree_id: "wt-1".into(),
        session_id: "sess-1".into(),
        scope_version: "scope-1".into(),
    };
    let paths = PiPaths {
        node: PiPaths::development().node,
        script: tool_script,
    };
    let cmd = PiCommand::TestConnection {
        request_id: "req-tool-roundtrip".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let outcome = supervisor
        .run_task_with_tools(
            &paths,
            "req-tool-roundtrip".into(),
            context.clone(),
            &cmd,
            Duration::from_secs(5),
            None,
            |_| Ok(()),
            |call_id, tool_name, args| async move {
                assert_eq!(call_id, "tc-1");
                assert_eq!(tool_name, "read_repo_file");
                assert_eq!(args["path"], "src/auth.ts");
                PiToolExecutionResult::success("export const ok = true;")
            },
        )
        .await
        .unwrap();
    match outcome {
        PiTaskOutcome::Completed { text } => {
            assert_eq!(text, "tool-reply:export const ok = true;");
        }
        other => panic!("expected Completed, got {other:?}"),
    }

    // Verify cancellation while a tool handler is suspended waiting (e.g. ask_user_question).
    let cancel_sup = supervisor.clone();
    let cancel_cmd = PiCommand::TestConnection {
        request_id: "req-tool-cancel".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let cancelled_outcome = supervisor
        .run_task_with_tools(
            &paths,
            "req-tool-cancel".into(),
            context,
            &cancel_cmd,
            Duration::from_secs(5),
            None,
            |_| Ok(()),
            |_call_id, _tool_name, _args| {
                let sup = cancel_sup.clone();
                async move {
                    let _ = sup.cancel("req-tool-cancel");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    PiToolExecutionResult::success("unreachable")
                }
            },
        )
        .await
        .unwrap();
    assert!(matches!(cancelled_outcome, PiTaskOutcome::Cancelled));
}

#[tokio::test]
async fn tool_human_wait_pauses_total_timeout_large_frames_pass_and_explicit_errors_are_not_masked()
{
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("phase3-edge.mjs");
    std::fs::write(
        &script,
        r#"
            import { createInterface } from 'node:readline';
            const emit = (e) => process.stdout.write(JSON.stringify(e) + '\n');
            emit({ type: 'ready', protocolVersion: 1, piCoreVersion: '1.6.0', nodeVersion: process.versions.node, pid: process.pid });
            const rl = createInterface({ input: process.stdin });
            let active = null;
            rl.on('line', (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') { process.exit(0); return; }
              if (cmd.type === 'toolResult') {
                if (active && cmd.callId === 'tc-wait') {
                  emit({
                    requestId: active.requestId,
                    context: active.context,
                    type: 'completed',
                    text: `len=${cmd.content.length}`,
                  });
                  active = null;
                }
                return;
              }
              active = { requestId: cmd.requestId, context: cmd.context };
              emit({ requestId: cmd.requestId, context: cmd.context, type: 'started' });
              if (cmd.requestId === 'req-fail-exit') {
                emit({ requestId: cmd.requestId, context: cmd.context, type: 'failed', message: 'Quota exceeded (429)' });
                process.exit(1);
                return;
              }
              // Emit a >256 KiB payload inside toolCall args to verify the 2 MiB frame ceiling:
              emit({
                requestId: cmd.requestId,
                context: cmd.context,
                type: 'toolCall',
                callId: 'tc-wait',
                toolName: 'message_user',
                args: { blob: 'x'.repeat(300 * 1024) },
              });
            });
        "#,
    )
    .unwrap();

    let supervisor = PiSupervisor::default();
    let context = RequestContext {
        repository_id: "repo-p3".into(),
        worktree_id: "wt-p3".into(),
        session_id: "sess-p3".into(),
        scope_version: "scope-p3".into(),
    };
    let paths = PiPaths {
        node: PiPaths::development().node,
        script,
    };

    // 1. idle_timeout = 100ms -> total_ceiling = 800ms, but human wait sleeps 950ms and MUST still succeed!
    //    Also tests >300 KiB JSON frames in BOTH directions (toolCall + toolResult).
    let wait_cmd = PiCommand::TestConnection {
        request_id: "req-human-wait".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let large_reply = "y".repeat(320 * 1024);
    let outcome = supervisor
        .run_task_with_tools(
            &paths,
            "req-human-wait".into(),
            context.clone(),
            &wait_cmd,
            Duration::from_millis(100),
            None,
            |_| Ok(()),
            move |_call_id, tool_name, args| {
                let reply = large_reply.clone();
                async move {
                    assert_eq!(tool_name, "message_user");
                    assert_eq!(args["blob"].as_str().unwrap().len(), 300 * 1024);
                    tokio::time::sleep(Duration::from_millis(950)).await;
                    PiToolExecutionResult::success(reply)
                }
            },
        )
        .await
        .unwrap();
    match outcome {
        PiTaskOutcome::Completed { text } => assert_eq!(text, format!("len={}", 320 * 1024)),
        other => panic!("expected Completed after long human wait, got {other:?}"),
    }

    // 2. Explicit `Failed` event followed immediately by `process.exit(1)` must preserve business error:
    let fail_cmd = PiCommand::TestConnection {
        request_id: "req-fail-exit".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let err = supervisor
        .run_task(
            &paths,
            "req-fail-exit".into(),
            context,
            &fail_cmd,
            Duration::from_secs(5),
            None,
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(err.category, crate::error::ErrorCategory::Model);
    assert!(
        err.message.contains("Quota exceeded (429)"),
        "expected business error to be preserved over exit code, got: {err}"
    );
}

#[tokio::test]
async fn pre_cancel_and_handshake_cancel_settle_immediately_as_cancelled() {
    let context = RequestContext {
        repository_id: "repo-1".into(),
        worktree_id: "wt-1".into(),
        session_id: "sess-cancel".into(),
        scope_version: "scope-1".into(),
    };

    // 1. Pre-cancelling before run_task starts must short-circuit before spawning any child:
    let supervisor = PiSupervisor::default();
    supervisor.cancel_or_pre_cancel("req-pre-cancelled");
    let missing_paths = PiPaths {
        node: std::path::PathBuf::from("/nonexistent/picket-node"),
        script: std::path::PathBuf::from("/nonexistent/pi-sidecar.mjs"),
    };
    let pre_cmd = PiCommand::TestConnection {
        request_id: "req-pre-cancelled".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let outcome = supervisor
        .run_task(
            &missing_paths,
            "req-pre-cancelled".into(),
            context.clone(),
            &pre_cmd,
            Duration::from_secs(5),
            None,
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, PiTaskOutcome::Cancelled));

    // 2. Cancelling while sidecar is stuck before emitting `ready` must cancel immediately
    //    without waiting for START_TIMEOUT or returning an abnormal-exit error:
    let directory = tempfile::tempdir().unwrap();
    let silent_boot = directory.path().join("silent-boot.mjs");
    std::fs::write(
        &silent_boot,
        r#"
            import { createInterface } from 'node:readline';
            const rl = createInterface({ input: process.stdin });
            rl.on('line', (line) => {
              const cmd = JSON.parse(line);
              if (cmd.type === 'shutdown') process.exit(0);
            });
            setInterval(() => {}, 1000);
        "#,
    )
    .unwrap();
    let paths = PiPaths {
        node: PiPaths::development().node,
        script: silent_boot,
    };
    let handshake_cmd = PiCommand::TestConnection {
        request_id: "req-handshake-cancel".into(),
        context: context.clone(),
        model: sample_model(),
        locale: LanguageKind::En,
    };
    let cancel_sup = supervisor.clone();
    let cancel_task = tokio::spawn(async move {
        loop {
            if cancel_sup.has_active_requests() {
                tokio::time::sleep(Duration::from_millis(80)).await;
                cancel_sup.cancel_or_pre_cancel("req-handshake-cancel");
                break;
            }
            tokio::task::yield_now().await;
        }
    });
    let started = std::time::Instant::now();
    let outcome = supervisor
        .run_task(
            &paths,
            "req-handshake-cancel".into(),
            context,
            &handshake_cmd,
            Duration::from_secs(10),
            None,
            |_| Ok(()),
        )
        .await
        .unwrap();
    cancel_task.await.unwrap();
    assert!(matches!(outcome, PiTaskOutcome::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "handshake cancellation took too long: {:?}",
        started.elapsed()
    );
}
