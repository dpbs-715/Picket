//! Controlled in-app Git skill hot-sync, validation, atomic directory swap,
//! enable/disable persistence, and restore-to-bundled manager for `~/.picket/skills/`.
//!
//! Security & reliability invariants:
//! - Uses `/usr/bin/git` with discrete argument arrays (never a shell), `core.hooksPath=/dev/null`,
//!   `core.fsmonitor=false`, `core.symlinks=false`, `protocol.ext.allow=never`, and `GIT_TERMINAL_PROMPT=0`.
//! - Clones shallow (`--depth 1 --single-branch --no-tags`) into an isolated temporary directory,
//!   validates every `SKILL.md` frontmatter, file size, non-symlink file type, and relative Markdown
//!   link BEFORE touching `~/.picket/skills/`.
//! - Strips embedded URL credentials (`userinfo@`) before persisting `sourceRepo` into
//!   `~/.picket/skills/skills-manifest.json` (`0600` inside `0700` dirs).
//! - Swaps staged skills atomically on the same filesystem with rollback on failure.
//! - Active Pi Core tasks stay locked to their startup `RuntimeSkillSnapshot`; synced updates apply
//!   cleanly to subsequent tasks without mutating running sessions.

use super::{
    MAX_SKILL_FILE_BYTES, MAX_SKILL_FILES_PER_SKILL, normalize_relative_doc_path, parse_frontmatter,
};
use crate::i18n::{self, LanguageKind, deserialize_language_or_default};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::{NamedTempFile, TempDir};
use tokio::process::Command;

pub const SKILLS_PROTOCOL_VERSION: u8 = 1;
pub const DEFAULT_SKILLS_SOURCE_REPO: &str = "https://github.com/dpbs-715/skills";
pub const DEFAULT_GIT_SKILLS: &[&str] = &[
    "commit",
    "push",
    "cpush",
    "dcr",
    "create-issues",
    "create-pr",
    "review-pr",
    "submit",
];

const BUNDLED_MANIFEST_JSON: &str = include_str!("../../../agent/skills/skills-manifest.json");
const MAX_DISCOVERED_SKILLS: usize = 128;
const MAX_SOURCE_REPO_LEN: usize = 512;
const SKILL_SYNC_TIMEOUT: Duration = Duration::from_secs(60);
const SKILL_MANIFEST_FILE: &str = "skills-manifest.json";

static STAGE_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogItem {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub built_in: bool,
    pub source: &'static str,
    pub file_count: usize,
    pub reference_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsStateSnapshot {
    pub protocol_version: u8,
    pub mode: &'static str,
    pub source_repo: String,
    pub default_source_repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
    pub skills: Vec<SkillCatalogItem>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetSkillsStateRequest {
    #[allow(dead_code)]
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncSkillsRequest {
    #[serde(default)]
    pub source_repo: Option<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateEnabledSkillsRequest {
    pub enabled_skills: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetSkillsRequest {
    #[allow(dead_code)]
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSkillManifestEntry {
    name: String,
    description: String,
    entry: String,
    files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSyncedManifest {
    schema_version: u8,
    source_repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled_skills: Option<Vec<String>>,
    #[serde(default)]
    skills: Vec<StoredSkillManifestEntry>,
}

#[derive(Clone, Default)]
pub struct SkillManager {
    gate: Arc<tokio::sync::Mutex<()>>,
}

fn now_utc_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn next_nonce() -> String {
    let seq = STAGE_NONCE.fetch_add(1, Ordering::SeqCst);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{}-{ms}-{seq}", std::process::id())
}

fn is_valid_skill_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'_' | b'.'))
}

/// Strips embedded credentials (`user:pass@`) from HTTP(S)/SSH URLs before storing in manifest.
pub(crate) fn sanitize_display_source_repo(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/').trim_end_matches(".git");
    for scheme in ["https://", "http://", "ssh://"] {
        if let Some(rest) = trimmed.strip_prefix(scheme) {
            let authority_end = rest.find('/').unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            if let Some((_, host_part)) = authority.rsplit_once('@') {
                return format!("{scheme}{host_part}{}", &rest[authority_end..]);
            }
            return trimmed.to_owned();
        }
    }
    trimmed.to_owned()
}

fn validate_source_repo(
    raw: Option<&str>,
    locale: LanguageKind,
) -> Result<(String, String), String> {
    let trimmed = raw.map(str::trim).unwrap_or("");
    let candidate = if trimmed.is_empty() {
        DEFAULT_SKILLS_SOURCE_REPO
    } else {
        trimmed
    };
    if candidate.len() > MAX_SOURCE_REPO_LEN
        || candidate.starts_with('-')
        || candidate
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(i18n::skills_invalid_source_url_error(locale).into());
    }
    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("ext::") || lower.starts_with("fd::") {
        return Err(i18n::skills_invalid_source_url_error(locale).into());
    }
    let allowed = lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("ssh://")
        || lower.starts_with("file://")
        || candidate.starts_with("git@")
        || candidate.starts_with('/');
    if !allowed {
        return Err(i18n::skills_invalid_source_url_error(locale).into());
    }
    let display = sanitize_display_source_repo(candidate);
    Ok((candidate.to_owned(), display))
}

fn bundled_manifest() -> StoredSyncedManifest {
    serde_json::from_str::<StoredSyncedManifest>(BUNDLED_MANIFEST_JSON).unwrap_or_else(|_| {
        StoredSyncedManifest {
            schema_version: 1,
            source_repo: DEFAULT_SKILLS_SOURCE_REPO.into(),
            source_commit: None,
            synced_at: None,
            enabled_skills: Some(DEFAULT_GIT_SKILLS.iter().map(|s| (*s).to_owned()).collect()),
            skills: Vec::new(),
        }
    })
}

fn count_references(files: &[String]) -> usize {
    files
        .iter()
        .filter(|path| !path.ends_with("/SKILL.md") && *path != "SKILL.md")
        .count()
}

fn sort_skill_items(items: &mut [SkillCatalogItem]) {
    items.sort_by(|a, b| {
        let a_idx = DEFAULT_GIT_SKILLS
            .iter()
            .position(|k| *k == a.name)
            .unwrap_or(usize::MAX);
        let b_idx = DEFAULT_GIT_SKILLS
            .iter()
            .position(|k| *k == b.name)
            .unwrap_or(usize::MAX);
        a_idx.cmp(&b_idx).then_with(|| a.name.cmp(&b.name))
    });
}

fn load_stored_manifest(skills_dir: &Path) -> Option<StoredSyncedManifest> {
    let raw = fs::read_to_string(skills_dir.join(SKILL_MANIFEST_FILE)).ok()?;
    let parsed = serde_json::from_str::<StoredSyncedManifest>(&raw).ok()?;
    if parsed.schema_version != 1 || parsed.skills.is_empty() {
        return None;
    }
    Some(parsed)
}

pub fn build_skills_state_snapshot(storage_dir: &Path) -> SkillsStateSnapshot {
    let bundled = bundled_manifest();
    let bundled_names: HashSet<String> = bundled.skills.iter().map(|s| s.name.clone()).collect();
    let skills_dir = storage_dir.join("skills");

    if let Some(synced) = load_stored_manifest(&skills_dir) {
        let enabled_set: HashSet<&str> = match &synced.enabled_skills {
            Some(list) => list.iter().map(String::as_str).collect(),
            None => synced.skills.iter().map(|s| s.name.as_str()).collect(),
        };
        let mut items: Vec<SkillCatalogItem> = synced
            .skills
            .iter()
            .map(|entry| {
                let enabled = enabled_set.contains(entry.name.as_str());
                let built_in = bundled_names.contains(&entry.name);
                SkillCatalogItem {
                    name: entry.name.clone(),
                    description: entry.description.clone(),
                    enabled,
                    built_in,
                    source: if enabled || !built_in {
                        "synced"
                    } else {
                        "bundled"
                    },
                    file_count: entry.files.len().max(1),
                    reference_count: count_references(&entry.files),
                }
            })
            .collect();
        sort_skill_items(&mut items);
        return SkillsStateSnapshot {
            protocol_version: SKILLS_PROTOCOL_VERSION,
            mode: "synced",
            source_repo: sanitize_display_source_repo(&synced.source_repo),
            default_source_repo: DEFAULT_SKILLS_SOURCE_REPO.into(),
            source_commit: synced.source_commit,
            synced_at: synced.synced_at,
            skills: items,
        };
    }

    let mut items: Vec<SkillCatalogItem> = bundled
        .skills
        .iter()
        .map(|entry| SkillCatalogItem {
            name: entry.name.clone(),
            description: entry.description.clone(),
            enabled: true,
            built_in: true,
            source: "bundled",
            file_count: entry.files.len().max(1),
            reference_count: count_references(&entry.files),
        })
        .collect();
    sort_skill_items(&mut items);

    SkillsStateSnapshot {
        protocol_version: SKILLS_PROTOCOL_VERSION,
        mode: "bundled",
        source_repo: if bundled.source_repo.trim().is_empty() {
            DEFAULT_SKILLS_SOURCE_REPO.into()
        } else {
            sanitize_display_source_repo(&bundled.source_repo)
        },
        default_source_repo: DEFAULT_SKILLS_SOURCE_REPO.into(),
        source_commit: bundled.source_commit,
        synced_at: bundled.synced_at,
        skills: items,
    }
}

fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| {
        format!(
            "Failed to create private directory `{}`: {err}",
            dir.display()
        )
    })?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|err| {
        format!(
            "Failed to set 0700 permissions on `{}`: {err}",
            dir.display()
        )
    })?;
    Ok(())
}

fn write_manifest_atomic(skills_dir: &Path, manifest: &StoredSyncedManifest) -> Result<(), String> {
    ensure_private_dir(skills_dir)?;
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|err| format!("Failed to serialize skills manifest: {err}"))?;
    let mut temp = NamedTempFile::new_in(skills_dir)
        .map_err(|err| format!("Failed to create temporary skills manifest: {err}"))?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("Failed to set 0600 permissions on skills manifest: {err}"))?;
    temp.write_all(&encoded)
        .map_err(|err| format!("Failed to write skills manifest: {err}"))?;
    temp.write_all(b"\n")
        .map_err(|err| format!("Failed to finalize skills manifest: {err}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|err| format!("Failed to sync skills manifest: {err}"))?;
    temp.persist(skills_dir.join(SKILL_MANIFEST_FILE))
        .map_err(|err| format!("Failed to persist skills manifest: {err}"))?;
    Ok(())
}

fn strip_repo_git_overrides(cmd: &mut Command) {
    for (k, _) in std::env::vars_os() {
        let key = k.to_string_lossy();
        if key.starts_with("GIT_") && key != "GIT_SSH" && key != "GIT_SSH_COMMAND" {
            cmd.env_remove(k);
        }
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
}

async fn run_controlled_skill_git(args: &[&str]) -> Result<(bool, String, String), String> {
    let mut cmd = Command::new("/usr/bin/git");
    cmd.args([
        "--no-pager",
        "-c",
        "color.ui=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.symlinks=false",
        "-c",
        "protocol.ext.allow=never",
    ])
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    strip_repo_git_overrides(&mut cmd);

    let output = tokio::time::timeout(SKILL_SYNC_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "Git skill sync timed out after 60s.".to_owned())?
        .map_err(|err| format!("Failed to execute /usr/bin/git: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Ok((output.status.success(), stdout, stderr))
}

fn copy_validated_skill_tree(
    src_skill_root: &Path,
    src_current: &Path,
    dst_skill_root: &Path,
    files: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(src_current)
        .map_err(|err| format!("Failed to scan skill directory: {err}"))?;
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|v| v.path())).collect();
    paths.sort();

    for path in paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "evals" {
            continue;
        }
        let meta = fs::symlink_metadata(&path)
            .map_err(|err| format!("Failed to inspect `{name}`: {err}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refused symlink inside skill directory: `{}`",
                path.display()
            ));
        }
        let rel = path
            .strip_prefix(src_skill_root)
            .map_err(|_| "Skill file escaped skill root".to_owned())?;
        let rel_str = rel
            .to_str()
            .ok_or_else(|| "Skill file path is not valid UTF-8".to_owned())?
            .replace('\\', "/");

        if meta.is_dir() {
            let dst_dir = dst_skill_root.join(rel);
            ensure_private_dir(&dst_dir)?;
            copy_validated_skill_tree(src_skill_root, &path, dst_skill_root, files)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if files.len() >= MAX_SKILL_FILES_PER_SKILL {
            return Err(format!(
                "Skill exceeded maximum file limit ({MAX_SKILL_FILES_PER_SKILL} files)."
            ));
        }
        if meta.len() > MAX_SKILL_FILE_BYTES {
            return Err(format!(
                "Skill file `{rel_str}` exceeds maximum allowed size ({MAX_SKILL_FILE_BYTES} bytes)."
            ));
        }
        let clean_rel = normalize_relative_doc_path(&rel_str)?;
        let bytes = fs::read(&path)
            .map_err(|err| format!("Failed to read skill file `{clean_rel}`: {err}"))?;
        let dst_file = dst_skill_root.join(&clean_rel);
        if let Some(parent) = dst_file.parent() {
            ensure_private_dir(parent)?;
        }
        fs::write(&dst_file, &bytes)
            .map_err(|err| format!("Failed to stage skill file `{clean_rel}`: {err}"))?;
        fs::set_permissions(&dst_file, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("Failed to set permissions on `{clean_rel}`: {err}"))?;
        files.push(clean_rel);
    }
    Ok(())
}

fn resolve_normalized_relative(base_dir: &Path, target: &str) -> Result<String, String> {
    let mut stack: Vec<String> = Vec::new();
    for comp in base_dir.components() {
        if let Component::Normal(seg) = comp {
            stack.push(seg.to_string_lossy().into_owned());
        }
    }
    for comp in Path::new(target).components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err(format!("Relative link escapes skill directory: `{target}`"));
                }
            }
            Component::Normal(seg) => {
                stack.push(seg.to_string_lossy().into_owned());
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Relative link cannot be absolute: `{target}`"));
            }
        }
    }
    if stack.is_empty() {
        return Err(format!("Empty relative link target: `{target}`"));
    }
    Ok(stack.join("/"))
}

fn has_url_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

fn validate_markdown_relative_links(
    skill_name: &str,
    rel_file: &str,
    markdown: &str,
    existing_files: &HashSet<String>,
) -> Result<(), String> {
    let base_dir = Path::new(rel_file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let bytes = markdown.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b')' && bytes[end] != b'\n' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b')' {
                let raw = markdown[start..end]
                    .trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>');
                // Strip optional title `"title"` after whitespace.
                let target_only = raw.split_whitespace().next().unwrap_or("");
                if !target_only.is_empty()
                    && !target_only.starts_with('#')
                    && !target_only.starts_with('/')
                    && !has_url_scheme(target_only)
                    && !target_only.contains("{{REPO_ROOT}}")
                {
                    let without_fragment = target_only.split('#').next().unwrap_or("");
                    if !without_fragment.is_empty() {
                        let resolved = resolve_normalized_relative(base_dir, without_fragment)
                            .map_err(|e| format!("In skill `{skill_name}/{rel_file}`: {e}"))?;
                        if !existing_files.contains(&resolved) {
                            return Err(format!(
                                "Broken relative link in `{skill_name}/{rel_file}`: `{target_only}` (resolved `{resolved}` not found)."
                            ));
                        }
                    }
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    Ok(())
}

fn stage_skills_from_clone(
    clone_root: &Path,
    stage_dir: &Path,
) -> Result<Vec<StoredSkillManifestEntry>, String> {
    let scan_root = if clone_root.join("skills").is_dir() {
        clone_root.join("skills")
    } else {
        clone_root.to_path_buf()
    };
    let entries = fs::read_dir(&scan_root)
        .map_err(|err| format!("Failed to read cloned skills directory: {err}"))?;
    let mut candidate_dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|v| v.path()))
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            !name.starts_with('.')
                && fs::symlink_metadata(path)
                    .map(|m| m.is_dir() && !m.file_type().is_symlink())
                    .unwrap_or(false)
        })
        .collect();
    candidate_dirs.sort();

    let mut discovered = Vec::new();
    let mut seen_names = HashSet::new();
    for dir in candidate_dirs {
        if discovered.len() >= MAX_DISCOVERED_SKILLS {
            break;
        }
        let skill_md_path = dir.join("SKILL.md");
        let Ok(md_meta) = fs::symlink_metadata(&skill_md_path) else {
            continue;
        };
        if !md_meta.is_file() || md_meta.file_type().is_symlink() {
            continue;
        }
        if md_meta.len() > MAX_SKILL_FILE_BYTES {
            return Err(format!(
                "SKILL.md in `{}` exceeds max file size.",
                dir.display()
            ));
        }
        let raw_md = fs::read_to_string(&skill_md_path)
            .map_err(|err| format!("Failed to read `{}`: {err}", skill_md_path.display()))?;
        let Some((name, description)) = parse_frontmatter(&raw_md) else {
            return Err(format!(
                "Missing valid YAML frontmatter (name/description) in `{}`.",
                skill_md_path.display()
            ));
        };
        if !is_valid_skill_name(&name) {
            return Err(format!("Invalid skill name `{name}` in frontmatter."));
        }
        if !seen_names.insert(name.clone()) {
            return Err(format!("Duplicate skill name `{name}` in repository."));
        }

        let dst_skill_dir = stage_dir.join(&name);
        ensure_private_dir(&dst_skill_dir)?;
        let mut rel_files = Vec::new();
        copy_validated_skill_tree(&dir, &dir, &dst_skill_dir, &mut rel_files)?;
        rel_files.sort();
        if !rel_files.iter().any(|f| f == "SKILL.md") {
            return Err(format!("Skill `{name}` is missing SKILL.md after staging."));
        }
        let file_set: HashSet<String> = rel_files.iter().cloned().collect();
        for rel_file in &rel_files {
            if rel_file.ends_with(".md") {
                let content = fs::read_to_string(dst_skill_dir.join(rel_file))
                    .map_err(|err| format!("Failed to read staged `{name}/{rel_file}`: {err}"))?;
                validate_markdown_relative_links(&name, rel_file, &content, &file_set)?;
            }
        }
        discovered.push(StoredSkillManifestEntry {
            name: name.clone(),
            description,
            entry: format!("{name}/SKILL.md"),
            files: rel_files
                .into_iter()
                .map(|file| format!("{name}/{file}"))
                .collect(),
        });
    }

    if discovered.is_empty() {
        return Err("No valid SKILL.md definitions were found in the repository.".into());
    }
    Ok(discovered)
}

fn resolve_enabled_skills(
    discovered: &[StoredSkillManifestEntry],
    previous: Option<&StoredSyncedManifest>,
    display_repo: &str,
) -> Vec<String> {
    let discovered_set: HashSet<&str> = discovered.iter().map(|s| s.name.as_str()).collect();

    if let Some(prev) = previous
        && sanitize_display_source_repo(&prev.source_repo) == display_repo
        && let Some(ref prev_enabled) = prev.enabled_skills
    {
        return prev_enabled
            .iter()
            .filter(|name| discovered_set.contains(name.as_str()))
            .cloned()
            .collect();
    }

    let default_matches: Vec<String> = DEFAULT_GIT_SKILLS
        .iter()
        .filter(|name| discovered_set.contains(**name))
        .map(|name| (*name).to_owned())
        .collect();
    if !default_matches.is_empty() {
        return default_matches;
    }

    discovered.iter().map(|s| s.name.clone()).collect()
}

fn swap_staged_skills_dir(storage_dir: &Path, stage_dir: &Path) -> Result<(), String> {
    let target = storage_dir.join("skills");
    let backup = storage_dir.join(format!(".skills-prev-{}", next_nonce()));
    let had_target = target.exists();
    if had_target {
        fs::rename(&target, &backup).map_err(|err| {
            format!("Failed to prepare existing skills dir for atomic swap: {err}")
        })?;
    }
    if let Err(err) = fs::rename(stage_dir, &target) {
        if had_target {
            let _ = fs::rename(&backup, &target);
        }
        return Err(format!("Failed to activate synced skills directory: {err}"));
    }
    if had_target {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

impl SkillManager {
    pub async fn get_state(&self, storage_dir: &Path) -> SkillsStateSnapshot {
        let _guard = self.gate.lock().await;
        build_skills_state_snapshot(storage_dir)
    }

    pub async fn sync_skills(
        &self,
        storage_dir: &Path,
        request: &SyncSkillsRequest,
    ) -> Result<SkillsStateSnapshot, String> {
        let _guard = self.gate.lock().await;
        let locale = request.locale;
        let (clone_url, display_repo) =
            validate_source_repo(request.source_repo.as_deref(), locale)?;

        ensure_private_dir(storage_dir)?;
        let clone_temp = TempDir::new_in(storage_dir)
            .map_err(|err| format!("Failed to allocate temporary clone directory: {err}"))?;
        let clone_dir = clone_temp.path().join("repo");
        let clone_dir_str = clone_dir.to_string_lossy().into_owned();

        let (ok_clone, _, err_clone) = run_controlled_skill_git(&[
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "--no-tags",
            "--",
            &clone_url,
            &clone_dir_str,
        ])
        .await?;
        if !ok_clone {
            let sanitized_err = err_clone.replace(&clone_url, &display_repo);
            return Err(i18n::skills_sync_clone_failed_error(locale, &sanitized_err));
        }

        let (ok_head, head_out, _) =
            run_controlled_skill_git(&["-C", &clone_dir_str, "rev-parse", "HEAD"]).await?;
        let source_commit = ok_head
            .then(|| head_out.trim().to_owned())
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()));

        let stage_dir = storage_dir.join(format!(".skills-stage-{}", next_nonce()));
        ensure_private_dir(&stage_dir)?;
        let stage_result = (|| -> Result<(), String> {
            let discovered = stage_skills_from_clone(&clone_dir, &stage_dir)?;
            let previous = load_stored_manifest(&storage_dir.join("skills"));
            let enabled_skills =
                resolve_enabled_skills(&discovered, previous.as_ref(), &display_repo);
            let manifest = StoredSyncedManifest {
                schema_version: 1,
                source_repo: display_repo.clone(),
                source_commit,
                synced_at: Some(now_utc_iso()),
                enabled_skills: Some(enabled_skills),
                skills: discovered,
            };
            write_manifest_atomic(&stage_dir, &manifest)?;
            swap_staged_skills_dir(storage_dir, &stage_dir)?;
            Ok(())
        })();

        if let Err(err) = stage_result {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(i18n::skills_sync_validation_failed_error(locale, &err));
        }

        Ok(build_skills_state_snapshot(storage_dir))
    }

    pub async fn set_enabled_skills(
        &self,
        storage_dir: &Path,
        request: &UpdateEnabledSkillsRequest,
    ) -> Result<SkillsStateSnapshot, String> {
        let _guard = self.gate.lock().await;
        let locale = request.locale;
        let skills_dir = storage_dir.join("skills");
        let Some(mut manifest) = load_stored_manifest(&skills_dir) else {
            return Err(i18n::skills_not_synced_yet_error(locale).into());
        };
        let available: HashSet<&str> = manifest.skills.iter().map(|s| s.name.as_str()).collect();
        let mut seen = HashSet::new();
        let next_enabled: Vec<String> = request
            .enabled_skills
            .iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| available.contains(s.as_str()) && seen.insert(s.clone()))
            .collect();
        manifest.enabled_skills = Some(next_enabled);
        write_manifest_atomic(&skills_dir, &manifest)?;
        Ok(build_skills_state_snapshot(storage_dir))
    }

    pub async fn reset_to_bundled(
        &self,
        storage_dir: &Path,
        _request: &ResetSkillsRequest,
    ) -> Result<SkillsStateSnapshot, String> {
        let _guard = self.gate.lock().await;
        let target = storage_dir.join("skills");
        if target.exists() {
            let trash = storage_dir.join(format!(".skills-reset-{}", next_nonce()));
            if fs::rename(&target, &trash).is_ok() {
                let _ = fs::remove_dir_all(&trash);
            } else {
                fs::remove_dir_all(&target)
                    .map_err(|err| format!("Failed to remove synced skills directory: {err}"))?;
            }
        }
        Ok(build_skills_state_snapshot(storage_dir))
    }
}
