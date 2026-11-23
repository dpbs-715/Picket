//! Explicit native UI harness only: real Git, always a disposable directory.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
};
use tempfile::TempDir;

pub struct Fixture {
    _directory: TempDir,
    pub root: PathBuf,
    index: Vec<u8>,
    staged_entries: Vec<u8>,
    head: Vec<u8>,
    contents: Vec<u8>,
}
pub fn git(root: &std::path::Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("/usr/bin/git")
        .current_dir(root)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Temporary read proof failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}
impl Fixture {
    pub fn new() -> Result<Self, String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let root = directory.path().join("read-proof");
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        git(&root, &["init", "-b", "feat/read-proof"])?;
        git(&root, &["config", "user.name", "Picket Read Test"])?;
        git(&root, &["config", "user.email", "read@picket.invalid"])?;
        git(&root, &["config", "commit.gpgSign", "false"])?;
        fs::write(root.join("part.txt"), "first=1\nsecond=1\n")
            .map_err(|error| error.to_string())?;
        fs::write(root.join("pnpm-lock.yaml"), "lock: old\n").map_err(|error| error.to_string())?;
        git(&root, &["add", "--", "part.txt", "pnpm-lock.yaml"])?;
        git(&root, &["commit", "-m", "test: read fixture"])?;
        let linked = directory.path().join("linked worktree");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "feat/linked",
                linked.to_str().ok_or("Non UTF-8 temporary path")?,
            ],
        )?;
        fs::write(root.join("part.txt"), "first=2\nsecond=1\n")
            .map_err(|error| error.to_string())?;
        git(&root, &["add", "--", "part.txt"])?;
        let contents = b"first=2\nsecond=3\n".to_vec();
        fs::write(root.join("part.txt"), &contents).map_err(|error| error.to_string())?;
        fs::write(root.join("新 文件.ts"), "export const proof = true;\n")
            .map_err(|error| error.to_string())?;
        fs::write(root.join("pnpm-lock.yaml"), "lock: updated\n")
            .map_err(|error| error.to_string())?;
        let index = fs::read(root.join(".git/index")).map_err(|error| error.to_string())?;
        let staged_entries = git(&root, &["ls-files", "--stage", "-z"])?;
        let head = git(&root, &["rev-parse", "HEAD"])?;
        Ok(Self {
            _directory: directory,
            root,
            index,
            staged_entries,
            head,
            contents,
        })
    }
    pub fn verify_branch_operations(&self) -> Result<(), String> {
        let created = self
            .root
            .parent()
            .ok_or("Missing fixture parent")?
            .join("feat-worktree-proof");
        if git(&self.root, &["symbolic-ref", "--short", "HEAD"])? != b"feat/branch-proof\n"
            || git(&self.root, &["rev-parse", "HEAD"])? != self.head
            || git(&self.root, &["ls-files", "--stage", "-z"])? != self.staged_entries
            || fs::read(self.root.join("part.txt")).map_err(|error| error.to_string())?
                != self.contents
            || fs::read(self.root.join("pnpm-lock.yaml")).map_err(|error| error.to_string())?
                != b"lock: updated\n"
            || fs::read(self.root.join("新 文件.ts")).map_err(|error| error.to_string())?
                != b"export const proof = true;\n"
            || git(&created, &["symbolic-ref", "--short", "HEAD"])? != b"feat/worktree-proof\n"
            || git(&created, &["rev-parse", "HEAD"])? != self.head
            || !git(&created, &["status", "--porcelain"])?.is_empty()
            || fs::read(created.join("part.txt")).map_err(|error| error.to_string())?
                != b"first=1\nsecond=1\n"
        {
            return Err("Branch/worktree proof changed staging, drafts, HEAD, or created an incorrect worktree".into());
        }
        Ok(())
    }
    pub fn verify_commit_operations(&self) -> Result<(), String> {
        let created = self
            .root
            .parent()
            .ok_or("Missing fixture parent")?
            .join("feat-worktree-proof");
        if git(&self.root, &["symbolic-ref", "--short", "HEAD"])? != b"feat/branch-proof\n"
            || git(&self.root, &["rev-parse", "HEAD^1"])? != self.head
            || git(&self.root, &["rev-list", "--count", "HEAD"])? != b"2\n"
            || git(&self.root, &["show", "-s", "--format=%s", "HEAD"])?
                != b"read-proof: keep manual draft\n"
            || git(&self.root, &["show", "HEAD:part.txt"])? != b"first=2\nsecond=1\n"
            || git(&self.root, &["show", ":part.txt"])? != b"first=2\nsecond=1\n"
            || fs::read(self.root.join("part.txt")).map_err(|error| error.to_string())?
                != self.contents
            || git(&self.root, &["show", "HEAD:pnpm-lock.yaml"])? != b"lock: old\n"
            || fs::read(self.root.join("pnpm-lock.yaml")).map_err(|error| error.to_string())?
                != b"lock: updated\n"
            || git(&self.root, &["show", "HEAD:新 文件.ts"]).is_ok()
            || fs::read(self.root.join("新 文件.ts")).map_err(|error| error.to_string())?
                != b"export const proof = true;\n"
            || git(&created, &["symbolic-ref", "--short", "HEAD"])? != b"feat/worktree-proof\n"
            || git(&created, &["rev-parse", "HEAD"])? != self.head
            || !git(&created, &["status", "--porcelain"])?.is_empty()
        {
            return Err(
                "Local commit proof did not commit the exact staged scope or failed to preserve drafts/unselected files"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn verify_unchanged(&self) -> Result<(), String> {
        if fs::read(self.root.join(".git/index")).map_err(|error| error.to_string())? != self.index
            || fs::read(self.root.join("part.txt")).map_err(|error| error.to_string())?
                != self.contents
        {
            return Err("Read-only workbench changed the real index or working draft".into());
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct SmokeState(pub Mutex<Option<Fixture>>);
impl SmokeState {
    pub fn path(&self) -> Result<PathBuf, String> {
        let mut fixture = self.0.lock().map_err(|_| "Read proof is unavailable")?;
        if fixture.is_none() {
            *fixture = Some(Fixture::new()?);
        }
        Ok(fixture.as_ref().unwrap().root.clone())
    }
    pub fn cleanup(&self) -> Result<(), String> {
        let fixture = self
            .0
            .lock()
            .map_err(|_| "Read proof is unavailable")?
            .take();
        if let Some(fixture) = fixture {
            fixture
                ._directory
                .close()
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    pub fn verify_branches(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Branch proof is unavailable")?
            .as_ref()
            .ok_or("Branch proof did not open")?
            .verify_branch_operations()
    }
    pub fn verify_commit(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Commit proof is unavailable")?
            .as_ref()
            .ok_or("Commit proof did not open")?
            .verify_commit_operations()
    }
    pub fn verify(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Read proof is unavailable")?
            .as_ref()
            .ok_or("Read proof did not open")?
            .verify_unchanged()
    }
}
