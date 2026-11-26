use crate::i18n::{self, LanguageKind};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

pub fn utf8(bytes: &[u8]) -> Result<String, String> {
    utf8_with_locale(bytes, LanguageKind::En)
}
pub fn utf8_with_locale(bytes: &[u8], locale: LanguageKind) -> Result<String, String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| i18n::repository::parse_non_utf8_path_error(locale).into())
}
pub fn relative(path: &str) -> Result<(), String> {
    if path.is_empty()
        || !Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("Git returned an unsafe file path".into());
    }
    Ok(())
}
#[derive(Clone, Debug)]
pub struct StatusEntry {
    pub path: String,
    pub previous: Option<String>,
    pub xy: String,
    pub index_oid: String,
    pub mode: String,
    pub conflicted: bool,
}
#[derive(Default)]
pub struct Status {
    pub head: Option<String>,
    pub branch: String,
    pub upstream: Option<String>,
    pub divergence: Option<(u64, u64)>,
    pub entries: Vec<StatusEntry>,
}
pub fn status_with_locale(bytes: &[u8], locale: LanguageKind) -> Result<Status, String> {
    let mut parsed = Status::default();
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let line = utf8_with_locale(record, locale)?;
        if let Some(head) = line.strip_prefix("# branch.oid ") {
            parsed.head = (head != "(initial)").then(|| head.to_owned());
        } else if let Some(branch) = line.strip_prefix("# branch.head ") {
            parsed.branch = branch.to_owned();
        } else if let Some(upstream) = line.strip_prefix("# branch.upstream ") {
            parsed.upstream = Some(upstream.to_owned());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let (ahead, behind) = ab
                .split_once(' ')
                .ok_or("Git ahead/behind status format mismatch")?;
            parsed.divergence = Some((
                ahead
                    .trim_start_matches('+')
                    .parse()
                    .map_err(|_| "Invalid ahead commit count")?,
                behind
                    .trim_start_matches('-')
                    .parse()
                    .map_err(|_| "Invalid behind commit count")?,
            ));
        } else if let Some(path) = line.strip_prefix("? ") {
            relative(path)?;
            parsed.entries.push(StatusEntry {
                path: path.into(),
                previous: None,
                xy: "??".into(),
                index_oid: String::new(),
                mode: "100644".into(),
                conflicted: false,
            });
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            let count = if line.starts_with("1 ") {
                9
            } else if line.starts_with("u ") {
                11
            } else {
                10
            };
            let fields: Vec<_> = line.splitn(count, ' ').collect();
            if fields.len() != count {
                return Err("Git file status format mismatch".into());
            }
            let path = fields[count - 1].to_owned();
            relative(&path)?;
            let previous = if fields[0] == "2" {
                let previous = utf8_with_locale(
                    records
                        .next()
                        .ok_or("Git rename entry is missing source path")?,
                    locale,
                )?;
                relative(&previous)?;
                Some(previous)
            } else {
                None
            };
            let is_submodule = fields[2].starts_with('S')
                || if fields[0] == "u" {
                    fields[3..=6].contains(&"160000")
                } else {
                    fields[3..=5].contains(&"160000")
                };
            parsed.entries.push(StatusEntry {
                path,
                previous,
                xy: fields[1].into(),
                index_oid: if fields[0] == "u" {
                    fields[7..10].join(":")
                } else {
                    fields[7].into()
                },
                mode: if is_submodule {
                    "160000".into()
                } else {
                    fields[4].into()
                },
                conflicted: fields[0] == "u",
            });
        } else if !line.starts_with("# ") && !line.starts_with("! ") {
            return Err("Unrecognized Git status record format".into());
        }
    }
    if parsed.branch.is_empty() {
        return Err("Git did not report current branch status".into());
    }
    let mut unique = BTreeMap::<String, StatusEntry>::new();
    for mut entry in parsed.entries {
        if let Some(previous) = unique.get_mut(&entry.path) {
            if entry.xy == "??" && previous.xy != "??" && !previous.conflicted {
                previous.xy = format!("{}?", &previous.xy[..1]);
            } else if previous.xy == "??" && entry.xy != "??" && !entry.conflicted {
                entry.xy = format!("{}?", &entry.xy[..1]);
                unique.insert(entry.path.clone(), entry);
            } else {
                return Err(
                    "Git returned incompatible duplicate status entries; incomplete list was not loaded"
                        .into(),
                );
            }
        } else {
            unique.insert(entry.path.clone(), entry);
        }
    }
    parsed.entries = unique.into_values().collect();
    Ok(parsed)
}
#[derive(Clone, Default, Debug)]
pub struct Stats {
    pub added: u64,
    pub removed: u64,
    pub binary: bool,
}
pub fn numstat(bytes: &[u8]) -> Result<BTreeMap<String, Stats>, String> {
    let mut parsed = BTreeMap::new();
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let line = utf8(record)?;
        let fields: Vec<_> = line.splitn(3, '\t').collect();
        if fields.len() != 3 {
            return Err("Git numstat format mismatch".into());
        }
        let path = if fields[2].is_empty() {
            let old = utf8(
                records
                    .next()
                    .ok_or("Git rename numstat missing source path")?,
            )?;
            relative(&old)?;
            utf8(
                records
                    .next()
                    .ok_or("Git rename numstat missing target path")?,
            )?
        } else {
            fields[2].into()
        };
        relative(&path)?;
        parsed.insert(path, parse_stats(fields[0], fields[1])?);
    }
    Ok(parsed)
}
fn parse_stats(added: &str, removed: &str) -> Result<Stats, String> {
    let binary = added == "-" || removed == "-";
    Ok(Stats {
        added: if binary {
            0
        } else {
            added.parse().map_err(|_| "Invalid added line count")?
        },
        removed: if binary {
            0
        } else {
            removed.parse().map_err(|_| "Invalid removed line count")?
        },
        binary,
    })
}
pub fn untracked_numstat(bytes: &[u8], expected_path: &str) -> Result<Stats, String> {
    relative(expected_path)?;
    let records: Vec<_> = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect();
    let header = utf8(
        records
            .first()
            .ok_or("Git did not return untracked file stats")?,
    )?;
    let fields: Vec<_> = header.splitn(3, '\t').collect();
    if fields.len() != 3 {
        return Err("Untracked file numstat format mismatch".into());
    }
    // --no-index encodes /dev/null -> path like a rename. Only this known
    // pseudo-source is allowed; normal numstat paths retain their strict guard.
    let target = if fields[2].is_empty() && records.len() == 3 && records[1] == b"/dev/null" {
        utf8(records[2])?
    } else if records.len() == 1 {
        fields[2].into()
    } else {
        return Err("Untracked file numstat source mismatch".into());
    };
    if target != expected_path {
        return Err("Untracked file numstat target mismatch".into());
    }
    parse_stats(fields[0], fields[1])
}
#[derive(Clone, Debug)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: String,
    pub head: Option<String>,
    pub bare: bool,
}
pub fn worktrees(bytes: &[u8]) -> Result<Vec<WorktreeEntry>, String> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for record in bytes.split(|byte| *byte == 0) {
        if record.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }
        let line = utf8(record)?;
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(WorktreeEntry {
                path: path.into(),
                branch: "(detached)".into(),
                head: None,
                bare: false,
            });
        } else if let Some(entry) = current.as_mut() {
            if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                entry.branch = branch.into();
            } else if let Some(head) = line.strip_prefix("HEAD ") {
                entry.head = (!head.bytes().all(|byte| byte == b'0')).then(|| head.into());
            } else if line == "bare" {
                entry.bare = true;
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    Ok(entries)
}
