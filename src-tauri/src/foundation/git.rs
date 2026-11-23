//! M0 candidate algorithm, used only in disposable repositories. No command exposes
//! this executor to user repositories. Hook execution and crash recovery remain M2/M4 gates.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::{NamedTempFile, TempDir};

type Result<T> = std::result::Result<T, String>;

#[derive(Debug)]
struct PreparedScope {
    parent: String,
    branch: String,
    tree: String,
    paths: Vec<String>,
    fingerprint: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct ExecutionRecord {
    parent: String,
    branch: String,
    tree: String,
    paths: Vec<String>,
    message: String,
    commit: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitProbeReport {
    pub git_version: String,
    pub cases: Vec<String>,
    pub commits: Vec<String>,
    pub temporary_repository_removed: bool,
}

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn git(&self, args: &[&str]) -> Result<Vec<u8>> {
        self.execute(args, None, None)
    }

    fn text(&self, args: &[&str]) -> Result<String> {
        String::from_utf8(self.git(args)?)
            .map(|s| s.trim_end().to_owned())
            .map_err(|e| e.to_string())
    }

    fn execute(
        &self,
        args: &[&str],
        index: Option<&Path>,
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut command = Command::new("/usr/bin/git");
        // Disposable proof repositories must not inherit a user's hooks, signing, or Git environment.
        command
            .current_dir(&self.root)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_LITERAL_PATHSPECS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(index) = index {
            command.env("GIT_INDEX_FILE", index);
        }
        command.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        if let Some(input) = input {
            let mut stdin = child.stdin.take().ok_or("Git input is unavailable")?;
            stdin.write_all(input).map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "git {}: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(output.stdout)
    }

    fn index_path(&self) -> Result<PathBuf> {
        self.text(&["rev-parse", "--path-format=absolute", "--git-path", "index"])
            .map(PathBuf::from)
    }

    fn branch(&self) -> Result<String> {
        self.text(&["symbolic-ref", "--short", "HEAD"])
    }

    fn ensure_allowed(&self) -> Result<()> {
        if ["main", "master"].contains(&self.branch()?.as_str()) {
            return Err("Create or switch to a development branch first".into());
        }
        if !self.git(&["ls-files", "--unmerged", "-z"])?.is_empty() {
            return Err("Resolve existing conflicts first".into());
        }
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
        ] {
            let path = self.text(&["rev-parse", "--path-format=absolute", "--git-path", marker])?;
            if Path::new(&path).exists() {
                return Err("An existing Git operation is in progress".into());
            }
        }
        // Refuse configured executable hooks in this candidate rather than bypassing them.
        let hooks = self.text(&["rev-parse", "--path-format=absolute", "--git-path", "hooks"])?;
        if let Ok(entries) = fs::read_dir(hooks) {
            use std::os::unix::fs::PermissionsExt;
            for entry in entries {
                let path = entry.map_err(|e| e.to_string())?.path();
                let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
                if metadata.is_file()
                    && metadata.permissions().mode() & 0o111 != 0
                    && !path.to_string_lossy().ends_with(".sample")
                {
                    return Err(
                        "Executable hooks are not supported by the M0 proof; no commit was made"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }

    fn fingerprint(&self, paths: &[String]) -> Result<Vec<u8>> {
        let mut result = Vec::new();
        for prefix in [
            vec!["diff", "--binary", "--no-ext-diff", "HEAD", "--"],
            vec![
                "diff",
                "--cached",
                "--binary",
                "--no-ext-diff",
                "HEAD",
                "--",
            ],
            vec!["ls-files", "--stage", "-z", "--"],
        ] {
            let args: Vec<_> = prefix
                .into_iter()
                .chain(paths.iter().map(String::as_str))
                .collect();
            let bytes = self.git(&args)?;
            result.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            result.extend(bytes);
        }
        for path in paths {
            let path = self.root.join(path);
            let (kind, bytes) = match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    use std::os::unix::ffi::OsStrExt;
                    (
                        1,
                        fs::read_link(path)
                            .map_err(|e| e.to_string())?
                            .as_os_str()
                            .as_bytes()
                            .to_vec(),
                    )
                }
                Ok(meta) if meta.is_file() => (2, fs::read(path).map_err(|e| e.to_string())?),
                Ok(_) => {
                    return Err(
                        "Directory or submodule selection is not supported by the M0 proof".into(),
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (0, Vec::new()),
                Err(error) => return Err(error.to_string()),
            };
            result.push(kind);
            result.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            result.extend(bytes);
        }
        Ok(result)
    }

    fn prepare(&self, units: &[Vec<&str>]) -> Result<PreparedScope> {
        self.ensure_allowed()?;
        let paths: Vec<String> = units
            .iter()
            .flatten()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if paths.is_empty() {
            return Err("No selected paths".into());
        }
        for path in &paths {
            if Path::new(path)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
                || path
                    .split('/')
                    .any(|part| part.eq_ignore_ascii_case(".git"))
            {
                return Err("Selection must contain literal repository-relative file paths".into());
            }
        }
        let parent = self.text(&["rev-parse", "HEAD"])?;
        let branch = self.branch()?;
        let fingerprint = self.fingerprint(&paths)?;
        let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
        let index = staging.path().join("selected.index");
        self.execute(&["read-tree", &parent], Some(&index), None)?;
        for unit in units {
            let cached: Vec<_> = ["diff", "--cached", "--name-only", "-z", "HEAD", "--"]
                .into_iter()
                .chain(unit.iter().copied())
                .collect();
            if self.git(&cached)?.is_empty() {
                let add: Vec<_> = ["add", "--all", "--"]
                    .into_iter()
                    .chain(unit.iter().copied())
                    .collect();
                self.execute(&add, Some(&index), None)?;
            } else {
                let remove: Vec<_> = ["update-index", "--force-remove", "--"]
                    .into_iter()
                    .chain(unit.iter().copied())
                    .collect();
                self.execute(&remove, Some(&index), None)?;
                let entries: Vec<_> = ["ls-files", "--stage", "-z", "--"]
                    .into_iter()
                    .chain(unit.iter().copied())
                    .collect();
                self.execute(
                    &["update-index", "-z", "--index-info"],
                    Some(&index),
                    Some(&self.git(&entries)?),
                )?;
            }
        }
        let tree = String::from_utf8(self.execute(&["write-tree"], Some(&index), None)?)
            .map_err(|e| e.to_string())?
            .trim()
            .to_owned();
        if self.text(&["rev-parse", "HEAD"])? != parent
            || self.branch()? != branch
            || self.fingerprint(&paths)? != fingerprint
        {
            return Err("Selected content changed while preparing the scope".into());
        }
        if tree == self.text(&["rev-parse", "HEAD^{tree}"])? {
            return Err("Selected scope has no changes".into());
        }
        Ok(PreparedScope {
            parent,
            branch,
            tree,
            paths,
            fingerprint,
        })
    }

    // A single mutable owner orders writes; the real index lock also excludes external Git index writes.
    fn commit(
        &mut self,
        scope: &PreparedScope,
        operation_id: &str,
        message: &str,
    ) -> Result<String> {
        if message.trim().is_empty() {
            return Err("Commit message is required".into());
        }
        if operation_id.is_empty()
            || !operation_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("Invalid operation identity".into());
        }
        let index_path = self.index_path()?;
        let journal = index_path
            .parent()
            .ok_or("Missing Git directory")?
            .join("picket-probe");
        fs::create_dir_all(&journal).map_err(|e| e.to_string())?;
        let record_path = journal.join(format!("{operation_id}.json"));
        if record_path.exists() {
            let previous: ExecutionRecord =
                serde_json::from_slice(&fs::read(&record_path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if previous.parent != scope.parent
                || previous.branch != scope.branch
                || previous.tree != scope.tree
                || previous.paths != scope.paths
                || previous.message != message
            {
                return Err("Operation identity belongs to another scope or message".into());
            }
            let commit = previous.commit.ok_or(
                "Interrupted operation requires Git reconciliation; it will not be repeated",
            )?;
            self.git(&["merge-base", "--is-ancestor", &commit, "HEAD"])?;
            if self.text(&["rev-parse", &format!("{commit}^{{tree}}")])? != scope.tree
                || self.text(&["rev-parse", &format!("{commit}^")])? != scope.parent
            {
                return Err("Recorded commit does not match the authorized scope".into());
            }
            return Ok(commit);
        }
        self.ensure_allowed()?;
        let mut index_lock = IndexLock::acquire(&index_path)?;
        if self.text(&["rev-parse", "HEAD"])? != scope.parent
            || self.branch()? != scope.branch
            || self.fingerprint(&scope.paths)? != scope.fingerprint
        {
            return Err("Selected scope is outdated; no commit was made".into());
        }
        let mut record = ExecutionRecord {
            parent: scope.parent.clone(),
            branch: scope.branch.clone(),
            tree: scope.tree.clone(),
            paths: scope.paths.clone(),
            message: message.to_owned(),
            commit: None,
        };
        let mut record_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&record_path)
            .map_err(|e| e.to_string())?;
        record_file
            .write_all(&serde_json::to_vec(&record).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        record_file.sync_all().map_err(|e| e.to_string())?;
        let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
        let selected_index = temporary.path().join("commit.index");
        self.execute(&["read-tree", &scope.tree], Some(&selected_index), None)?;
        self.execute(&["commit", "-m", message], Some(&selected_index), None)?;
        let commit = self.text(&["rev-parse", "HEAD"])?;
        if self.text(&["rev-parse", "HEAD^{tree}"])? != scope.tree
            || self.text(&["rev-parse", "HEAD^"])? != scope.parent
        {
            return Err(format!(
                "Commit {commit} exists but differs from the authorized scope; stop and inspect Git"
            ));
        }
        let reconcile = (|| -> Result<()> {
            let remaining = temporary.path().join("remaining.index");
            fs::copy(&index_path, &remaining).map_err(|e| e.to_string())?;
            let args: Vec<_> = ["reset", "--quiet", commit.as_str(), "--"]
                .into_iter()
                .chain(scope.paths.iter().map(String::as_str))
                .collect();
            self.execute(&args, Some(&remaining), None)?;
            index_lock.replace(
                &index_path,
                &fs::read(remaining).map_err(|e| e.to_string())?,
            )?;
            record.commit = Some(commit.clone());
            let mut completed = NamedTempFile::new_in(&journal).map_err(|e| e.to_string())?;
            completed
                .write_all(&serde_json::to_vec(&record).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            completed.as_file().sync_all().map_err(|e| e.to_string())?;
            completed.persist(&record_path).map_err(|e| e.to_string())?;
            Ok(())
        })();
        reconcile.map_err(|error| format!("Commit {commit} already exists; reconciliation failed: {error}. Do not repeat the commit."))?;
        Ok(commit)
    }
}

struct IndexLock {
    path: PathBuf,
    file: File,
    replaced: bool,
}
impl IndexLock {
    fn acquire(index: &Path) -> Result<Self> {
        let path = index.with_file_name("index.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Cannot acquire Git index lock: {e}"))?;
        Ok(Self {
            path,
            file,
            replaced: false,
        })
    }
    fn replace(&mut self, index: &Path, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes).map_err(|e| e.to_string())?;
        self.file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&self.path, index).map_err(|e| e.to_string())?;
        self.replaced = true;
        Ok(())
    }
}
impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.replaced {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct Fixture {
    directory: TempDir,
    repository: Repository,
}
impl Fixture {
    fn new() -> Result<Self> {
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let repository = Repository {
            root: directory.path().to_path_buf(),
        };
        repository.git(&["init", "-b", "feat/foundation"])?;
        repository.git(&["config", "user.name", "Picket Foundation Test"])?;
        repository.git(&["config", "user.email", "foundation@picket.invalid"])?;
        repository.git(&["config", "commit.gpgSign", "false"])?;
        for (path, contents) in [
            ("Order.ts", "first = 1\nsecond = 1\n"),
            ("notes.md", "old notes\n"),
            ("remove.txt", "remove me\n"),
            ("rename.txt", "rename me\n"),
        ] {
            fs::write(directory.path().join(path), contents).map_err(|e| e.to_string())?;
        }
        repository.git(&["add", "."])?;
        repository.git(&["commit", "-m", "test: initial fixture"])?;
        Ok(Self {
            directory,
            repository,
        })
    }
    fn write(&self, path: &str, content: &str) -> Result<()> {
        fs::write(self.directory.path().join(path), content).map_err(|e| e.to_string())
    }
    fn mixed_changes(&self) -> Result<()> {
        self.write("Order.ts", "first = 2\nsecond = 1\n")?;
        self.repository.git(&["add", "--", "Order.ts"])?;
        self.write("Order.ts", "first = 2\nsecond = 3\n")?;
        self.write("notes.md", "staged notes\n")?;
        self.repository.git(&["add", "--", "notes.md"])?;
        self.write("新 文件.ts", "export const retry = 3;\n")
    }
}

fn require(condition: bool, description: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(format!("Git proof failed: {description}"))
    }
}

pub fn run_probe() -> Result<GitProbeReport> {
    let mut fixture = Fixture::new()?;
    fixture.mixed_changes()?;
    let root = fixture.directory.path().to_path_buf();
    let scope = fixture
        .repository
        .prepare(&[vec!["Order.ts"], vec!["新 文件.ts"]])?;
    let first = fixture
        .repository
        .commit(&scope, "scope-one", "feat: selected fixture changes")?;
    require(
        fixture.repository.text(&["show", "HEAD:Order.ts"])? == "first = 2\nsecond = 1",
        "partial staging committed the wrong content",
    )?;
    require(
        fs::read_to_string(root.join("Order.ts")).map_err(|e| e.to_string())?
            == "first = 2\nsecond = 3\n",
        "unstaged draft was modified",
    )?;
    require(
        fixture.repository.text(&["show", ":notes.md"])? == "staged notes",
        "unselected staging was modified",
    )?;
    require(
        fixture.repository.text(&["show", "HEAD:notes.md"])? == "old notes",
        "unselected staged file entered the commit",
    )?;
    require(
        fixture
            .repository
            .commit(&scope, "scope-one", "feat: selected fixture changes")?
            == first,
        "repeated operation created another commit",
    )?;
    require(
        fixture.repository.text(&["rev-list", "--count", "HEAD"])? == "2",
        "duplicate commit was written",
    )?;
    let second_scope = fixture.repository.prepare(&[vec!["notes.md"]])?;
    let second =
        fixture
            .repository
            .commit(&second_scope, "scope-two", "docs: selected fixture notes")?;
    require(
        fixture.repository.text(&["show", "HEAD:notes.md"])? == "staged notes",
        "split commit has the wrong notes",
    )?;
    require(
        fixture.repository.text(&["rev-list", "--count", "HEAD"])? == "3",
        "split did not produce exactly two commits",
    )?;
    let git_version = fixture.repository.text(&["--version"])?;
    drop(fixture);
    require(!root.exists(), "temporary repository was not removed")?;
    Ok(GitProbeReport {
        git_version,
        commits: vec![first, second],
        temporary_repository_removed: true,
        cases: [
            "partial staging",
            "unstaged draft preserved",
            "unselected staged file preserved",
            "Unicode and spaces",
            "duplicate operation",
            "split commits",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn mixed_scope_and_split_commits_preserve_remaining_changes() {
        assert_eq!(run_probe().unwrap().commits.len(), 2);
    }

    #[test]
    fn deletion_and_rename_commit_both_paths() {
        let mut fixture = Fixture::new().unwrap();
        fs::remove_file(fixture.directory.path().join("remove.txt")).unwrap();
        fixture
            .repository
            .git(&["mv", "--", "rename.txt", "重命名 文件.txt"])
            .unwrap();
        let scope = fixture
            .repository
            .prepare(&[vec!["remove.txt"], vec!["rename.txt", "重命名 文件.txt"]])
            .unwrap();
        fixture
            .repository
            .commit(&scope, "rename-delete", "test: rename and delete")
            .unwrap();
        assert!(
            fixture
                .repository
                .git(&["show", "HEAD:remove.txt"])
                .is_err()
        );
        assert!(
            fixture
                .repository
                .git(&["show", "HEAD:rename.txt"])
                .is_err()
        );
        assert_eq!(
            fixture
                .repository
                .text(&["show", "HEAD:重命名 文件.txt"])
                .unwrap(),
            "rename me"
        );
        assert!(
            fixture
                .repository
                .git(&["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn stale_selected_content_does_not_commit() {
        let mut fixture = Fixture::new().unwrap();
        fixture.write("Order.ts", "first = 2\n").unwrap();
        let scope = fixture.repository.prepare(&[vec!["Order.ts"]]).unwrap();
        fixture.write("Order.ts", "first = 3\n").unwrap();
        assert!(
            fixture
                .repository
                .commit(&scope, "stale", "test: stale")
                .unwrap_err()
                .contains("outdated")
        );
        assert_eq!(
            fixture
                .repository
                .text(&["rev-list", "--count", "HEAD"])
                .unwrap(),
            "1"
        );
        assert!(
            !fixture
                .repository
                .index_path()
                .unwrap()
                .with_file_name("index.lock")
                .exists()
        );
    }

    #[test]
    fn unrelated_changes_do_not_invalidate_selected_scope() {
        let mut fixture = Fixture::new().unwrap();
        fixture.write("Order.ts", "first = 2\n").unwrap();
        let scope = fixture.repository.prepare(&[vec!["Order.ts"]]).unwrap();
        fixture.write("notes.md", "unrelated staging\n").unwrap();
        fixture.repository.git(&["add", "--", "notes.md"]).unwrap();
        fixture
            .repository
            .commit(&scope, "unrelated", "test: selected only")
            .unwrap();
        assert_eq!(
            fixture.repository.text(&["show", ":notes.md"]).unwrap(),
            "unrelated staging"
        );
        assert_eq!(
            fixture.repository.text(&["show", "HEAD:notes.md"]).unwrap(),
            "old notes"
        );
    }

    #[test]
    fn deleting_an_untracked_file_invalidates_its_frozen_scope() {
        let mut fixture = Fixture::new().unwrap();
        fixture.write("new.txt", "missing").unwrap();
        let scope = fixture.repository.prepare(&[vec!["new.txt"]]).unwrap();
        fs::remove_file(fixture.directory.path().join("new.txt")).unwrap();
        assert!(
            fixture
                .repository
                .commit(&scope, "deleted-new", "test: stale new file")
                .unwrap_err()
                .contains("outdated")
        );
        assert_eq!(
            fixture
                .repository
                .text(&["rev-list", "--count", "HEAD"])
                .unwrap(),
            "1"
        );
    }

    #[test]
    fn operation_identity_cannot_be_reused_for_another_message() {
        let mut fixture = Fixture::new().unwrap();
        fixture.mixed_changes().unwrap();
        let scope = fixture.repository.prepare(&[vec!["Order.ts"]]).unwrap();
        fixture
            .repository
            .commit(&scope, "same-operation", "test: first message")
            .unwrap();
        assert!(
            fixture
                .repository
                .commit(&scope, "same-operation", "test: different message")
                .unwrap_err()
                .contains("another scope or message")
        );
        assert_eq!(
            fixture
                .repository
                .text(&["rev-list", "--count", "HEAD"])
                .unwrap(),
            "2"
        );
    }

    #[test]
    fn protected_branch_and_path_escape_are_refused() {
        let fixture = Fixture::new().unwrap();
        assert!(fixture.repository.prepare(&[vec!["../outside"]]).is_err());
        fixture.repository.git(&["branch", "-m", "main"]).unwrap();
        assert!(
            fixture
                .repository
                .prepare(&[vec!["Order.ts"]])
                .unwrap_err()
                .contains("development branch")
        );
    }

    #[test]
    fn real_index_lock_prevents_writes() {
        let mut fixture = Fixture::new().unwrap();
        fixture.mixed_changes().unwrap();
        let scope = fixture.repository.prepare(&[vec!["Order.ts"]]).unwrap();
        let lock = IndexLock::acquire(&fixture.repository.index_path().unwrap()).unwrap();
        assert!(
            fixture
                .repository
                .commit(&scope, "locked", "test: lock")
                .unwrap_err()
                .contains("index lock")
        );
        drop(lock);
        assert_eq!(
            fixture
                .repository
                .text(&["rev-list", "--count", "HEAD"])
                .unwrap(),
            "1"
        );
    }

    #[test]
    fn executable_hook_is_refused_not_bypassed() {
        let fixture = Fixture::new().unwrap();
        let hook = fixture.directory.path().join(".git/hooks/pre-commit");
        fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            fixture
                .repository
                .prepare(&[vec!["Order.ts"]])
                .unwrap_err()
                .contains("hooks are not supported")
        );
        assert_eq!(
            fixture
                .repository
                .text(&["rev-list", "--count", "HEAD"])
                .unwrap(),
            "1"
        );
    }

    #[test]
    fn interrupted_journal_never_repeats_a_commit() {
        let mut fixture = Fixture::new().unwrap();
        fixture.mixed_changes().unwrap();
        let scope = fixture.repository.prepare(&[vec!["Order.ts"]]).unwrap();
        let journal = fixture.directory.path().join(".git/picket-probe");
        fs::create_dir(&journal).unwrap();
        let record = ExecutionRecord {
            parent: scope.parent.clone(),
            branch: scope.branch.clone(),
            tree: scope.tree.clone(),
            paths: scope.paths.clone(),
            message: "test: never duplicate".into(),
            commit: None,
        };
        fs::write(
            journal.join("interrupted.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(
            fixture
                .repository
                .commit(&scope, "interrupted", "test: never duplicate")
                .unwrap_err()
                .contains("Interrupted operation")
        );
        assert_eq!(
            fixture
                .repository
                .text(&["rev-list", "--count", "HEAD"])
                .unwrap(),
            "1"
        );
    }
}
