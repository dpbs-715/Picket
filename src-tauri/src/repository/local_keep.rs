use super::{
    ExportedLocalKeep, MAX_LOCAL_KEEP_PATHS_PER_REPO, RepositoryStore, Result, display_with_locale,
    identity, is_valid_local_keep_path,
    types::{ReadTarget, RepositorySnapshot},
};
use crate::i18n::{self, LanguageKind};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::Path,
};

impl RepositoryStore {
    pub async fn set_local_keep(
        &self,
        target: &ReadTarget,
        paths: &[String],
        keep: bool,
        locale: LanguageKind,
    ) -> Result<RepositorySnapshot> {
        if paths.is_empty() {
            return Err(i18n::repository::local_keep_empty_paths_error(locale).into());
        }
        if paths.len() > MAX_LOCAL_KEEP_PATHS_PER_REPO {
            return Err(i18n::repository::local_keep_limit_exceeded_error(locale).into());
        }
        if !paths.iter().all(|path| is_valid_local_keep_path(path)) {
            return Err(i18n::repository::local_keep_invalid_path_error(locale).into());
        }
        let _write_guard = self.track_write();
        let path = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?
            .get(&target.repository_id)
            .and_then(|repository| repository.paths.get(&target.worktree_id))
            .cloned()
            .ok_or_else(|| i18n::repository::repo_not_opened_error(locale))?;
        let reader = self.inner.reader.with_locale(locale);
        let common = reader
            .text(
                &path,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?;
        let common = fs::canonicalize(common)
            .map_err(|_| "Cannot access Git common directory".to_string())?;
        if identity("repo", &common) != target.repository_id {
            return Err(i18n::repository::repo_identity_changed_error(locale).into());
        }
        let _guard = self
            .repository_gate(&target.repository_id)?
            .lock_owned()
            .await;
        self.resolve_keep_set_for_common(&common)?;
        {
            let mut keep_map = self
                .inner
                .local_keep
                .lock()
                .map_err(|_| "Local Keep registry is unavailable")?;
            let set = keep_map.entry(common.clone()).or_default();
            if keep {
                let added_count = paths
                    .iter()
                    .filter(|candidate| !set.contains(candidate.as_str()))
                    .collect::<HashSet<_>>()
                    .len();
                if set.len() + added_count > MAX_LOCAL_KEEP_PATHS_PER_REPO {
                    return Err(i18n::repository::local_keep_limit_exceeded_error(locale).into());
                }
                for item in paths {
                    set.insert(item.clone());
                }
            } else {
                for item in paths {
                    set.remove(item.as_str());
                }
            }
        }
        let snapshot = self.read_repository_locked(&path, locale).await?;
        if snapshot.repository.id != target.repository_id {
            return Err(i18n::repository::repo_identity_changed_error(locale).into());
        }
        Ok(snapshot)
    }

    pub(crate) fn import_persisted_local_keep(
        &self,
        entries: &[(String, Vec<String>)],
    ) -> Result<()> {
        let mut live = self
            .inner
            .local_keep
            .lock()
            .map_err(|_| "Local Keep registry is unavailable")?;
        let mut offline = self
            .inner
            .offline_local_keep
            .lock()
            .map_err(|_| "Offline Local Keep registry is unavailable")?;
        live.clear();
        offline.clear();
        for (common_dir, raw_paths) in entries {
            if common_dir.is_empty() {
                continue;
            }
            let sanitized: BTreeSet<String> = raw_paths
                .iter()
                .filter(|path| is_valid_local_keep_path(path))
                .take(MAX_LOCAL_KEEP_PATHS_PER_REPO)
                .cloned()
                .collect();
            if sanitized.is_empty() {
                continue;
            }
            match fs::canonicalize(common_dir) {
                Ok(canon) => {
                    let set = live.entry(canon).or_default();
                    for item in sanitized {
                        if set.len() >= MAX_LOCAL_KEEP_PATHS_PER_REPO {
                            break;
                        }
                        set.insert(item);
                    }
                }
                Err(_) => {
                    offline.insert(common_dir.clone(), sanitized.into_iter().collect());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn export_persisted_local_keep(
        &self,
        locale: LanguageKind,
    ) -> Result<ExportedLocalKeep> {
        let repositories = self
            .inner
            .repositories
            .lock()
            .map_err(|_| "Repository registry is unavailable")?;
        let live = self
            .inner
            .local_keep
            .lock()
            .map_err(|_| "Local Keep registry is unavailable")?;
        let offline = self
            .inner
            .offline_local_keep
            .lock()
            .map_err(|_| "Offline Local Keep registry is unavailable")?;
        let mut known = HashSet::with_capacity(repositories.len() + live.len() + offline.len());
        for reg in repositories.values() {
            known.insert(display_with_locale(&reg.common_dir, locale)?);
        }
        let mut out = BTreeMap::<String, Vec<String>>::new();
        for (common_path, set) in live.iter() {
            let key = display_with_locale(common_path, locale)?;
            known.insert(key.clone());
            if !set.is_empty() {
                out.insert(key, set.iter().cloned().collect());
            }
        }
        for (common_str, paths) in offline.iter() {
            if !known.contains(common_str) {
                known.insert(common_str.clone());
                if !paths.is_empty() {
                    out.insert(common_str.clone(), paths.clone());
                }
            }
        }
        Ok(ExportedLocalKeep {
            entries: out.into_iter().collect(),
            known_common_dirs: known,
        })
    }

    pub(crate) fn offline_local_keep_for_repo_id(
        &self,
        repository_id: &str,
    ) -> Option<Vec<String>> {
        let offline = self.inner.offline_local_keep.lock().ok()?;
        offline
            .iter()
            .find(|(common_dir, _)| identity("repo", Path::new(common_dir)) == repository_id)
            .map(|(_, paths)| paths.clone())
    }

    pub(super) fn resolve_keep_set_for_common(&self, common: &Path) -> Result<BTreeSet<String>> {
        let migrated = if let Some(common_str) = common.to_str() {
            self.inner
                .offline_local_keep
                .lock()
                .map_err(|_| "Offline Local Keep registry is unavailable")?
                .remove(common_str)
        } else {
            None
        };
        let mut keep_map = self
            .inner
            .local_keep
            .lock()
            .map_err(|_| "Local Keep registry is unavailable")?;
        let set = keep_map.entry(common.to_path_buf()).or_default();
        if let Some(paths) = migrated {
            for item in paths {
                if set.len() >= MAX_LOCAL_KEEP_PATHS_PER_REPO {
                    break;
                }
                if is_valid_local_keep_path(&item) {
                    set.insert(item);
                }
            }
        }
        Ok(set.clone())
    }
}
