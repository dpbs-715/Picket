use super::{
    discover::scan_project_manifest_paths,
    manager::{RunProcessManager, RunTargetContext, merge_run_process_env},
    process_io::read_bounded_lossy_lines,
    types::{
        MAX_LOG_LINE_BYTES, PersistedRunCommand, RunEnvEntry, RunLogStream, RunProcessStatus,
        extract_local_urls, make_process_id, normalize_run_cwd, normalize_run_group,
        strip_ansi_codes, validate_local_http_url, validate_run_commands, validate_run_env_entries,
    },
    user_env::{ResolvedUserEnv, parse_marked_env_bytes},
    workspace_scan::scan_workspace_modules,
};
use crate::{error::AppErrorCode, i18n::LanguageKind};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::Notify, time::sleep};

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    let Ok(raw) = i32::try_from(pid) else {
        return false;
    };
    if raw <= 0 {
        return false;
    }
    // SAFETY: Signal 0 only checks process existence without delivering a signal.
    unsafe { kill(raw, 0) == 0 }
}

fn test_user_env() -> ResolvedUserEnv {
    let mut vars = BTreeMap::new();
    vars.insert(
        "PATH".into(),
        "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin".into(),
    );
    vars.insert("PICKET_USER_MARKER".into(), "from-user-shell".into());
    ResolvedUserEnv {
        shell: PathBuf::from("/bin/sh"),
        vars,
        loaded_from_login_shell: true,
    }
}

fn make_ctx(root: &Path) -> RunTargetContext {
    RunTargetContext {
        repository_id: "repo-run-test".into(),
        repository_name: "demo".into(),
        worktree_id: "wt-run-test".into(),
        worktree_label: "demo".into(),
        worktree_root: root.to_path_buf(),
    }
}

#[test]
fn parses_marked_nul_delimited_user_env_ignoring_shell_banners_and_picket_internals() {
    let mut raw = Vec::new();
    raw.extend_from_slice(b"Welcome to zsh!\nSome noisy banner\n");
    raw.extend_from_slice(b"__PICKET_USER_ENV_START_9F2A__");
    raw.extend_from_slice(b"PATH=/Users/dev/.nvm/versions/node/v24/bin:/usr/bin\0");
    raw.extend_from_slice(b"MULTILINE=line1\nline2\0");
    raw.extend_from_slice(b"PICKET_DIST_DIR=dist-smoke\0");
    raw.extend_from_slice(b"SHLVL=2\0");
    raw.extend_from_slice(b"__PICKET_USER_ENV_END_9F2A__");
    raw.extend_from_slice(b"goodbye\n");

    let parsed = parse_marked_env_bytes(&raw).expect("expected valid env map");
    assert_eq!(
        parsed.get("PATH").map(String::as_str),
        Some("/Users/dev/.nvm/versions/node/v24/bin:/usr/bin")
    );
    assert_eq!(
        parsed.get("MULTILINE").map(String::as_str),
        Some("line1\nline2")
    );
    assert!(!parsed.contains_key("PICKET_DIST_DIR"));
    assert!(!parsed.contains_key("SHLVL"));
    assert!(!parsed.get("PATH").unwrap().contains("picket-node"));
}

#[test]
fn validates_run_cwd_local_urls_ansi_stripping_and_command_limits() {
    assert_eq!(normalize_run_cwd(None).unwrap(), None);
    assert_eq!(normalize_run_cwd(Some(".")).unwrap(), None);
    assert_eq!(
        normalize_run_cwd(Some("./apps/web/")).unwrap(),
        Some("apps/web".into())
    );
    assert!(normalize_run_cwd(Some("../escape")).is_err());
    assert!(normalize_run_cwd(Some("/etc")).is_err());
    assert!(normalize_run_cwd(Some(".git/hooks")).is_err());

    assert_eq!(
        strip_ansi_codes("\x1b[32m➜\x1b[0m  Local:   \x1b[36mhttp://localhost:5173/\x1b[0m\r"),
        "➜  Local:   http://localhost:5173/"
    );
    let urls = extract_local_urls(
        "Ready at http://localhost:1420/ and http://127.0.0.1:8080/health and https://evil.example.com",
    );
    assert_eq!(
        urls,
        vec![
            "http://localhost:1420/".to_string(),
            "http://127.0.0.1:8080/health".to_string()
        ]
    );

    assert!(validate_local_http_url("http://localhost:5173").is_ok());
    assert!(validate_local_http_url("http://127.0.0.1:3000/api").is_ok());
    assert!(validate_local_http_url("http://[::1]:4000").is_ok());
    assert!(validate_local_http_url("https://example.com").is_err());
    assert!(validate_local_http_url("http://localhost.evil.com:3000").is_err());
    assert!(validate_local_http_url("file:///etc/passwd").is_err());

    let valid = vec![PersistedRunCommand {
        id: "run-1".into(),
        label: "Dev".into(),
        command: "pnpm dev".into(),
        cwd: Some("apps/web".into()),
        group: Some("Spring Boot".into()),
        icon: Some("vite".into()),
        env: vec![RunEnvEntry {
            key: "PORT".into(),
            value: "3000".into(),
        }],
    }];
    assert!(validate_run_commands(&valid));
    assert_eq!(
        normalize_run_group(Some("  Maven  ")).unwrap(),
        Some("Maven".into())
    );
    assert_eq!(normalize_run_group(Some("   ")).unwrap(), None);
    assert!(normalize_run_group(Some("Bad\nGroup")).is_err());

    let invalid_icon = vec![PersistedRunCommand {
        id: "run-icon".into(),
        label: "Bad Icon".into(),
        command: "pnpm dev".into(),
        cwd: None,
        group: None,
        icon: Some("../evil".into()),
        env: vec![],
    }];
    assert!(!validate_run_commands(&invalid_icon));

    let invalid_env = vec![PersistedRunCommand {
        id: "run-1".into(),
        label: "Dev".into(),
        command: "pnpm dev".into(),
        cwd: None,
        group: None,
        icon: None,
        env: vec![RunEnvEntry {
            key: "1BAD-KEY".into(),
            value: "x".into(),
        }],
    }];
    assert!(!validate_run_commands(&invalid_env));

    assert!(validate_run_env_entries(&[RunEnvEntry {
        key: "JAVA_HOME".into(),
        value: "/Library/Java/jbr-25".into(),
    }]));
    assert!(!validate_run_env_entries(&[
        RunEnvEntry {
            key: "JAVA_HOME".into(),
            value: "/a".into(),
        },
        RunEnvEntry {
            key: "JAVA_HOME".into(),
            value: "/b".into(),
        },
    ]));
}

#[test]
fn merges_run_process_env_with_precedence_and_prepends_explicit_java_home_bin() {
    let mut user_vars = BTreeMap::new();
    user_vars.insert("PATH".into(), "/usr/bin:/bin:/opt/jbr-25/bin".into());
    user_vars.insert("JAVA_HOME".into(), "/opt/default-jdk".into());
    user_vars.insert("SPRING_PROFILES_ACTIVE".into(), "base".into());

    // Without explicit JAVA_HOME in shared or command env, PATH stays untouched:
    let unchanged = merge_run_process_env(&user_vars, &[], &[]);
    assert_eq!(
        unchanged.get("PATH").map(String::as_str),
        Some("/usr/bin:/bin:/opt/jbr-25/bin")
    );

    let shared_env = vec![
        RunEnvEntry {
            key: "JAVA_HOME".into(),
            value: "/opt/jbr-25/".into(),
        },
        RunEnvEntry {
            key: "SPRING_PROFILES_ACTIVE".into(),
            value: "dev".into(),
        },
        RunEnvEntry {
            key: "MAVEN_OPTS".into(),
            value: "-Xmx1024m".into(),
        },
    ];
    let command_env = vec![RunEnvEntry {
        key: "SPRING_PROFILES_ACTIVE".into(),
        value: "local".into(),
    }];

    let merged = merge_run_process_env(&user_vars, &shared_env, &command_env);
    assert_eq!(
        merged.get("JAVA_HOME").map(String::as_str),
        Some("/opt/jbr-25/")
    );
    assert_eq!(
        merged.get("SPRING_PROFILES_ACTIVE").map(String::as_str),
        Some("local")
    );
    assert_eq!(
        merged.get("MAVEN_OPTS").map(String::as_str),
        Some("-Xmx1024m")
    );
    // `<JAVA_HOME>/bin` is prepended and deduplicated from the trailing entry:
    assert_eq!(
        merged.get("PATH").map(String::as_str),
        Some("/opt/jbr-25/bin:/usr/bin:/bin")
    );
}

#[tokio::test]
async fn spawns_streams_logs_extracts_urls_enforces_single_instance_and_kills_process_tree() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("repo");
    let sub = work.join("web");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&sub).unwrap();
    fs::create_dir_all(&outside).unwrap();

    // Symlink escaping worktree must be rejected:
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, work.join("escape-dir")).unwrap();

    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;

    let escaped_err = manager
        .start_command(
            make_ctx(&work),
            PersistedRunCommand {
                id: "cmd-escape".into(),
                label: "Escape".into(),
                command: "echo nope".into(),
                cwd: Some("escape-dir".into()),
                group: None,
                icon: None,
                env: vec![],
            },
            vec![],
            LanguageKind::En,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(escaped_err.code, AppErrorCode::RunnerInvalidInput);

    // Spawn a shell that starts a grandchild `sleep 60` process and prints its PID + localhost URL:
    let grandchild_pid_file = work.join("grandchild.pid");
    let cmd_str = format!(
        "sleep 60 & child=$!; echo $child > '{}'; printf '\\033[32mLocal: http://localhost:5173/\\033[0m\\n'; echo \"USER_ENV=$PICKET_USER_MARKER SHARED=$SHARED_FLAG EXTRA=$CUSTOM_FLAG\"; wait $child",
        grandchild_pid_file.display()
    );
    let command = PersistedRunCommand {
        id: "cmd-dev".into(),
        label: "Frontend Dev".into(),
        command: cmd_str,
        cwd: Some("web".into()),
        group: Some("Frontend".into()),
        icon: Some("vite".into()),
        env: vec![RunEnvEntry {
            key: "CUSTOM_FLAG".into(),
            value: "enabled".into(),
        }],
    };
    let shared_env = vec![
        RunEnvEntry {
            key: "SHARED_FLAG".into(),
            value: "repo-shared".into(),
        },
        RunEnvEntry {
            key: "CUSTOM_FLAG".into(),
            value: "overridden-by-command".into(),
        },
    ];
    let started = manager
        .start_command(
            make_ctx(&work),
            command.clone(),
            shared_env.clone(),
            LanguageKind::En,
            None,
        )
        .await
        .unwrap();
    assert_eq!(started.status, RunProcessStatus::Running);
    assert_eq!(started.cwd, "web");
    assert!(manager.has_active_processes());
    assert_eq!(manager.tray_running_services().len(), 1);

    // Same worktree + commandId must enforce single-instance:
    let dup_err = manager
        .start_command(
            make_ctx(&work),
            command.clone(),
            shared_env,
            LanguageKind::En,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(dup_err.code, AppErrorCode::RunnerAlreadyRunning);

    // Wait for grandchild PID file and streamed logs:
    let mut grandchild_pid: Option<u32> = None;
    for _ in 0..40 {
        if let Ok(raw) = fs::read_to_string(&grandchild_pid_file)
            && let Ok(pid) = raw.trim().parse::<u32>()
        {
            grandchild_pid = Some(pid);
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    let grandchild_pid = grandchild_pid.expect("grandchild process should have written its PID");
    #[cfg(unix)]
    assert!(
        is_pid_alive(grandchild_pid),
        "grandchild process should be alive before stop"
    );

    // Wait for throttled log flush:
    let process_id = make_process_id("wt-run-test", "cmd-dev");
    let mut saw_logs = false;
    for _ in 0..30 {
        let reply = manager.get_logs(&process_id);
        let combined: String = reply
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if combined.contains("USER_ENV=from-user-shell SHARED=repo-shared EXTRA=enabled")
            && reply.process.as_ref().is_some_and(|p| {
                p.detected_urls
                    .contains(&"http://localhost:5173/".to_string())
                    && p.last_log_line.as_deref().is_some_and(|line| {
                        line.contains("USER_ENV=from-user-shell SHARED=repo-shared EXTRA=enabled")
                    })
            })
        {
            saw_logs = true;
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(
        saw_logs,
        "expected streamed logs with user env and detected URL"
    );

    // Stop the process and verify BOTH parent and grandchild processes are terminated cleanly:
    let stopped = manager
        .stop_process(&process_id, LanguageKind::En, None)
        .await
        .unwrap();
    assert_eq!(stopped.status, RunProcessStatus::Exited);
    assert!(!manager.has_active_processes());
    assert!(manager.tray_running_services().is_empty());

    #[cfg(unix)]
    {
        let mut dead = false;
        for _ in 0..20 {
            if !is_pid_alive(grandchild_pid) {
                dead = true;
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(
            dead,
            "grandchild process {grandchild_pid} must be terminated when process group is stopped"
        );
    }

    // Clear logs works cleanly:
    assert!(manager.clear_logs(&process_id, None));
    let after_clear = manager.get_logs(&process_id);
    assert!(after_clear.lines.is_empty());
    assert!(after_clear.process.and_then(|p| p.last_log_line).is_none());
}

#[tokio::test]
async fn shutdown_all_cleanly_stops_multiple_concurrent_processes() {
    let temp = tempfile::tempdir().unwrap();
    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;

    for idx in 1..=2 {
        let cmd = PersistedRunCommand {
            id: format!("svc-{idx}"),
            label: format!("Service {idx}"),
            command: "sleep 60".into(),
            cwd: None,
            group: None,
            icon: None,
            env: vec![],
        };
        manager
            .start_command(make_ctx(temp.path()), cmd, vec![], LanguageKind::En, None)
            .await
            .unwrap();
    }
    assert_eq!(manager.active_snapshots().len(), 2);

    manager.shutdown_all().await;
    assert!(!manager.has_active_processes());
    assert!(manager.active_snapshots().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn force_kills_entire_process_group_including_sigterm_ignoring_grandchild() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("repo");
    fs::create_dir_all(&work).unwrap();

    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;
    manager.set_stop_grace_for_test(Duration::from_millis(200));

    // Parent shell exits immediately on SIGTERM, while grandchild traps and ignores SIGTERM.
    // `stop_process` must NOT stop early when the leader exits: it must detect the surviving
    // process-group member and escalate to SIGKILL on `-pgid`.
    let grandchild_pid_file = work.join("stubborn-grandchild.pid");
    let cmd_str = format!(
        "sh -c \"trap '' TERM; sleep 60\" & child=$!; echo $child > '{}'; trap 'exit 0' TERM; while true; do sleep 1; done",
        grandchild_pid_file.display()
    );
    let command = PersistedRunCommand {
        id: "cmd-stubborn".into(),
        label: "Stubborn Group".into(),
        command: cmd_str,
        cwd: None,
        group: None,
        icon: None,
        env: vec![],
    };
    manager
        .start_command(make_ctx(&work), command, vec![], LanguageKind::En, None)
        .await
        .unwrap();

    let mut grandchild_pid: Option<u32> = None;
    for _ in 0..40 {
        if let Ok(raw) = fs::read_to_string(&grandchild_pid_file)
            && let Ok(pid) = raw.trim().parse::<u32>()
        {
            grandchild_pid = Some(pid);
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    let grandchild_pid = grandchild_pid.expect("stubborn grandchild should write its PID");
    assert!(is_pid_alive(grandchild_pid));

    let process_id = make_process_id("wt-run-test", "cmd-stubborn");
    let stopped = manager
        .stop_process(&process_id, LanguageKind::En, None)
        .await
        .unwrap();
    assert_eq!(stopped.status, RunProcessStatus::Exited);
    assert!(!manager.has_active_processes());
    assert!(
        !is_pid_alive(grandchild_pid),
        "SIGTERM-ignoring grandchild {grandchild_pid} must be killed via SIGKILL to -pgid"
    );

    let logs = manager.get_logs(&process_id);
    assert!(
        logs.lines.iter().any(|l| l.text.contains("SIGKILL")),
        "expected SIGKILL escalation log entry when grandchild ignored SIGTERM"
    );
}

#[tokio::test]
async fn cancels_starting_process_immediately_without_spawning_child() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("repo");
    fs::create_dir_all(&work).unwrap();
    let marker = work.join("must-not-exist.marker");

    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;
    let barrier = Arc::new(Notify::new());
    manager
        .user_env_cache()
        .set_load_barrier_for_test(Some(Arc::clone(&barrier)));

    let command = PersistedRunCommand {
        id: "cmd-cancel-start".into(),
        label: "Slow Starting Service".into(),
        command: format!("touch '{}'; sleep 60", marker.display()),
        cwd: None,
        group: None,
        icon: None,
        env: vec![],
    };

    let start_manager = manager.clone();
    let start_work = work.clone();
    let start_handle = tokio::spawn(async move {
        start_manager
            .start_command(
                make_ctx(&start_work),
                command,
                vec![],
                LanguageKind::En,
                None,
            )
            .await
    });

    let process_id = make_process_id("wt-run-test", "cmd-cancel-start");
    let mut saw_starting = false;
    for _ in 0..40 {
        if manager
            .list_snapshots()
            .iter()
            .any(|p| p.process_id == process_id && p.status == RunProcessStatus::Starting)
        {
            saw_starting = true;
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        saw_starting,
        "process must enter Starting state before cancel"
    );

    let t0 = Instant::now();
    let stopped = manager
        .stop_process(&process_id, LanguageKind::En, None)
        .await
        .unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "cancelling during Starting must return immediately, took {:?}",
        t0.elapsed()
    );
    assert_eq!(stopped.status, RunProcessStatus::Exited);
    assert!(!manager.has_active_processes());

    let start_result = start_handle.await.unwrap().unwrap();
    assert_eq!(start_result.status, RunProcessStatus::Exited);

    // Releasing the env-load barrier later must NEVER spawn a belated ghost process:
    barrier.notify_waiters();
    sleep(Duration::from_millis(100)).await;
    assert!(
        !marker.exists(),
        "cancelled startup must never spawn the child command"
    );
    assert!(!manager.has_active_processes());
}

#[tokio::test]
async fn tolerates_invalid_utf8_and_caps_overlong_log_lines_without_aborting_stream() {
    // 1. strip_ansi_codes must never panic when ESC precedes a multi-byte UTF-8 or U+FFFD char:
    assert_eq!(strip_ansi_codes("\x1b中\x1b\u{FFFD}ok"), "中\u{FFFD}ok");

    // 2. Unit check: bounded lossy reader trims incomplete trailing UTF-8 at truncation boundary
    //    without emitting spurious U+FFFD, while preserving real invalid bytes as U+FFFD:
    let mut payload = Vec::new();
    payload.extend_from_slice(b"hello \xff\xfe world\r\n");
    // 5 bytes of valid UTF-8 ("a中b"): "a" (1) + "中" (3 bytes: E4 B8 AD) + "b" (1).
    // With cap = 3, bytes[..3] is [b'a', 0xE4, 0xB8] -> trims incomplete "中" cleanly to "a"!
    payload.extend_from_slice("a中bcdef\n".as_bytes());
    payload.extend_from_slice(b"tail\n");

    let mut unit_lines = Vec::new();
    read_bounded_lossy_lines(payload.as_slice(), 3, |line| {
        unit_lines.push(line);
        async { true }
    })
    .await;
    assert_eq!(unit_lines.len(), 3);
    assert!(unit_lines[0].starts_with("hel") && unit_lines[0].contains("[truncated]"));
    assert_eq!(unit_lines[1], "a … [truncated]");
    assert!(unit_lines[2].starts_with("tai") && unit_lines[2].contains("[truncated]"));

    let mut invalid_lines = Vec::new();
    read_bounded_lossy_lines(b"ok \xff\xfe end\nnext\n".as_slice(), 64, |line| {
        invalid_lines.push(line);
        async { true }
    })
    .await;
    assert_eq!(invalid_lines, vec!["ok \u{FFFD}\u{FFFD} end", "next"]);

    // 3. End-to-end subprocess check through RunProcessManager:
    //    invalid UTF-8 + ESC-followed-by-invalid-byte + 40 KiB overlong line + trailing URL line.
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("repo");
    fs::create_dir_all(&work).unwrap();
    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;

    let command = PersistedRunCommand {
        id: "cmd-binary-logs".into(),
        label: "Binary & Long Logs".into(),
        command: concat!(
            "printf 'BEFORE_BAD \\377\\376 \\033\\377 AFTER_BAD\\n'; ",
            "awk 'BEGIN{for(i=0;i<40000;i++) printf \"A\"; printf \"\\n\"}'; ",
            "printf 'AFTER_OVERLONG http://localhost:5174/\\n'"
        )
        .into(),
        cwd: None,
        group: None,
        icon: None,
        env: vec![],
    };
    manager
        .start_command(make_ctx(&work), command, vec![], LanguageKind::En, None)
        .await
        .unwrap();

    let process_id = make_process_id("wt-run-test", "cmd-binary-logs");
    let mut finished = false;
    for _ in 0..60 {
        let reply = manager.get_logs(&process_id);
        if reply
            .process
            .as_ref()
            .is_some_and(|p| p.status == RunProcessStatus::Exited)
            && reply
                .lines
                .iter()
                .any(|l| l.text.contains("AFTER_OVERLONG"))
        {
            finished = true;
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(
        finished,
        "subprocess logs must keep streaming after invalid UTF-8 and 40 KiB line"
    );

    let reply = manager.get_logs(&process_id);
    let bad_line = reply
        .lines
        .iter()
        .find(|l| l.stream == RunLogStream::Stdout && l.text.contains("BEFORE_BAD"))
        .expect("invalid UTF-8 stdout line must be preserved with replacement chars");
    assert!(bad_line.text.contains('\u{FFFD}'));
    assert!(bad_line.text.contains("AFTER_BAD"));

    let long_line = reply
        .lines
        .iter()
        .find(|l| l.stream == RunLogStream::Stdout && l.text.starts_with("AAAA"))
        .expect("overlong stdout line must be emitted with truncation marker");
    assert!(long_line.text.contains("[truncated]"));
    assert!(
        long_line.text.len() <= MAX_LOG_LINE_BYTES + 32,
        "overlong line must be capped at MAX_LOG_LINE_BYTES (got {} bytes)",
        long_line.text.len()
    );

    let proc = reply.process.expect("process snapshot must exist");
    assert!(
        proc.detected_urls
            .contains(&"http://localhost:5174/".to_string()),
        "URL printed after invalid UTF-8 and overlong line must still be extracted"
    );
}

#[test]
fn scans_project_manifests_scripts_and_monorepo_apps_while_skipping_secrets_and_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let outside = temp.path().join("outside");
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::create_dir_all(root.join("apps/web")).unwrap();
    fs::create_dir_all(root.join("src-tauri")).unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::create_dir_all(&outside).unwrap();

    fs::write(root.join("package.json"), "{}").unwrap();
    fs::write(root.join(".env"), "SECRET=1").unwrap();
    fs::write(root.join("scripts/desktop.mjs"), "").unwrap();
    fs::write(root.join("apps/web/package.json"), "{}").unwrap();
    fs::write(root.join("src-tauri/Cargo.toml"), "").unwrap();
    fs::write(root.join("node_modules/pkg/package.json"), "{}").unwrap();
    fs::write(outside.join("Makefile"), "").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("Makefile"), root.join("Makefile")).unwrap();

    let found = scan_project_manifest_paths(&root);
    assert!(found.contains(&"package.json".to_string()));
    assert!(found.contains(&"scripts/desktop.mjs".to_string()));
    assert!(found.contains(&"apps/web/package.json".to_string()));
    assert!(found.contains(&"src-tauri/Cargo.toml".to_string()));
    assert!(!found.iter().any(|p| p.contains(".env")));
    assert!(!found.iter().any(|p| p.contains("node_modules")));
    assert!(!found.contains(&"Makefile".to_string()));
}

#[test]
fn scans_nested_spring_cloud_maven_and_pnpm_workspaces_without_leaking_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("scm-service");
    let outside = temp.path().join("outside-mod");
    fs::create_dir_all(root.join("scm-common/src/main/java/com/acme")).unwrap();
    fs::create_dir_all(
        root.join("scm-modules/scm-business-starter/src/main/java/com/acme/business"),
    )
    .unwrap();
    fs::create_dir_all(root.join("scm-modules/scm-business-starter/src/main/resources")).unwrap();
    fs::create_dir_all(root.join("scm-modules/scm-alarm-starter/src/main/java/com/acme/alarm"))
        .unwrap();
    fs::create_dir_all(&outside).unwrap();

    fs::write(root.join("mvnw"), "#!/bin/sh\n").unwrap();
    fs::write(
        root.join("pom.xml"),
        r#"<project>
  <artifactId>inspur-scm-service</artifactId>
  <packaging>pom</packaging>
  <modules>
    <module>scm-common</module>
    <module>scm-modules</module>
    <!-- <module>disabled-module</module> -->
    <module>../outside-mod</module>
  </modules>
  <profiles>
    <profile><id>dev</id></profile>
    <profile><id>prod</id></profile>
  </profiles>
</project>"#,
    )
    .unwrap();
    fs::write(
        root.join("scm-common/pom.xml"),
        r#"<project>
  <parent><artifactId>inspur-scm-service</artifactId></parent>
  <artifactId>scm-common</artifactId>
</project>"#,
    )
    .unwrap();
    fs::write(
        root.join("scm-modules/pom.xml"),
        r#"<project>
  <parent><artifactId>inspur-scm-service</artifactId></parent>
  <artifactId>scm-modules</artifactId>
  <packaging>pom</packaging>
  <modules>
    <module>scm-business-starter</module>
    <module>scm-alarm-starter</module>
  </modules>
</project>"#,
    )
    .unwrap();
    fs::write(
        root.join("scm-modules/scm-business-starter/pom.xml"),
        r#"<project>
  <parent><artifactId>scm-modules</artifactId></parent>
  <artifactId>inspur-scm-business-starter</artifactId>
</project>"#,
    )
    .unwrap();
    fs::write(
        root.join("scm-modules/scm-business-starter/src/main/java/com/acme/business/BusinessServerApplication.java"),
        "@SpringBootApplication\npublic class BusinessServerApplication {}\n",
    )
    .unwrap();
    fs::write(
        root.join("scm-modules/scm-business-starter/src/main/resources/bootstrap.yml"),
        "server:\n  port: ${SERVER_PORT:8082}\nspring:\n  application:\n    name: scm-business\n  datasource:\n    password: super-secret-db-password\n",
    )
    .unwrap();
    fs::write(
        root.join("scm-modules/scm-alarm-starter/pom.xml"),
        r#"<project>
  <parent><artifactId>scm-modules</artifactId></parent>
  <artifactId>inspur-scm-alarm-starter</artifactId>
  <build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build>
</project>"#,
    )
    .unwrap();
    fs::write(
        root.join(
            "scm-modules/scm-alarm-starter/src/main/java/com/acme/alarm/AlarmApplication.java",
        ),
        "@SpringBootApplication\npublic class AlarmApplication {}\n",
    )
    .unwrap();
    fs::write(
        outside.join("pom.xml"),
        "<project><artifactId>evil-outside</artifactId></project>",
    )
    .unwrap();

    let modules = scan_workspace_modules(&root);
    let joined = modules.join("\n");
    assert!(
        joined.contains(
            "maven-root: path=. artifact=inspur-scm-service wrapper=./mvnw totalModules=4 runnableServices=2 libraryModules=1 profiles=dev,prod"
        ),
        "unexpected maven-root output:\n{joined}"
    );
    assert!(joined.contains("module=scm-modules/scm-business-starter"));
    assert!(joined.contains("mainClass=BusinessServerApplication"));
    assert!(joined.contains("appName=scm-business"));
    assert!(joined.contains("port=8082"));
    assert!(joined.contains("module=scm-modules/scm-alarm-starter"));
    assert!(joined.contains("mainClass=AlarmApplication"));
    assert!(!joined.contains("scm-common"));
    assert!(!joined.contains("disabled-module"));
    assert!(!joined.contains("evil-outside"));
    assert!(!joined.contains("super-secret-db-password"));

    // Verify pnpm workspace scanning:
    let node_root = temp.path().join("mono");
    fs::create_dir_all(node_root.join("apps/web")).unwrap();
    fs::create_dir_all(node_root.join("apps/api")).unwrap();
    fs::write(
        node_root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'apps/*'\n",
    )
    .unwrap();
    fs::write(
        node_root.join("apps/web/package.json"),
        r#"{"name":"@mono/web","scripts":{"dev":"vite","build":"vite build"}}"#,
    )
    .unwrap();
    fs::write(
        node_root.join("apps/api/package.json"),
        r#"{"name":"@mono/api","scripts":{"start":"node server.js"}}"#,
    )
    .unwrap();
    let node_modules = scan_workspace_modules(&node_root).join("\n");
    assert!(node_modules.contains("node-workspace-root: path=. manager=pnpm packages=2"));
    assert!(
        node_modules.contains("node-workspace-pkg: path=apps/web name=@mono/web scripts=build,dev")
    );
    assert!(
        node_modules.contains("node-workspace-pkg: path=apps/api name=@mono/api scripts=start")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn manual_stop_preserves_pre_stop_preview_and_clears_shutdown_exit_code() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("repo");
    fs::create_dir_all(&work).unwrap();

    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;

    // 1. Reproduce pnpm/npm shutdown noise on SIGTERM: emit normal ready output, then trap TERM
    //    to print `ELIFECYCLE Command failed.` to stderr and exit 1.
    let stop_cmd = PersistedRunCommand {
        id: "cmd-pnpm-stop".into(),
        label: "Playground dev".into(),
        command: "trap 'echo \"ELIFECYCLE Command failed.\" >&2; exit 1' TERM; echo 'Local: http://localhost:5173/'; while true; do sleep 1; done".into(),
        cwd: None,
        group: None,
        icon: None,
        env: vec![],
    };
    manager
        .start_command(make_ctx(&work), stop_cmd, vec![], LanguageKind::En, None)
        .await
        .unwrap();

    let stop_pid = make_process_id("wt-run-test", "cmd-pnpm-stop");
    let mut saw_ready = false;
    for _ in 0..40 {
        if manager
            .get_logs(&stop_pid)
            .process
            .as_ref()
            .and_then(|p| p.last_log_line.as_deref())
            .is_some_and(|line| line.contains("http://localhost:5173/"))
        {
            saw_ready = true;
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(saw_ready, "expected ready preview line before manual stop");

    let stopped = manager
        .stop_process(&stop_pid, LanguageKind::En, None)
        .await
        .unwrap();
    assert_eq!(stopped.status, RunProcessStatus::Exited);
    assert_eq!(
        stopped.exit_code, None,
        "manual stop must clear non-zero teardown exit code so UI shows Stopped instead of Exited (1)"
    );

    // Wait briefly to confirm any late-draining stderr output does not pollute `last_log_line`:
    let mut saw_teardown_in_full_logs = false;
    for _ in 0..30 {
        let reply = manager.get_logs(&stop_pid);
        if reply
            .lines
            .iter()
            .any(|l| l.text.contains("ELIFECYCLE Command failed."))
        {
            saw_teardown_in_full_logs = true;
            break;
        }
        sleep(Duration::from_millis(30)).await;
    }
    assert!(
        saw_teardown_in_full_logs,
        "full log stream must still preserve raw shutdown stderr output"
    );

    let after_drain = manager.get_logs(&stop_pid);
    let preview = after_drain
        .process
        .as_ref()
        .and_then(|p| p.last_log_line.as_deref())
        .unwrap_or("");
    assert!(
        preview.contains("http://localhost:5173/") && !preview.contains("ELIFECYCLE"),
        "card preview must keep pre-stop runtime line instead of teardown noise, got: {preview:?}"
    );
    assert!(
        after_drain
            .lines
            .iter()
            .any(|l| l.text.contains("Process exited with code 1.")),
        "system log line must still record the raw exit code for debugging"
    );

    // 2. Genuine unrequested failure must still surface `Failed (1)` and `ELIFECYCLE Command failed.`:
    let crash_cmd = PersistedRunCommand {
        id: "cmd-pnpm-crash".into(),
        label: "Broken dev".into(),
        command: "echo 'ELIFECYCLE Command failed.' >&2; exit 1".into(),
        cwd: None,
        group: None,
        icon: None,
        env: vec![],
    };
    manager
        .start_command(make_ctx(&work), crash_cmd, vec![], LanguageKind::En, None)
        .await
        .unwrap();

    let crash_pid = make_process_id("wt-run-test", "cmd-pnpm-crash");
    let mut failed_snap = None;
    for _ in 0..40 {
        if let Some(proc) = manager.get_logs(&crash_pid).process
            && !proc.status.is_active()
        {
            failed_snap = Some(proc);
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    let failed_snap = failed_snap.expect("crashing command should finish promptly");
    assert_eq!(failed_snap.status, RunProcessStatus::Failed);
    assert_eq!(failed_snap.exit_code, Some(1));
    assert!(
        failed_snap
            .last_log_line
            .as_deref()
            .is_some_and(|line| line.contains("ELIFECYCLE Command failed.")),
        "genuine crash must keep its error preview, got: {:?}",
        failed_snap.last_log_line
    );
}

#[tokio::test]
async fn validates_signed_port_bare_ipv6_dot_git_symlink_and_single_truncation_suffix() {
    assert!(validate_local_http_url("http://localhost:+8080").is_err());
    assert!(validate_local_http_url("http://127.0.0.1:+3000").is_err());
    assert!(validate_local_http_url("http://[::1]").is_ok());

    let ipv6_urls = extract_local_urls("Listening on http://[::1] and http://[::1]:3000/api");
    assert_eq!(
        ipv6_urls,
        vec![
            "http://[::1]".to_string(),
            "http://[::1]:3000/api".to_string(),
        ]
    );

    // cap_log_text must not double-append truncation marker:
    let already_capped = format!("{} … [truncated]", "a".repeat(MAX_LOG_LINE_BYTES + 200));
    let capped = super::process_io::cap_log_text(already_capped, MAX_LOG_LINE_BYTES);
    assert_eq!(capped.matches("[truncated]").count(), 1);

    // Symlink inside worktree pointing into `.git` must be rejected by start_command:
    #[cfg(unix)]
    {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("repo");
        fs::create_dir_all(work.join(".git/hooks")).unwrap();
        std::os::unix::fs::symlink(work.join(".git"), work.join("sneaky-git")).unwrap();

        let manager = RunProcessManager::default();
        manager.user_env_cache().set_for_test(test_user_env()).await;
        let err = manager
            .start_command(
                make_ctx(&work),
                PersistedRunCommand {
                    id: "cmd-git-link".into(),
                    label: "Sneaky".into(),
                    command: "pwd".into(),
                    cwd: Some("sneaky-git".into()),
                    group: None,
                    icon: None,
                    env: vec![],
                },
                vec![],
                LanguageKind::En,
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, AppErrorCode::RunnerInvalidInput);
    }
}

#[tokio::test]
async fn restart_validates_before_stopping_increments_generation_and_shutdown_blocks_new_starts() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("repo");
    fs::create_dir_all(&work).unwrap();

    let manager = RunProcessManager::default();
    manager.user_env_cache().set_for_test(test_user_env()).await;

    let cmd = PersistedRunCommand {
        id: "cmd-gen".into(),
        label: "Gen Service".into(),
        command: "sleep 60".into(),
        cwd: None,
        group: None,
        icon: None,
        env: vec![],
    };
    let first = manager
        .start_command(make_ctx(&work), cmd.clone(), vec![], LanguageKind::En, None)
        .await
        .unwrap();
    assert_eq!(first.generation, 1);
    assert!(manager.has_active_processes());

    // Invalid restart input must fail BEFORE stopping the running gen-1 instance:
    let bad_restart = manager
        .restart_command(
            make_ctx(&work),
            PersistedRunCommand {
                cwd: Some("../outside".into()),
                ..cmd.clone()
            },
            vec![],
            LanguageKind::En,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(bad_restart.code, AppErrorCode::RunnerInvalidInput);
    let still_running = manager
        .list_snapshots()
        .into_iter()
        .find(|p| p.command_id == "cmd-gen")
        .unwrap();
    assert_eq!(still_running.status, RunProcessStatus::Running);
    assert_eq!(still_running.generation, 1);

    // Valid restart increments generation to 2:
    let second = manager
        .restart_command(make_ctx(&work), cmd.clone(), vec![], LanguageKind::En, None)
        .await
        .unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(second.status, RunProcessStatus::Running);

    // shutdown_all sets the closing gate and rejects subsequent start_command calls:
    manager.shutdown_all().await;
    assert!(!manager.has_active_processes());
    let after_shutdown = manager
        .start_command(make_ctx(&work), cmd, vec![], LanguageKind::En, None)
        .await
        .unwrap_err();
    assert_eq!(after_shutdown.code, AppErrorCode::RunnerFailed);
}

#[test]
fn workspace_scan_handles_self_closing_xml_blocks_symlinks_and_rejects_absolute_globs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let outside = temp.path().join("outside");
    fs::create_dir_all(root.join("svc-a/src/main/java/com/acme")).unwrap();
    fs::create_dir_all(outside.join("secret-res")).unwrap();
    fs::create_dir_all(outside.join("go-escape")).unwrap();
    fs::write(
        outside.join("secret-res/application.yml"),
        "spring:\n  application:\n    name: leaked-app\nserver:\n  port: 9999\n",
    )
    .unwrap();
    fs::write(outside.join("go-escape/main.go"), "package main\n").unwrap();

    // Maven root pom with self-closing `<dependencies/>` BEFORE `<artifactId>` and `<modules>`:
    fs::write(
        root.join("pom.xml"),
        r#"<project>
  <dependencies/>
  <artifactId>self-closing-root</artifactId>
  <packaging>pom</packaging>
  <modules><module>svc-a</module></modules>
</project>"#,
    )
    .unwrap();
    fs::write(
        root.join("svc-a/pom.xml"),
        "<project><dependencies/><artifactId>svc-a</artifactId><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>",
    )
    .unwrap();
    fs::write(
        root.join("svc-a/src/main/java/com/acme/SvcApp.java"),
        "@SpringBootApplication\npublic class SvcApp {}\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.join("secret-res"),
        root.join("svc-a/src/main/resources"),
    )
    .unwrap();

    // Node workspace with malicious absolute glob `/*` or `/etc/*`:
    fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - '/etc/*'\n  - '~/secret/*'\n",
    )
    .unwrap();

    // Go workspace with symlinked cmd child escaping worktree:
    fs::create_dir_all(root.join("go-mod/cmd")).unwrap();
    fs::write(root.join("go.work"), "go 1.22\nuse ./go-mod\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("go-escape"), root.join("go-mod/cmd/escaped")).unwrap();

    let joined = scan_workspace_modules(&root).join("\n");
    assert!(
        joined.contains("artifact=self-closing-root"),
        "self-closing <dependencies/> must not truncate pom.xml:\n{joined}"
    );
    assert!(joined.contains("module=svc-a"));
    assert!(joined.contains("mainClass=SvcApp"));
    assert!(!joined.contains("leaked-app"));
    assert!(!joined.contains("port=9999"));
    assert!(!joined.contains("cmd/escaped"));
    assert!(!joined.contains("node-workspace-root"));
}
