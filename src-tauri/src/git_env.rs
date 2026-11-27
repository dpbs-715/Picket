//! Shared Git process environment sanitization helpers.
//!
//! Strips ambient `GIT_*` overrides (such as `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`,
//! `GIT_CONFIG_*`) so caller environments cannot redirect repository operations, while
//! optionally preserving `GIT_SSH` and `GIT_SSH_COMMAND` for authenticated remote operations.

use std::ffi::OsStr;

fn should_strip_git_env_key(key: &OsStr, preserve_ssh: bool) -> bool {
    let key_str = key.to_string_lossy();
    if !key_str.starts_with("GIT_") {
        return false;
    }
    if preserve_ssh && matches!(key_str.as_ref(), "GIT_SSH" | "GIT_SSH_COMMAND") {
        return false;
    }
    true
}

pub fn strip_git_overrides(cmd: &mut tokio::process::Command, preserve_ssh: bool) {
    for (key, _) in std::env::vars_os() {
        if should_strip_git_env_key(&key, preserve_ssh) {
            cmd.env_remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn preserves_ssh_env_only_when_requested_and_always_strips_repo_overrides() {
        assert!(should_strip_git_env_key(&OsString::from("GIT_DIR"), true));
        assert!(should_strip_git_env_key(
            &OsString::from("GIT_INDEX_FILE"),
            true
        ));
        assert!(should_strip_git_env_key(
            &OsString::from("GIT_CONFIG_PARAMETERS"),
            true
        ));
        assert!(!should_strip_git_env_key(&OsString::from("GIT_SSH"), true));
        assert!(!should_strip_git_env_key(
            &OsString::from("GIT_SSH_COMMAND"),
            true
        ));
        assert!(should_strip_git_env_key(&OsString::from("GIT_SSH"), false));
        assert!(should_strip_git_env_key(
            &OsString::from("GIT_SSH_COMMAND"),
            false
        ));
        assert!(!should_strip_git_env_key(&OsString::from("PATH"), false));
    }
}
