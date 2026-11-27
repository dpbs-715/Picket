//! Controlled discrete-argv execution for `run_git`, `run_platform_cli`, and `run_repo_command`,
//! with RAII temp Markdown body files (zero shell interpolation), strict output caps, token
//! redaction, and automatic GitHub/GitLab/Actions URL extraction.

use super::{
    cli_env::{build_augmented_path, redact_sensitive_cli_output, resolve_cli_binary},
    policy::validate_repo_command_target,
    types::CustomActionLink,
};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::NamedTempFile;
use tokio::{io::AsyncReadExt, process::Command};

const CLI_EXEC_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_REPO_CMD_TIMEOUT_SECS: u64 = 90;
const MIN_REPO_CMD_TIMEOUT_SECS: u64 = 5;
const MAX_REPO_CMD_TIMEOUT_SECS: u64 = 180;
const MAX_CLI_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_RETURNED_CHARS: usize = 24_000;

#[derive(Clone, Debug)]
pub struct ControlledCliOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub extracted_links: Vec<CustomActionLink>,
}

pub fn extract_platform_links(text: &str) -> Vec<CustomActionLink> {
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | ',' | ';'
            )
        });
        if !(cleaned.starts_with("https://") || cleaned.starts_with("http://")) {
            continue;
        }
        let lower = cleaned.to_ascii_lowercase();
        let label = if let Some(num) = lower
            .rsplit_once("/pull/")
            .map(|(_, tail)| tail.split(&['/', '#', '?'][..]).next().unwrap_or(""))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            format!("PR #{num}")
        } else if let Some(num) = lower
            .rsplit_once("/merge_requests/")
            .map(|(_, tail)| tail.split(&['/', '#', '?'][..]).next().unwrap_or(""))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            format!("MR !{num}")
        } else if let Some(num) = lower
            .rsplit_once("/issues/")
            .map(|(_, tail)| tail.split(&['/', '#', '?'][..]).next().unwrap_or(""))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            format!("Issue #{num}")
        } else if lower.contains("/releases/tag/") {
            let tag = cleaned
                .rsplit_once("/releases/tag/")
                .map(|(_, t)| t.split(&['#', '?'][..]).next().unwrap_or("Release"))
                .unwrap_or("Release");
            format!("Release {tag}")
        } else if let Some(num) = lower
            .rsplit_once("/actions/runs/")
            .map(|(_, tail)| tail.split(&['/', '#', '?'][..]).next().unwrap_or(""))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            format!("Actions #{num}")
        } else if let Some(wf) = cleaned
            .rsplit_once("/actions/workflows/")
            .map(|(_, tail)| tail.split(&['/', '#', '?'][..]).next().unwrap_or(""))
            .filter(|name| !name.is_empty())
        {
            format!("Workflow {wf}")
        } else if let Some(num) = lower
            .rsplit_once("/-/pipelines/")
            .map(|(_, tail)| tail.split(&['/', '#', '?'][..]).next().unwrap_or(""))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            format!("Pipeline #{num}")
        } else {
            continue;
        };
        if seen.insert(cleaned.to_owned()) {
            links.push(CustomActionLink {
                label,
                url: cleaned.to_owned(),
            });
        }
    }
    links
}

fn truncate_text(raw: String) -> String {
    if raw.chars().count() <= MAX_RETURNED_CHARS {
        return raw;
    }
    let head: String = raw.chars().take(MAX_RETURNED_CHARS).collect();
    format!("{head}\n… (output truncated)")
}

pub async fn exec_controlled_git(
    worktree_root: &Path,
    args: &[String],
    allow_remote_protocol: bool,
    read_only: bool,
) -> Result<ControlledCliOutput, String> {
    let mut cmd = Command::new("/usr/bin/git");
    cmd.current_dir(worktree_root)
        .args(["--no-pager", "--no-optional-locks", "-c", "color.ui=false"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if read_only {
        cmd.args(["-c", "core.hooksPath=/dev/null"]);
    }
    crate::git_env::strip_git_overrides(&mut cmd, allow_remote_protocol);
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", ":")
        .env(
            "GIT_ALLOW_PROTOCOL",
            if allow_remote_protocol {
                "file:https:http:ssh:git"
            } else {
                ""
            },
        );
    cmd.args(args);

    run_child_bounded(cmd, "git", CLI_EXEC_TIMEOUT).await
}

pub async fn exec_controlled_platform_cli(
    cli: &str,
    worktree_root: &Path,
    args: &[String],
    body_text: Option<&str>,
    body_flag: Option<&str>,
    override_dirs: Option<&[PathBuf]>,
) -> Result<ControlledCliOutput, String> {
    let binary = resolve_cli_binary(cli, override_dirs).ok_or_else(|| {
        format!(
            "`{cli}` CLI was not found on this machine. Run `check_cli_env` first and offer installation via `message_user`."
        )
    })?;

    // Optional RAII temp Markdown file so multiline issue/PR bodies never touch a shell.
    let mut final_args = args.to_vec();
    let _temp_body: Option<NamedTempFile> = if let Some(text) = body_text {
        let flag = body_flag
            .map(str::trim)
            .filter(|f| {
                matches!(
                    *f,
                    "--body-file" | "--description-file" | "-F" | "--notes-file"
                )
            })
            .unwrap_or(if cli == "gh" {
                "--body-file"
            } else {
                "--description-file"
            });
        let mut temp = tempfile::Builder::new()
            .prefix("picket-cli-body-")
            .suffix(".md")
            .tempfile()
            .map_err(|err| format!("Failed to create temporary Markdown body file: {err}"))?;
        temp.write_all(text.as_bytes())
            .map_err(|err| format!("Failed to write temporary Markdown body file: {err}"))?;
        temp.flush()
            .map_err(|err| format!("Failed to flush temporary Markdown body file: {err}"))?;
        final_args.push(flag.to_owned());
        final_args.push(temp.path().to_string_lossy().into_owned());
        Some(temp)
    } else {
        None
    };

    let mut cmd = Command::new(&binary);
    cmd.current_dir(worktree_root)
        .args(&final_args)
        .env("PATH", build_augmented_path(override_dirs))
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    run_child_bounded(cmd, cli, CLI_EXEC_TIMEOUT).await
}

fn is_executable_regular_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

fn resolve_repo_command_binary(
    worktree_root: &Path,
    clean_command: &str,
    bundled_node_path: Option<&Path>,
    override_dirs: Option<&[PathBuf]>,
) -> Result<PathBuf, String> {
    if let Some(rel) = clean_command.strip_prefix("./") {
        let canon_root = fs::canonicalize(worktree_root)
            .map_err(|_| "Cannot verify worktree directory for command execution".to_owned())?;
        let candidate = canon_root.join(rel);
        let resolved = fs::canonicalize(&candidate)
            .map_err(|_| format!("Worktree executable not found: `{clean_command}`"))?;
        if !resolved.starts_with(&canon_root) {
            return Err(format!(
                "Command `{clean_command}` resolves outside the active repository worktree."
            ));
        }
        if !is_executable_regular_file(&resolved) {
            return Err(format!(
                "Target `{clean_command}` is not an executable file inside the worktree."
            ));
        }
        return Ok(resolved);
    }

    if let Some(found) = resolve_cli_binary(clean_command, override_dirs) {
        return Ok(found);
    }
    if clean_command == "node"
        && let Some(node_path) = bundled_node_path
        && is_executable_regular_file(node_path)
    {
        return Ok(node_path.to_path_buf());
    }
    Err(format!(
        "Command `{clean_command}` was not found on PATH (checked system PATH, Homebrew, ~/.local/bin, and ~/.cargo/bin)."
    ))
}

pub async fn exec_controlled_repo_command(
    worktree_root: &Path,
    command: &str,
    args: &[String],
    bundled_node_path: Option<&Path>,
    override_dirs: Option<&[PathBuf]>,
    timeout_secs: Option<u64>,
) -> Result<ControlledCliOutput, String> {
    let clean = validate_repo_command_target(command).map_err(|(_, reason)| reason)?;
    let binary =
        resolve_repo_command_binary(worktree_root, &clean, bundled_node_path, override_dirs)?;
    let clamped_timeout = Duration::from_secs(
        timeout_secs
            .unwrap_or(DEFAULT_REPO_CMD_TIMEOUT_SECS)
            .clamp(MIN_REPO_CMD_TIMEOUT_SECS, MAX_REPO_CMD_TIMEOUT_SECS),
    );

    let mut cmd = Command::new(&binary);
    cmd.current_dir(worktree_root)
        .args(args)
        .env("PATH", build_augmented_path(override_dirs))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    run_child_bounded(cmd, &clean, clamped_timeout).await
}

async fn run_child_bounded(
    mut cmd: Command,
    label: &str,
    timeout: Duration,
) -> Result<ControlledCliOutput, String> {
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to start `{label}`: {err}"))?;
    let stdout = child.stdout.take().ok_or("Missing child stdout")?;
    let stderr = child.stderr.take().ok_or("Missing child stderr")?;
    let out_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stdout
            .take(MAX_CLI_OUTPUT_BYTES)
            .read_to_end(&mut buf)
            .await;
        buf
    });
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr
            .take(MAX_CLI_OUTPUT_BYTES)
            .read_to_end(&mut buf)
            .await;
        buf
    });
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return Err(format!("`{label}` process failed: {err}")),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!(
                "`{label}` command timed out after {} seconds.",
                timeout.as_secs()
            ));
        }
    };
    let out_bytes = out_task.await.unwrap_or_default();
    let err_bytes = err_task.await.unwrap_or_default();
    let stdout_str = truncate_text(redact_sensitive_cli_output(&String::from_utf8_lossy(
        &out_bytes,
    )));
    let stderr_str = truncate_text(redact_sensitive_cli_output(&String::from_utf8_lossy(
        &err_bytes,
    )));
    let mut extracted_links = extract_platform_links(&stdout_str);
    for link in extract_platform_links(&stderr_str) {
        if !extracted_links
            .iter()
            .any(|existing| existing.url == link.url)
        {
            extracted_links.push(link);
        }
    }
    Ok(ControlledCliOutput {
        success: status.success(),
        exit_code: status.code(),
        stdout: stdout_str,
        stderr: stderr_str,
        extracted_links,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn extracts_issue_pr_mr_and_release_links() {
        let text = "Created https://github.com/acme/demo/issues/42 and https://github.com/acme/demo/pull/99 and https://gitlab.com/team/app/-/merge_requests/17 and https://github.com/dpbs-715/Picket/actions/runs/987654321 and https://github.com/dpbs-715/Picket/actions/workflows/release.yml and https://gitlab.com/team/app/-/pipelines/555";
        let links = extract_platform_links(text);
        assert_eq!(links.len(), 6);
        assert_eq!(links[0].label, "Issue #42");
        assert_eq!(links[1].label, "PR #99");
        assert_eq!(links[2].label, "MR !17");
        assert_eq!(links[3].label, "Actions #987654321");
        assert_eq!(links[4].label, "Workflow release.yml");
        assert_eq!(links[5].label, "Pipeline #555");
    }

    #[tokio::test]
    async fn executes_repo_command_with_bundled_node_fallback_and_blocks_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        let empty_bin = temp.path().join("empty-bin");
        let bundled_dir = temp.path().join("bundled");
        let outside_dir = temp.path().join("outside");
        std::fs::create_dir_all(work.join("scripts")).unwrap();
        std::fs::create_dir_all(&empty_bin).unwrap();
        std::fs::create_dir_all(&bundled_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();

        // Mock bundled picket-node binary:
        let mock_node = bundled_dir.join("picket-node");
        std::fs::write(
            &mock_node,
            r#"#!/bin/sh
echo "bundled-node args: $*"
echo "Token: gho_secretValue999"
echo "https://github.com/dpbs-715/Picket/actions/runs/424242"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&mock_node, std::fs::Permissions::from_mode(0o755)).unwrap();

        let out = exec_controlled_repo_command(
            &work,
            "node",
            &[
                "scripts/package-dmg.mjs".into(),
                "--check-version-only".into(),
                "--tag".into(),
                "v0.2.1".into(),
            ],
            Some(&mock_node),
            Some(std::slice::from_ref(&empty_bin)),
            Some(15),
        )
        .await
        .unwrap();
        assert!(out.success);
        assert!(out.stdout.contains(
            "bundled-node args: scripts/package-dmg.mjs --check-version-only --tag v0.2.1"
        ));
        assert!(!out.stdout.contains("gho_secretValue999"));
        assert!(out.stdout.contains("[REDACTED]"));
        assert_eq!(out.extracted_links.len(), 1);
        assert_eq!(out.extracted_links[0].label, "Actions #424242");

        // Worktree-relative script inside worktree succeeds:
        let local_script = work.join("scripts/check");
        std::fs::write(&local_script, "#!/bin/sh\necho local-ok $1\n").unwrap();
        std::fs::set_permissions(&local_script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let local_out = exec_controlled_repo_command(
            &work,
            "./scripts/check",
            &["v0.2.1".into()],
            None,
            Some(std::slice::from_ref(&empty_bin)),
            Some(15),
        )
        .await
        .unwrap();
        assert!(local_out.success);
        assert_eq!(local_out.stdout.trim(), "local-ok v0.2.1");

        // Symlink escaping worktree must be refused:
        let escaped_target = outside_dir.join("evil");
        std::fs::write(&escaped_target, "#!/bin/sh\necho escaped\n").unwrap();
        std::fs::set_permissions(&escaped_target, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&escaped_target, work.join("scripts/escape-link")).unwrap();
        let err = exec_controlled_repo_command(
            &work,
            "./scripts/escape-link",
            &[],
            None,
            Some(&[empty_bin]),
            Some(15),
        )
        .await
        .unwrap_err();
        assert!(err.contains("outside"));
    }

    #[tokio::test]
    async fn executes_git_tag_and_push_against_temp_bare_remote_and_cleans_body_file() {
        let temp = tempfile::tempdir().unwrap();
        let bare = temp.path().join("remote.git");
        let work = temp.path().join("work");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::create_dir_all(&work).unwrap();

        assert!(
            std::process::Command::new("/usr/bin/git")
                .current_dir(&bare)
                .args(["init", "--bare"])
                .status()
                .unwrap()
                .success()
        );
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Picket Test"],
            vec!["config", "user.email", "test@picket.local"],
            vec!["remote", "add", "origin", bare.to_str().unwrap()],
        ] {
            assert!(
                std::process::Command::new("/usr/bin/git")
                    .current_dir(&work)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(work.join("README.md"), "hello\n").unwrap();
        for args in [
            vec!["add", "README.md"],
            vec!["commit", "-m", "feat: init"],
            vec!["push", "-u", "origin", "main"],
        ] {
            assert!(
                std::process::Command::new("/usr/bin/git")
                    .current_dir(&work)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let tag_out = exec_controlled_git(
            &work,
            &[
                "tag".into(),
                "-a".into(),
                "v0.2.0".into(),
                "-m".into(),
                "Release v0.2.0".into(),
            ],
            false,
            false,
        )
        .await
        .unwrap();
        assert!(tag_out.success);

        let push_out = exec_controlled_git(
            &work,
            &["push".into(), "origin".into(), "v0.2.0".into()],
            true,
            false,
        )
        .await
        .unwrap();
        assert!(push_out.success);

        let ls_remote = std::process::Command::new("/usr/bin/git")
            .current_dir(&bare)
            .args(["tag", "-l"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&ls_remote.stdout).trim(), "v0.2.0");

        // Verify mock `gh` with RAII `--body-file` cleanup:
        let mock_bin = temp.path().join("bin");
        std::fs::create_dir_all(&mock_bin).unwrap();
        let gh = mock_bin.join("gh");
        let captured = temp.path().join("captured-body.txt");
        let captured_path_file = temp.path().join("captured-path.txt");
        std::fs::write(
            &gh,
            format!(
                r#"#!/bin/sh
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--body-file" ]; then
    shift
    echo "$1" > "{}"
    cat "$1" > "{}"
  fi
  shift
done
echo "https://github.com/acme/demo/issues/77"
"#,
                captured_path_file.display(),
                captured.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();

        let cli_out = exec_controlled_platform_cli(
            "gh",
            &work,
            &[
                "issue".into(),
                "create".into(),
                "--title".into(),
                "Test".into(),
            ],
            Some("## Details\nBody with `backticks` and $VARS"),
            Some("--body-file"),
            Some(&[mock_bin]),
        )
        .await
        .unwrap();
        assert!(cli_out.success);
        assert_eq!(cli_out.extracted_links.len(), 1);
        assert_eq!(cli_out.extracted_links[0].label, "Issue #77");
        assert_eq!(
            std::fs::read_to_string(&captured).unwrap(),
            "## Details\nBody with `backticks` and $VARS"
        );
        let temp_body_path = std::fs::read_to_string(&captured_path_file).unwrap();
        assert!(
            !Path::new(temp_body_path.trim()).exists(),
            "Temp Markdown body file should be cleaned up automatically after execution"
        );
    }
}
