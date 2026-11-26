use crate::repository::{
    Result, hash,
    parse::{self, StatusEntry},
    runner::GitOutput,
    staged,
    types::Change,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone)]
pub(in crate::repository) struct CommitRegistration {
    pub branch: String,
    pub detached: bool,
    pub head: Option<String>,
    pub files: HashMap<String, AuthorizedFile>,
}

#[derive(Clone)]
pub(in crate::repository) struct AuthorizedFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: bool,
    pub range_kind: String,
    pub conflicted: bool,
    pub stage_entries: Vec<u8>,
    pub working_hash: Option<String>,
    pub token: String,
}

impl AuthorizedFile {
    pub(super) fn unit_paths(&self) -> BTreeSet<String> {
        let mut paths = BTreeSet::new();
        paths.insert(self.path.clone());
        if let Some(previous) = &self.previous_path {
            paths.insert(previous.clone());
        }
        paths
    }
}

pub(super) fn ensure_safe_repo_path(path: &str) -> Result<()> {
    parse::relative(path)?;
    if path
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err("Commit scope contains an unsafe .git path".into());
    }
    Ok(())
}

pub(in crate::repository) fn stage_entries_by_path(
    index_bytes: &[u8],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    for record in index_bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("Git index entry format mismatch".into());
        };
        let header = parse::utf8(&record[..tab])?;
        if header.split_whitespace().count() != 3 {
            return Err("Git index header format mismatch".into());
        }
        let path = parse::utf8(&record[tab + 1..])?;
        parse::relative(&path)?;
        let slot = entries.entry(path).or_default();
        slot.extend_from_slice(record);
        slot.push(0);
    }
    Ok(entries)
}

pub(in crate::repository) fn gitlink_entries_from_stage(
    index_bytes: &[u8],
) -> Result<BTreeMap<String, String>> {
    let mut entries = BTreeMap::<String, String>::new();
    for record in index_bytes
        .split(|byte| *byte == 0)
        .filter(|record| record.starts_with(b"160000 "))
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("Git index entry format mismatch".into());
        };
        let header = parse::utf8(&record[..tab])?;
        let mut parts = header.split_whitespace();
        let mode = parts.next();
        let oid = parts.next();
        let stage = parts.next();
        if mode != Some("160000") || oid.is_none() || stage.is_none() || parts.next().is_some() {
            return Err("Git index header format mismatch".into());
        }
        let path = parse::utf8(&record[tab + 1..])?;
        parse::relative(&path)?;
        entries.insert(path, oid.unwrap().to_owned());
    }
    Ok(entries)
}

pub(in crate::repository) fn path_conflicts_with_submodule(
    path: &str,
    submodule_path: &str,
) -> bool {
    path == submodule_path
        || path
            .strip_prefix(submodule_path)
            .is_some_and(|rest| rest.starts_with('/'))
        || submodule_path
            .strip_prefix(path)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(super) fn collect_unit_stage_entries(
    stage_map: &BTreeMap<String, Vec<u8>>,
    unit_paths: &BTreeSet<String>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for path in unit_paths {
        if let Some(records) = stage_map.get(path) {
            bytes.extend_from_slice(records);
        }
    }
    bytes
}

pub(super) fn output_detail(output: &GitOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.bytes);
    let combined = match (stderr.trim(), stdout.trim()) {
        ("", "") => format!("exit code {:?}", output.code),
        (err, "") => err.to_owned(),
        ("", out) => out.to_owned(),
        (err, out) => format!("{err}; {out}"),
    };
    let single_line = combined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let truncated: String = chars.by_ref().take(320).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

impl CommitRegistration {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::repository) fn build(
        repository_id: &str,
        worktree_id: &str,
        branch: &str,
        detached: bool,
        head: Option<&str>,
        entries: &[StatusEntry],
        changes: &mut [Change],
        working_hashes: &HashMap<String, String>,
        index_bytes: &[u8],
    ) -> Result<Self> {
        let stage_map = stage_entries_by_path(index_bytes)?;
        let mut files = HashMap::with_capacity(changes.len());
        for (entry, change) in entries.iter().zip(changes.iter_mut()) {
            let is_staged = staged(entry);
            let previous_path = (is_staged && entry.xy.starts_with('R'))
                .then(|| entry.previous.clone())
                .flatten();
            let mut unit_paths = BTreeSet::new();
            unit_paths.insert(entry.path.clone());
            if let Some(previous) = &previous_path {
                unit_paths.insert(previous.clone());
            }
            let stage_entries = collect_unit_stage_entries(&stage_map, &unit_paths);
            let working_hash = if is_staged {
                None
            } else {
                Some(
                    working_hashes
                        .get(&entry.path)
                        .cloned()
                        .ok_or("Missing content hash for unstaged file")?,
                )
            };
            let payload = serde_json::to_vec(&(
                "picket-commit-v1",
                repository_id,
                worktree_id,
                branch,
                detached,
                head,
                &change.id,
                &entry.path,
                &previous_path,
                is_staged,
                &change.range.kind,
                entry.conflicted,
                change.local_keep,
                &stage_entries,
                &working_hash,
            ))
            .map_err(|error| error.to_string())?;
            let token = hash(payload);
            change.commit_token = token.clone();
            files.insert(
                change.id.clone(),
                AuthorizedFile {
                    path: entry.path.clone(),
                    previous_path,
                    staged: is_staged,
                    range_kind: change.range.kind.clone(),
                    conflicted: entry.conflicted,
                    stage_entries,
                    working_hash,
                    token,
                },
            );
        }
        Ok(Self {
            branch: branch.to_owned(),
            detached,
            head: head.map(str::to_owned),
            files,
        })
    }
}
