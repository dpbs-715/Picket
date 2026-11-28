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

mod git;
mod validation;

pub(crate) use git::sanitize_display_source_repo;

use crate::i18n::{self, LanguageKind, deserialize_language_or_default};
use git::{run_controlled_skill_git, validate_source_repo};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use validation::stage_skills_from_clone;

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

const BUNDLED_MANIFEST_JSON: &str = include_str!("../../../../agent/skills/skills-manifest.json");
const MAX_DISCOVERED_SKILLS: usize = 128;
const MAX_SOURCE_REPO_LEN: usize = 512;
const SKILL_SYNC_TIMEOUT: Duration = Duration::from_secs(60);
const SKILL_MANIFEST_FILE: &str = "skills-manifest.json";

static STAGE_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogItem {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub built_in: bool,
    #[cfg_attr(test, ts(type = "'bundled' | 'synced'"))]
    pub source: &'static str,
    pub file_count: usize,
    pub reference_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SkillsStateSnapshot {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    #[cfg_attr(test, ts(type = "'bundled' | 'synced'"))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetSkillsStateRequest {
    #[allow(dead_code)]
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncSkillsRequest {
    #[serde(default)]
    pub source_repo: Option<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateEnabledSkillsRequest {
    pub enabled_skills: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetSkillsRequest {
    #[allow(dead_code)]
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredSkillManifestEntry {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) entry: String,
    pub(super) files: Vec<String>,
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
    crate::time::now_utc_iso()
}

fn next_nonce() -> String {
    let seq = STAGE_NONCE.fetch_add(1, Ordering::SeqCst);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{}-{ms}-{seq}", std::process::id())
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

pub(crate) fn bundled_skill_names() -> Vec<String> {
    let manifest = bundled_manifest();
    if !manifest.skills.is_empty() {
        manifest.skills.into_iter().map(|s| s.name).collect()
    } else {
        DEFAULT_GIT_SKILLS.iter().map(|s| (*s).to_owned()).collect()
    }
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

pub(super) fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    crate::storage::ensure_private_dir(dir)
}

fn write_manifest_atomic(skills_dir: &Path, manifest: &StoredSyncedManifest) -> Result<(), String> {
    crate::storage::write_json_atomic(
        skills_dir,
        &skills_dir.join(SKILL_MANIFEST_FILE),
        manifest,
        true,
    )
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
            return Err(i18n::skills::skills_sync_clone_failed_error(
                locale,
                &sanitized_err,
            ));
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
            return Err(i18n::skills::skills_sync_validation_failed_error(
                locale, &err,
            ));
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
            return Err(i18n::skills::skills_not_synced_yet_error(locale).into());
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
