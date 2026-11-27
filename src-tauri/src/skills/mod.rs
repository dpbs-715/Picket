//! Dual-layer runtime skill snapshot loader for `~/.picket/skills/`.
//! Reads optional user-synced skill overrides once per task startup so active Pi Core sessions
//! remain snapshot-locked and never mutate mid-run. If `~/.picket/skills/` is absent, an empty
//! snapshot is returned and Pi Core transparently uses its bundled fallback skills.

pub mod sync;
#[cfg(test)]
mod sync_tests;

pub use sync::SkillManager;

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub(crate) const MAX_SKILL_FILE_BYTES: u64 = 256 * 1024;
pub(crate) const MAX_SKILL_FILES_PER_SKILL: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSkillOverrideEntry {
    pub name: String,
    pub description: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSkillOverrideCatalog {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    pub skills: Vec<RuntimeSkillOverrideEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_skills: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeSkillSnapshot {
    pub catalog: RuntimeSkillOverrideCatalog,
    docs: BTreeMap<(String, String), String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSkillManifest {
    #[serde(default)]
    source_repo: Option<String>,
    #[serde(default)]
    source_commit: Option<String>,
    #[serde(default)]
    enabled_skills: Option<Vec<String>>,
}

impl RuntimeSkillSnapshot {
    pub fn is_empty(&self) -> bool {
        self.catalog.skills.is_empty() && self.catalog.disabled_skills.is_empty()
    }

    pub fn catalog_option(&self) -> Option<RuntimeSkillOverrideCatalog> {
        if self.is_empty() {
            None
        } else {
            Some(self.catalog.clone())
        }
    }

    pub fn read_doc(&self, skill: &str, relative_path: &str) -> Result<Option<&str>, String> {
        let clean_skill = skill.trim();
        let clean_path = normalize_relative_doc_path(relative_path)?;
        Ok(self
            .docs
            .get(&(clean_skill.to_owned(), clean_path))
            .map(String::as_str))
    }

    pub fn load_from_data_dir(storage_dir: &Path) -> Self {
        Self::load_from_skills_dir(&storage_dir.join("skills"))
    }

    pub fn load_from_skills_dir(skills_dir: &Path) -> Self {
        if !skills_dir.is_dir() {
            return Self::default();
        }
        let manifest_meta = fs::read_to_string(skills_dir.join("skills-manifest.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredSkillManifest>(&raw).ok());
        let enabled_filter = manifest_meta
            .as_ref()
            .and_then(|meta| meta.enabled_skills.as_ref())
            .map(|list| {
                list.iter()
                    .cloned()
                    .collect::<std::collections::HashSet<_>>()
            });

        let scan_root = if skills_dir.join("skills").is_dir() {
            skills_dir.join("skills")
        } else {
            skills_dir.to_path_buf()
        };

        let Ok(entries) = fs::read_dir(&scan_root) else {
            return Self::default();
        };

        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| !n.starts_with('.'))
            })
            .collect();
        dirs.sort();

        let mut disabled_set: BTreeSet<String> = BTreeSet::new();
        if let Some(ref allowed) = enabled_filter {
            for bundled_name in sync::bundled_skill_names() {
                if !allowed.contains(&bundled_name) {
                    disabled_set.insert(bundled_name);
                }
            }
        }

        let mut snapshot = Self {
            catalog: RuntimeSkillOverrideCatalog {
                source_repo: manifest_meta
                    .as_ref()
                    .and_then(|meta| meta.source_repo.clone()),
                source_commit: manifest_meta
                    .as_ref()
                    .and_then(|meta| meta.source_commit.clone()),
                skills: Vec::new(),
                disabled_skills: Vec::new(),
            },
            docs: BTreeMap::new(),
        };

        for skill_dir in dirs {
            let skill_md_path = skill_dir.join("SKILL.md");
            let Ok(meta) = fs::symlink_metadata(&skill_md_path) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_SKILL_FILE_BYTES {
                continue;
            }
            let Ok(skill_md) = fs::read_to_string(&skill_md_path) else {
                continue;
            };
            let Some((name, description)) = parse_frontmatter(&skill_md) else {
                continue;
            };
            if let Some(ref allowed) = enabled_filter
                && !allowed.contains(&name)
            {
                disabled_set.insert(name);
                continue;
            }
            let mut files = Vec::new();
            collect_markdown_docs(
                &skill_dir,
                &skill_dir,
                &name,
                &mut files,
                &mut snapshot.docs,
            );
            if !files.iter().any(|file| file == "SKILL.md") {
                continue;
            }
            files.sort();
            snapshot.catalog.skills.push(RuntimeSkillOverrideEntry {
                name,
                description,
                files,
            });
        }

        snapshot.catalog.disabled_skills = disabled_set.into_iter().collect();
        snapshot
    }
}

fn collect_markdown_docs(
    skill_root: &Path,
    current_dir: &Path,
    skill_name: &str,
    files: &mut Vec<String>,
    docs: &mut BTreeMap<(String, String), String>,
) {
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|v| v.path())).collect();
    paths.sort();
    for path in paths {
        if files.len() >= MAX_SKILL_FILES_PER_SKILL {
            break;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_markdown_docs(skill_root, &path, skill_name, files, docs);
            continue;
        }
        if !meta.is_file() || !file_name.ends_with(".md") || meta.len() > MAX_SKILL_FILE_BYTES {
            continue;
        }
        let Ok(rel) = path.strip_prefix(skill_root) else {
            continue;
        };
        let Some(rel_str) = rel.to_str() else {
            continue;
        };
        let normalized = rel_str.replace('\\', "/");
        if normalize_relative_doc_path(&normalized).is_err() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            files.push(normalized.clone());
            docs.insert((skill_name.to_owned(), normalized), content);
        }
    }
}

pub fn normalize_relative_doc_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_start_matches('/');
    let candidate = if trimmed.is_empty() {
        "SKILL.md"
    } else {
        trimmed
    };
    if candidate.contains('\0') || candidate.contains('\\') {
        return Err(format!("Invalid skill document path: {candidate}"));
    }
    let segments: Vec<&str> = candidate.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(format!(
            "Skill document path must stay inside the skill directory: {candidate}"
        ));
    }
    Ok(segments.join("/"))
}

pub(crate) fn parse_frontmatter(markdown: &str) -> Option<(String, String)> {
    let rest = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let block = &rest[..end];
    let mut name = None;
    let mut description = None;
    for line in block.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let mut val = val.trim().to_owned();
        if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
            || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
        {
            val = val[1..val.len() - 1].to_owned();
        }
        if key == "name" && !val.is_empty() {
            name = Some(val);
        } else if key == "description" && !val.is_empty() {
            description = Some(val);
        }
    }
    Some((name?, description?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_runtime_skill_overrides_with_references_and_blocks_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let skills_root = dir.path().join("skills");
        fs::create_dir_all(skills_root.join("dcr/references")).unwrap();
        fs::write(
            skills_root.join("skills-manifest.json"),
            r#"{"sourceRepo":"https://github.com/dpbs-715/skills","sourceCommit":"abc1234"}"#,
        )
        .unwrap();
        fs::write(
            skills_root.join("dcr/SKILL.md"),
            "---\nname: dcr\ndescription: \"Overridden diff review\"\n---\n\n# DCR Override\n",
        )
        .unwrap();
        fs::write(skills_root.join("dcr/references/extra.md"), "# Extra Ref\n").unwrap();

        let snapshot = RuntimeSkillSnapshot::load_from_data_dir(dir.path());
        assert!(!snapshot.is_empty());
        assert_eq!(snapshot.catalog.source_commit.as_deref(), Some("abc1234"));
        assert_eq!(snapshot.catalog.skills.len(), 1);
        assert_eq!(snapshot.catalog.skills[0].name, "dcr");
        assert!(
            snapshot
                .read_doc("dcr", "SKILL.md")
                .unwrap()
                .unwrap()
                .contains("DCR Override")
        );
        assert!(
            snapshot
                .read_doc("dcr", "references/extra.md")
                .unwrap()
                .unwrap()
                .contains("Extra Ref")
        );
        assert!(snapshot.read_doc("dcr", "../secret.md").is_err());
    }

    #[test]
    fn missing_skills_dir_yields_empty_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = RuntimeSkillSnapshot::load_from_data_dir(dir.path());
        assert!(snapshot.is_empty());
        assert!(snapshot.catalog_option().is_none());
    }
}
