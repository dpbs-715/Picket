//! Controlled discrete-argv execution for `run_git` and `run_platform_cli`, with RAII temp
//! Markdown body files (zero shell interpolation), strict output caps, token redaction, and
//! automatic GitHub/GitLab URL extraction.

use super::{
    cli_env::{build_augmented_path, redact_sensitive_cli_output, resolve_cli_binary},
    types::CustomActionLink,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::NamedTempFile;
use tokio::{io::AsyncReadExt, process::Command};

const CLI_EXEC_TIMEOUT: Duration = Duration::from_secs(60);
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
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
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

    run_child_bounded(cmd, "git").await
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
            "`{cli}` CLI was not found on this machine. Run `check_cli_env` first and offer installation via `ask_user_question`."
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

    run_child_bounded(cmd, cli).await
}

async fn run_child_bounded(mut cmd: Command, label: &str) -> Result<ControlledCliOutput, String> {
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
    let status = match tokio::time::timeout(CLI_EXEC_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return Err(format!("`{label}` process failed: {err}")),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!("`{label}` command timed out after 60 seconds."));
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
        let text = "Created https://github.com/acme/demo/issues/42 and https://github.com/acme/demo/pull/99 and https://gitlab.com/team/app/-/merge_requests/17";
        let links = extract_platform_links(text);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].label, "Issue #42");
        assert_eq!(links[1].label, "PR #99");
        assert_eq!(links[2].label, "MR !17");
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
