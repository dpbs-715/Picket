//! Validation and staging helpers for cloned skill repositories.

use super::{MAX_DISCOVERED_SKILLS, StoredSkillManifestEntry, ensure_private_dir};
use crate::skills::{
    MAX_SKILL_FILE_BYTES, MAX_SKILL_FILES_PER_SKILL, normalize_relative_doc_path, parse_frontmatter,
};
use std::{
    collections::HashSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

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

pub(super) fn stage_skills_from_clone(
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
