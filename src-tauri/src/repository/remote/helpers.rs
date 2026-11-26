use super::{ConflictedFileSummary, RebasedCommitSummary, UpstreamSpec};
use crate::{
    i18n::{self, LanguageKind},
    repository::{RepositoryStore, Result, hash, parse, runner::GitOutput},
};
use std::{collections::BTreeSet, fs, path::Path};

pub(super) fn format_git_error(output: &GitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.bytes);
    let combined = match (stderr.trim(), stdout.trim()) {
        ("", "") => format!("Git exit code {:?}", output.code),
        (err, "") => err.to_owned(),
        ("", out) => out.to_owned(),
        (err, out) => format!("{err}; {out}"),
    };
    let clean = combined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = clean.chars();
    let head: String = chars.by_ref().take(280).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

pub(super) fn line_has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        let stripped = line.strip_suffix('\r').unwrap_or(line);
        stripped.starts_with("<<<<<<< ")
            || stripped == "<<<<<<<"
            || stripped == "======="
            || stripped.starts_with(">>>>>>> ")
            || stripped == ">>>>>>>"
            || stripped.starts_with("||||||| ")
            || stripped == "|||||||"
    })
}

pub(super) fn validate_remote_name(remote: &str) -> Result<()> {
    if remote.is_empty()
        || remote.starts_with('-')
        || remote.contains('/')
        || remote.contains("..")
        || remote
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("Invalid Git remote name".into());
    }
    Ok(())
}

impl RepositoryStore {
    pub(in crate::repository) fn is_remote_verified(
        &self,
        repository_id: &str,
        remote: &str,
    ) -> bool {
        if remote.is_empty() {
            return false;
        }
        self.inner
            .verified_remotes
            .lock()
            .map(|set| set.contains(&(repository_id.to_owned(), remote.to_owned())))
            .unwrap_or(false)
    }

    pub(in crate::repository) fn mark_remote_verified(&self, repository_id: &str, remote: &str) {
        if !remote.is_empty()
            && let Ok(mut set) = self.inner.verified_remotes.lock()
        {
            set.insert((repository_id.to_owned(), remote.to_owned()));
        }
    }

    pub(in crate::repository) fn clear_remote_verified(&self, repository_id: &str, remote: &str) {
        if !remote.is_empty()
            && let Ok(mut set) = self.inner.verified_remotes.lock()
        {
            set.remove(&(repository_id.to_owned(), remote.to_owned()));
        }
    }

    pub(in crate::repository) async fn active_rebase_branch(&self, root: &Path) -> Option<String> {
        let reader = &self.inner.reader;
        for marker in ["rebase-merge/head-name", "rebase-apply/head-name"] {
            let Ok(path) = reader
                .text(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .await
            else {
                continue;
            };
            if let Ok(raw) = fs::read_to_string(path) {
                let trimmed = raw.trim();
                if let Some(branch) = trimmed.strip_prefix("refs/heads/")
                    && !branch.is_empty()
                {
                    return Some(branch.to_owned());
                }
            }
        }
        None
    }

    pub(in crate::repository) async fn resolve_branch_upstream_and_divergence(
        &self,
        root: &Path,
        branch: &str,
    ) -> (Option<String>, Option<(u64, u64)>) {
        let Ok(Some(spec)) = self.branch_upstream_spec(root, branch).await else {
            return (None, None);
        };
        let div = self
            .compute_ahead_behind(root, "HEAD", &spec.tracking_ref)
            .await
            .ok();
        (Some(spec.display), div)
    }

    pub(in crate::repository) async fn list_remotes_unlocked(
        &self,
        root: &Path,
    ) -> Result<Vec<String>> {
        let output = self.inner.reader.run(root, &["remote"]).await?;
        if !output.success {
            return Ok(Vec::new());
        }
        let text = parse::utf8(&output.bytes)?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }

    pub(super) async fn branch_upstream_spec(
        &self,
        root: &Path,
        branch: &str,
    ) -> Result<Option<UpstreamSpec>> {
        if branch.is_empty() || branch == "(detached)" {
            return Ok(None);
        }
        let reader = &self.inner.reader;
        let remote_out = reader
            .run(
                root,
                &["config", "--get", &format!("branch.{branch}.remote")],
            )
            .await?;
        let merge_out = reader
            .run(
                root,
                &["config", "--get", &format!("branch.{branch}.merge")],
            )
            .await?;
        if !remote_out.success || !merge_out.success {
            return Ok(None);
        }
        let remote = parse::utf8(&remote_out.bytes)?.trim().to_owned();
        let merge_ref = parse::utf8(&merge_out.bytes)?.trim().to_owned();
        let Some(remote_branch) = merge_ref.strip_prefix("refs/heads/") else {
            return Ok(None);
        };
        if remote.is_empty() || remote == "." || remote_branch.is_empty() {
            return Ok(None);
        }
        Ok(Some(UpstreamSpec {
            display: format!("{remote}/{remote_branch}"),
            tracking_ref: format!("refs/remotes/{remote}/{remote_branch}"),
            remote,
            remote_branch: remote_branch.to_owned(),
        }))
    }

    pub(super) async fn ref_oid(&self, root: &Path, rev: &str) -> Option<String> {
        let out = self
            .inner
            .reader
            .run(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{rev}^{{commit}}"),
                ],
            )
            .await
            .ok()?;
        if !out.success {
            return None;
        }
        let oid = String::from_utf8_lossy(&out.bytes).trim().to_owned();
        (!oid.is_empty()).then_some(oid)
    }

    pub(super) async fn is_ancestor(&self, root: &Path, ancestor: &str, descendant: &str) -> bool {
        self.inner
            .reader
            .run(root, &["merge-base", "--is-ancestor", ancestor, descendant])
            .await
            .is_ok_and(|out| out.success)
    }

    pub(super) async fn compute_ahead_behind(
        &self,
        root: &Path,
        local_rev: &str,
        upstream_ref: &str,
    ) -> Result<(u64, u64)> {
        let text = self
            .inner
            .reader
            .text(
                root,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("{local_rev}...{upstream_ref}"),
                ],
            )
            .await?;
        let mut parts = text.split_whitespace();
        let ahead: u64 = parts
            .next()
            .ok_or("Missing ahead commit count")?
            .parse()
            .map_err(|_| "Invalid ahead commit count")?;
        let behind: u64 = parts
            .next()
            .ok_or("Missing behind commit count")?
            .parse()
            .map_err(|_| "Invalid behind commit count")?;
        Ok((ahead, behind))
    }

    pub(super) async fn git_marker_exists(&self, root: &Path, marker: &str) -> Result<bool> {
        let path = self
            .inner
            .reader
            .text(
                root,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
            )
            .await?;
        Ok(fs::symlink_metadata(path).is_ok())
    }

    pub(super) async fn rebase_in_progress(&self, root: &Path) -> Result<bool> {
        Ok(self.git_marker_exists(root, "rebase-merge").await?
            || self.git_marker_exists(root, "rebase-apply").await?)
    }

    pub(super) async fn other_git_op_in_progress(
        &self,
        root: &Path,
    ) -> Result<Option<&'static str>> {
        for marker in [
            "index.lock",
            "HEAD.lock",
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "BISECT_LOG",
        ] {
            if self.git_marker_exists(root, marker).await? {
                return Ok(Some(marker));
            }
        }
        Ok(None)
    }

    pub(super) async fn list_unmerged_files(
        &self,
        root: &Path,
    ) -> Result<Vec<ConflictedFileSummary>> {
        let bytes = self
            .inner
            .reader
            .bytes(root, &["ls-files", "--unmerged", "-z"])
            .await?;
        let mut paths = BTreeSet::new();
        for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let Some(tab) = record.iter().position(|b| *b == b'\t') else {
                continue;
            };
            let path = parse::utf8(&record[tab + 1..])?;
            parse::relative(&path)?;
            paths.insert(path);
        }
        Ok(paths
            .into_iter()
            .map(|path| ConflictedFileSummary {
                file_id: format!("file-{}", hash(&path)),
                path,
            })
            .collect())
    }

    pub(super) async fn list_commits_in_range(
        &self,
        root: &Path,
        branch: &str,
        range: &str,
        locale: LanguageKind,
    ) -> Result<Vec<RebasedCommitSummary>> {
        let out = self
            .inner
            .reader
            .with_locale(locale)
            .run(root, &["log", "--reverse", "--format=%H%x09%s", range])
            .await?;
        if !out.success {
            return Ok(Vec::new());
        }
        let text = parse::utf8_with_locale(&out.bytes, locale)?;
        let fallback_title = i18n::cpush::cpush_commit_completed_fallback(locale);
        let mut commits = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let (oid, title) = line.split_once('\t').unwrap_or((line, fallback_title));
            let oid = oid.trim().to_owned();
            if !oid.is_empty() {
                commits.push(RebasedCommitSummary {
                    short_oid: oid.chars().take(7).collect(),
                    oid,
                    title: title.trim().to_owned(),
                    branch: branch.to_owned(),
                });
            }
        }
        Ok(commits)
    }

    pub(super) async fn diff_touched_paths(
        &self,
        root: &Path,
        rev_a: &str,
        rev_b: &str,
    ) -> Result<BTreeSet<String>> {
        let bytes = self
            .inner
            .reader
            .bytes(
                root,
                &["diff", "--name-only", "-z", "--find-renames", rev_a, rev_b],
            )
            .await?;
        let mut set = BTreeSet::new();
        for entry in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            set.insert(parse::utf8(entry)?);
        }
        Ok(set)
    }

    pub(super) async fn diff_touches_submodule(
        &self,
        root: &Path,
        rev_a: &str,
        rev_b: &str,
    ) -> Result<bool> {
        let bytes = self
            .inner
            .reader
            .bytes(
                root,
                &["diff-tree", "-r", "-z", "--no-renames", rev_a, rev_b],
            )
            .await?;
        let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).filter(|r| !r.is_empty()).collect();
        for pair in fields.as_chunks::<2>().0 {
            let header = parse::utf8(pair[0])?;
            let Some(rest) = header.strip_prefix(':') else {
                continue;
            };
            let mut parts = rest.split_whitespace();
            let old_mode = parts.next();
            let new_mode = parts.next();
            if old_mode == Some("160000") || new_mode == Some("160000") {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
