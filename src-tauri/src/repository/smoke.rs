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
    pub bare_remote: PathBuf,
    pub editor_bin: PathBuf,
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
        let bare_remote = directory.path().join("origin.git");
        fs::create_dir(&bare_remote).map_err(|error| error.to_string())?;
        git(&bare_remote, &["init", "--bare"])?;

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

        git(&root, &["branch", "feat/branch-proof"])?;
        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                bare_remote.to_str().ok_or("Non UTF-8 temporary path")?,
            ],
        )?;
        git(&root, &["push", "-u", "origin", "feat/read-proof"])?;
        git(&root, &["checkout", "feat/branch-proof"])?;
        git(&root, &["push", "-u", "origin", "feat/branch-proof"])?;
        git(
            &root,
            &["push", "origin", "HEAD:refs/heads/feat/remote-checkout"],
        )?;
        git(
            &bare_remote,
            &["symbolic-ref", "HEAD", "refs/heads/feat/read-proof"],
        )?;
        git(&root, &["remote", "set-head", "origin", "-a"])?;
        git(&root, &["checkout", "feat/read-proof"])?;

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
        fs::write(
            root.join("\u{65b0} \u{6587}\u{4ef6}.ts"),
            "export const proof = true;\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(root.join("pnpm-lock.yaml"), "lock: updated\n")
            .map_err(|error| error.to_string())?;
        let index = fs::read(root.join(".git/index")).map_err(|error| error.to_string())?;
        let staged_entries = git(&root, &["ls-files", "--stage", "-z"])?;
        let head = git(&root, &["rev-parse", "HEAD"])?;
        let editor_bin = directory.path().join("editor-bin");
        fs::create_dir(&editor_bin).map_err(|error| error.to_string())?;
        let log_dir = directory.path();
        for cli in ["idea", "rustrover", "webstorm", "code", "cursor"] {
            let script_path = editor_bin.join(cli);
            let script = format!(
                concat!(
                    "#!/bin/sh\n",
                    "set -e\n",
                    "rm -f '{log}/ide-cli.txt' '{log}/ide-cwd.txt' '{log}/ide-mode.txt' '{log}/ide-left.bin' '{log}/ide-right.bin'\n",
                    "printf '%s\\n' \"{cli}\" > '{log}/ide-cli.txt'\n",
                    "pwd > '{log}/ide-cwd.txt'\n",
                    "printf '%s\\n' \"$1\" > '{log}/ide-mode.txt'\n",
                    "if [ \"$1\" = \"diff\" ] || [ \"$1\" = \"--diff\" ]; then\n",
                    "  cat \"$2\" > '{log}/ide-left.bin'\n",
                    "  cat \"$3\" > '{log}/ide-right.bin'\n",
                    "fi\n"
                ),
                cli = cli,
                log = log_dir.display()
            );
            fs::write(&script_path, script).map_err(|error| error.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
                .map_err(|error| error.to_string())?;
        }
        Ok(Self {
            _directory: directory,
            root,
            bare_remote,
            editor_bin,
            index,
            staged_entries,
            head,
            contents,
        })
    }
    pub fn verify_branch_operations(&self) -> Result<(), String> {
        let linked = self
            .root
            .parent()
            .ok_or("Missing fixture parent")?
            .join("linked worktree");
        if git(&self.root, &["symbolic-ref", "--short", "HEAD"])? != b"feat/branch-proof\n"
            || git(
                &self.root,
                &["config", "--get", "branch.feat/remote-checkout.remote"],
            )? != b"origin\n"
            || git(
                &self.root,
                &["config", "--get", "branch.feat/remote-checkout.merge"],
            )? != b"refs/heads/feat/remote-checkout\n"
            || git(&self.root, &["rev-parse", "HEAD"])? != self.head
            || git(&self.root, &["ls-files", "--stage", "-z"])? != self.staged_entries
            || fs::read(self.root.join("part.txt")).map_err(|error| error.to_string())?
                != self.contents
            || fs::read(self.root.join("pnpm-lock.yaml")).map_err(|error| error.to_string())?
                != b"lock: updated\n"
            || fs::read(self.root.join("\u{65b0} \u{6587}\u{4ef6}.ts"))
                .map_err(|error| error.to_string())?
                != b"export const proof = true;\n"
            || git(&linked, &["symbolic-ref", "--short", "HEAD"])? != b"feat/linked\n"
            || git(&linked, &["rev-parse", "HEAD"])? != self.head
            || !git(&linked, &["status", "--porcelain"])?.is_empty()
            || fs::read(linked.join("part.txt")).map_err(|error| error.to_string())?
                != b"first=1\nsecond=1\n"
        {
            return Err(
                "Branch/worktree switch proof changed staging, drafts, HEAD, or linked worktree"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn verify_commit_operations(&self) -> Result<(), String> {
        let linked = self
            .root
            .parent()
            .ok_or("Missing fixture parent")?
            .join("linked worktree");
        let head_body = String::from_utf8(git(&self.root, &["show", "-s", "--format=%B", "HEAD"])?)
            .map_err(|error| error.to_string())?;
        let local_head = git(&self.root, &["rev-parse", "HEAD"])?;
        let remote_head = git(
            &self.bare_remote,
            &["rev-parse", "refs/heads/feat/branch-proof"],
        )?;
        if git(&self.root, &["symbolic-ref", "--short", "HEAD"])? != b"feat/branch-proof\n"
            || git(&self.root, &["rev-parse", "HEAD^1"])? != self.head
            || git(&self.root, &["rev-list", "--count", "HEAD"])? != b"2\n"
            || local_head != remote_head
            || head_body.trim()
                != "feat(smoke): commit edited plan verbatim\n\n- commit only staged part"
            || git(&self.root, &["show", "HEAD:part.txt"])? != b"first=2\nsecond=1\n"
            || git(&self.root, &["show", ":part.txt"])? != b"first=2\nsecond=1\n"
            || fs::read(self.root.join("part.txt")).map_err(|error| error.to_string())?
                != self.contents
            || git(&self.root, &["show", "HEAD:pnpm-lock.yaml"])? != b"lock: old\n"
            || fs::read(self.root.join("pnpm-lock.yaml")).map_err(|error| error.to_string())?
                != b"lock: updated\n"
            || git(&self.root, &["show", "HEAD:\u{65b0} \u{6587}\u{4ef6}.ts"]).is_ok()
            || fs::read(self.root.join("\u{65b0} \u{6587}\u{4ef6}.ts"))
                .map_err(|error| error.to_string())?
                != b"export const proof = true;\n"
            || git(&linked, &["symbolic-ref", "--short", "HEAD"])? != b"feat/linked\n"
            || git(&linked, &["rev-parse", "HEAD"])? != self.head
            || !git(&linked, &["status", "--porcelain"])?.is_empty()
        {
            return Err(
                "Local commit proof did not commit the exact staged scope or failed to preserve drafts/unselected files"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn verify_ide_staged_diff(&self) -> Result<(), String> {
        let log_dir = self.root.parent().ok_or("Missing fixture parent")?;
        let cwd =
            fs::read_to_string(log_dir.join("ide-cwd.txt")).map_err(|error| error.to_string())?;
        let canonical_cwd = fs::canonicalize(cwd.trim()).map_err(|error| error.to_string())?;
        let canonical_root = fs::canonicalize(&self.root).map_err(|error| error.to_string())?;
        if fs::read_to_string(log_dir.join("ide-cli.txt"))
            .map_err(|error| error.to_string())?
            .trim()
            != "idea"
            || canonical_cwd != canonical_root
            || fs::read_to_string(log_dir.join("ide-mode.txt"))
                .map_err(|error| error.to_string())?
                .trim()
                != "diff"
            || fs::read(log_dir.join("ide-left.bin")).map_err(|error| error.to_string())?
                != b"first=1\nsecond=1\n"
            || fs::read(log_dir.join("ide-right.bin")).map_err(|error| error.to_string())?
                != b"first=2\nsecond=1\n"
        {
            return Err(
                "IDE staged diff proof did not receive the exact HEAD vs index snapshots in the active worktree"
                    .into(),
            );
        }
        self.verify_unchanged()
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
    pub fn prepare_cpush_m4(&self) -> Result<(), String> {
        git(&self.root, &["add", "--", "pnpm-lock.yaml"])?;
        fs::write(
            self.root.join("RELEASE_NOTES.md"),
            b"# Release\n- C-Push M4 split verification\n",
        )
        .map_err(|e| e.to_string())
    }
    pub fn verify_cpush_m4(&self) -> Result<(), String> {
        let count = String::from_utf8(git(&self.root, &["rev-list", "--count", "HEAD"])?)
            .map_err(|e| e.to_string())?;
        let local_head = git(&self.root, &["rev-parse", "HEAD"])?;
        let remote_head = git(
            &self.bare_remote,
            &["rev-parse", "refs/heads/feat/branch-proof"],
        )?;
        let head_doc = String::from_utf8(git(&self.root, &["show", "HEAD:RELEASE_NOTES.md"])?)
            .map_err(|e| e.to_string())?;
        let prev_part = String::from_utf8(git(&self.root, &["show", "HEAD~1:part.txt"])?)
            .map_err(|e| e.to_string())?;
        let head_lock = String::from_utf8(git(&self.root, &["show", "HEAD:pnpm-lock.yaml"])?)
            .map_err(|e| e.to_string())?;
        let index_lock = String::from_utf8(git(&self.root, &["show", ":pnpm-lock.yaml"])?)
            .map_err(|e| e.to_string())?;
        let porcelain = String::from_utf8(git(&self.root, &["status", "--porcelain"])?)
            .map_err(|e| e.to_string())?;
        if count.trim() != "4"
            || local_head != remote_head
            || head_doc.trim() != "# Release\n- C-Push M4 split verification"
            || prev_part.trim() != "first=2\nsecond=3"
            || head_lock.trim() != "lock: old"
            || index_lock.trim() != "lock: updated"
            || !porcelain.lines().any(|line| line == "M  pnpm-lock.yaml")
        {
            return Err(format!(
                "M4/M5 C-Push split & push proof did not match expected 2 commits on bare remote or failed to preserve unselected staged lockfile: count={}, head_lock={}, index_lock={}, porcelain={:?}",
                count.trim(),
                head_lock.trim(),
                index_lock.trim(),
                porcelain
            ));
        }
        Ok(())
    }
    pub fn prepare_m5_conflict(&self) -> Result<(), String> {
        let parent = self.root.parent().ok_or("Missing fixture parent")?;
        let peer = parent.join("peer-clone");
        if peer.exists() {
            fs::remove_dir_all(&peer).map_err(|e| e.to_string())?;
        }
        git(
            parent,
            &[
                "clone",
                "--branch",
                "feat/branch-proof",
                self.bare_remote.to_str().ok_or("Non UTF-8 bare path")?,
                peer.to_str().ok_or("Non UTF-8 peer path")?,
            ],
        )?;
        git(&peer, &["config", "user.name", "Peer Dev"])?;
        git(&peer, &["config", "user.email", "peer@picket.invalid"])?;
        git(&peer, &["config", "commit.gpgSign", "false"])?;
        fs::write(
            peer.join("RELEASE_NOTES.md"),
            b"# Release\n- Peer remote hotfix\n",
        )
        .map_err(|e| e.to_string())?;
        git(&peer, &["add", "--", "RELEASE_NOTES.md"])?;
        git(&peer, &["commit", "-m", "docs: peer remote hotfix"])?;
        git(&peer, &["push", "origin", "feat/branch-proof"])?;

        fs::write(
            self.root.join("RELEASE_NOTES.md"),
            b"# Release\n- Local M5 note\n",
        )
        .map_err(|e| e.to_string())
    }
    pub fn verify_m5_conflict(&self) -> Result<(), String> {
        let local_head = git(&self.root, &["rev-parse", "HEAD"])?;
        let remote_head = git(
            &self.bare_remote,
            &["rev-parse", "refs/heads/feat/branch-proof"],
        )?;
        let remote_count = String::from_utf8(git(
            &self.bare_remote,
            &["rev-list", "--count", "refs/heads/feat/branch-proof"],
        )?)
        .map_err(|e| e.to_string())?;
        let merged_doc = String::from_utf8(git(
            &self.bare_remote,
            &["show", "refs/heads/feat/branch-proof:RELEASE_NOTES.md"],
        )?)
        .map_err(|e| e.to_string())?;
        let head_lock = String::from_utf8(git(&self.root, &["show", "HEAD:pnpm-lock.yaml"])?)
            .map_err(|e| e.to_string())?;
        let index_lock = String::from_utf8(git(&self.root, &["show", ":pnpm-lock.yaml"])?)
            .map_err(|e| e.to_string())?;
        let porcelain = String::from_utf8(git(&self.root, &["status", "--porcelain"])?)
            .map_err(|e| e.to_string())?;
        if local_head != remote_head
            || remote_count.trim() != "6"
            || !merged_doc.contains("Peer remote hotfix")
            || !merged_doc.contains("Local M5 note")
            || merged_doc.contains("<<<<<<<")
            || merged_doc.contains("=======")
            || merged_doc.contains(">>>>>>>")
            || head_lock.trim() != "lock: old"
            || index_lock.trim() != "lock: updated"
            || !porcelain.lines().any(|line| line == "M  pnpm-lock.yaml")
        {
            return Err(format!(
                "M5 conflict merge & push proof failed: remote_count={}, merged_doc={:?}, porcelain={:?}",
                remote_count.trim(),
                merged_doc,
                porcelain
            ));
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct SmokeState(pub Mutex<Option<Fixture>>);
impl SmokeState {
    pub fn path_and_editor_bin(&self) -> Result<(PathBuf, PathBuf), String> {
        let mut fixture = self.0.lock().map_err(|_| "Read proof is unavailable")?;
        if fixture.is_none() {
            *fixture = Some(Fixture::new()?);
        }
        let current = fixture.as_ref().unwrap();
        Ok((current.root.clone(), current.editor_bin.clone()))
    }
    pub fn path(&self) -> Result<PathBuf, String> {
        Ok(self.path_and_editor_bin()?.0)
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
    pub fn preserve_for_restart(&self) -> Result<(), String> {
        let fixture = self
            .0
            .lock()
            .map_err(|_| "Read proof is unavailable")?
            .take();
        if let Some(fixture) = fixture {
            let _ = fixture._directory.keep();
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
    pub fn verify_ide(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "IDE proof is unavailable")?
            .as_ref()
            .ok_or("IDE proof did not open")?
            .verify_ide_staged_diff()
    }
    pub fn verify(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Read proof is unavailable")?
            .as_ref()
            .ok_or("Read proof did not open")?
            .verify_unchanged()
    }
    pub fn prepare_cpush(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "C-Push smoke state unavailable")?
            .as_ref()
            .ok_or("C-Push smoke fixture not opened")?
            .prepare_cpush_m4()
    }
    pub fn verify_cpush(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "C-Push smoke state unavailable")?
            .as_ref()
            .ok_or("C-Push smoke fixture not opened")?
            .verify_cpush_m4()
    }
    pub fn prepare_m5_conflict(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "M5 conflict smoke state unavailable")?
            .as_ref()
            .ok_or("M5 conflict smoke fixture not opened")?
            .prepare_m5_conflict()
    }
    pub fn verify_m5_conflict(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "M5 conflict smoke state unavailable")?
            .as_ref()
            .ok_or("M5 conflict smoke fixture not opened")?
            .verify_m5_conflict()
    }
}
